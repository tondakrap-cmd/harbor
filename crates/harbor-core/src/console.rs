//! SQL console: one statement on its own connection, in a background thread. Reads run
//! in a read-only transaction with a time limit; at most CONSOLE_ROW_LIMIT rows are
//! fetched, then the connection is dropped so the server stops sending.
use crate::*;
use futures_util::TryStreamExt;
use sqlx::{mysql::MySqlConnectOptions, Column, Connection, MySqlConnection, Row};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Clone)]
pub struct ConsoleJob {
    pub statement: String,
    pub started: Instant,
    state: Arc<Mutex<Option<std::result::Result<QueryResult, String>>>>,
    connection_id: Arc<AtomicU64>,
    options: Option<MySqlConnectOptions>,
}
impl ConsoleJob {
    pub fn result(&self) -> Option<std::result::Result<QueryResult, String>> {
        self.state.lock().ok().and_then(|s| s.clone())
    }
    pub fn running(&self) -> bool {
        self.state.lock().map(|s| s.is_none()).unwrap_or(false)
    }
    /// KILL QUERY on the console's connection, from a separate short connection.
    pub fn cancel(&self) {
        let id = self.connection_id.load(Ordering::Relaxed);
        let Some(options) = self.options.clone() else {
            return;
        };
        if id == 0 {
            return;
        }
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async move {
                    if let Ok(mut conn) = MySqlConnection::connect_with(&options).await {
                        let _ = sqlx::raw_sql(&format!("KILL QUERY {id}"))
                            .execute(&mut conn)
                            .await;
                    }
                });
            }
        });
    }
}
pub(crate) fn spawn(
    options: Option<MySqlConnectOptions>,
    database: String,
    statement: String,
    kind: ConsoleKind,
) -> ConsoleJob {
    let job = ConsoleJob {
        statement: statement.clone(),
        started: Instant::now(),
        state: Arc::new(Mutex::new(None)),
        connection_id: Arc::new(AtomicU64::new(0)),
        options: options.clone(),
    };
    let state = Arc::clone(&job.state);
    let connection_id = Arc::clone(&job.connection_id);
    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => {
                runtime.block_on(run(options, &database, &statement, kind, &connection_id))
            }
            Err(_) => Err(ConsoleError::Other(Error::Worker)),
        };
        if let Ok(mut slot) = state.lock() {
            *slot = Some(result.map_err(|e| e.to_string()));
        }
    });
    job
}
/// The user typed the statement: show the server's own message (it is about their SQL).
enum ConsoleError {
    Server(u16, String),
    Other(Error),
}
impl std::fmt::Display for ConsoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server(1317 | 1969, _) => {
                write!(f, "Dotaz byl zrušen nebo překročil časový limit {CONSOLE_SECONDS} s.")
            }
            Self::Server(1792, _) => write!(
                f,
                "Příkaz se pokusil zapisovat v režimu jen pro čtení (konzole čte v transakci READ ONLY)."
            ),
            Self::Server(code, message) => write!(f, "Chyba {code}: {}", safe_text(message)),
            Self::Other(error) => write!(f, "{error}"),
        }
    }
}
impl From<sqlx::Error> for ConsoleError {
    fn from(error: sqlx::Error) -> Self {
        if let sqlx::Error::Database(e) = &error {
            if let Some(mysql) = e.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>() {
                return Self::Server(mysql.number(), mysql.message().to_string());
            }
        }
        Self::Other(error.into())
    }
}
impl From<Error> for ConsoleError {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}
async fn run(
    options: Option<MySqlConnectOptions>,
    database: &str,
    statement: &str,
    kind: ConsoleKind,
    connection_id: &AtomicU64,
) -> std::result::Result<QueryResult, ConsoleError> {
    let Some(options) = options else {
        return Ok(demo_result(statement));
    };
    let started = Instant::now();
    let mut conn = MySqlConnection::connect_with(&options)
        .await
        .map_err(|e| ConsoleError::Other(e.into()))?;
    let id: u64 = sqlx::query_scalar("SELECT CAST(CONNECTION_ID() AS UNSIGNED)")
        .fetch_one(&mut conn)
        .await?;
    connection_id.store(id, Ordering::Relaxed);
    if !database.is_empty() {
        sqlx::raw_sql(&format!("USE {}", backup::quote_ident(database)))
            .execute(&mut conn)
            .await?;
    }
    let mut result = QueryResult {
        statement: statement.to_string(),
        ..Default::default()
    };
    if kind == ConsoleKind::Write {
        let done = sqlx::raw_sql(statement).execute(&mut conn).await?;
        result.affected = Some(done.rows_affected());
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        return Ok(result);
    }
    // Reads cannot change anything: a read-only transaction rejects writes (also from
    // functions), and SELECTs get a server-side time limit.
    sqlx::raw_sql("START TRANSACTION READ ONLY")
        .execute(&mut conn)
        .await?;
    let upper = statement.trim_start().to_ascii_uppercase();
    let text = if upper.starts_with("SELECT") || upper.starts_with("WITH") {
        format!("SET STATEMENT max_statement_time={CONSOLE_SECONDS} FOR {statement}")
    } else {
        statement.to_string()
    };
    {
        let mut stream = sqlx::raw_sql(&text).fetch(&mut conn);
        while let Some(row) = stream.try_next().await? {
            if result.columns.is_empty() {
                result.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
            }
            if result.rows.len() >= CONSOLE_ROW_LIMIT {
                result.truncated = true;
                break;
            }
            result.rows.push(
                (0..row.columns().len())
                    .map(|i| cell_text(&row, i))
                    .collect(),
            );
        }
    }
    result.elapsed_ms = started.elapsed().as_millis() as u64;
    if result.truncated {
        // Unread rows would otherwise be drained; closing the socket stops the server.
        drop(conn);
    } else {
        let _ = sqlx::raw_sql("ROLLBACK").execute(&mut conn).await;
        let _ = conn.close().await;
    }
    Ok(result)
}
/// Text protocol: every value arrives as text; binary data is shown as hex.
fn cell_text(row: &sqlx::mysql::MySqlRow, index: usize) -> Option<String> {
    if let Ok(value) = row.try_get_unchecked::<Option<String>, _>(index) {
        return value.map(|v| safe_text(&v));
    }
    row.try_get_unchecked::<Option<Vec<u8>>, _>(index)
        .ok()
        .flatten()
        .map(|bytes| {
            let shown: String = bytes.iter().take(64).map(|b| format!("{b:02X}")).collect();
            format!(
                "0x{shown}{}",
                if bytes.len() > 64 {
                    format!("… ({} B)", bytes.len())
                } else {
                    String::new()
                }
            )
        })
}
fn demo_result(statement: &str) -> QueryResult {
    QueryResult {
        statement: statement.to_string(),
        columns: vec!["id".into(), "customer".into(), "total".into()],
        rows: (1..=5)
            .map(|i| {
                vec![
                    Some(i.to_string()),
                    Some(format!("Zákazník {i} (demo)")),
                    if i == 3 {
                        None
                    } else {
                        Some(format!("{}.50", i * 100))
                    },
                ]
            })
            .collect(),
        truncated: false,
        affected: None,
        elapsed_ms: 3,
    }
}
