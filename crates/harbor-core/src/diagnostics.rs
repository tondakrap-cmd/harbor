//! Deeper diagnostics: locks and transactions, statement statistics (slow queries),
//! replication / Galera, binary logs and the SQL console. Types and pure logic here;
//! queries in `backend`.
use crate::*;
use std::collections::HashMap;

// ---------------------------------------------------------------- locks

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transaction {
    pub thread: u64,
    pub state: String,
    pub seconds: u64,
    pub rows_locked: u64,
    pub rows_modified: u64,
    pub tables_locked: u64,
    pub isolation: String,
    pub query: String,
    pub user: String,
    pub host: String,
    pub database: String,
    /// Processlist command of the session ("Sleep" = idle inside an open transaction).
    pub command: String,
}
impl Transaction {
    /// Open transaction whose session does nothing: the classic cause of lock waits.
    pub fn idle(&self) -> bool {
        self.command == "Sleep" && self.query.is_empty()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LockWait {
    pub waiting_thread: u64,
    pub waiting_query: String,
    pub waiting_seconds: u64,
    pub blocking_thread: u64,
    pub blocking_query: String,
    pub lock_table: String,
    pub lock_mode: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataWait {
    pub thread: u64,
    pub seconds: u64,
    pub database: String,
    pub query: String,
}
#[derive(Clone, Debug, Default)]
pub struct LockReport {
    pub transactions: Vec<Transaction>,
    pub waits: Vec<LockWait>,
    pub metadata_waits: Vec<MetadataWait>,
    /// Threads that block others and do not wait themselves (where to act).
    pub root_blockers: Vec<u64>,
    /// Missing PROCESS privilege etc.
    pub notes: Vec<String>,
}
/// Blocking threads that are not waiting themselves.
pub fn root_blockers(waits: &[LockWait]) -> Vec<u64> {
    let waiting: std::collections::HashSet<u64> = waits.iter().map(|w| w.waiting_thread).collect();
    let mut roots: Vec<u64> = waits
        .iter()
        .map(|w| w.blocking_thread)
        .filter(|t| !waiting.contains(t))
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots
}

// ---------------------------------------------------------------- statements

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatementStat {
    /// Normalized statement (literals replaced by ?).
    pub query: String,
    pub schema: String,
    pub count: u64,
    pub total_seconds: f64,
    pub max_seconds: f64,
    pub rows_examined: u64,
    pub rows_sent: u64,
    /// Executions that used no index (performance_schema only).
    pub no_index: u64,
    pub last_seen: String,
    /// One real execution (slow log), usable for EXPLAIN.
    pub sample: Option<String>,
}
impl StatementStat {
    pub fn average_ms(&self) -> f64 {
        self.total_seconds * 1000.0 / self.count.max(1) as f64
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatsSource {
    PerformanceSchema,
    SlowLogTable,
    /// The slow log is a file on the server: read through host access.
    SlowLogFile(String),
    /// Nothing collects statement statistics yet.
    Off,
}
#[derive(Clone, Debug, Default)]
pub struct Performance {
    pub source: Option<StatsSource>,
    pub statements: Vec<StatementStat>,
    pub performance_schema: bool,
    pub slow_query_log: bool,
    pub long_query_time: f64,
    pub log_output: String,
    pub slow_log_file: String,
}
/// Statement text with literals replaced, so executions differing only in values
/// group together. Keeps identifiers (also quoted ones) intact.
pub fn normalize_query(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    let mut last_space = true;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' | '"' => {
                // String literal (with backslash and doubled-quote escapes).
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == c {
                        if chars.get(i + 1) == Some(&c) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                out.push('?');
                i += 1;
                last_space = false;
            }
            '`' => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != '`' {
                    i += 1;
                }
                out.extend(&chars[start..(i + 1).min(chars.len())]);
                i += 1;
                last_space = false;
            }
            c if c.is_whitespace() => {
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
                i += 1;
            }
            c if c.is_ascii_digit() => {
                // Number (also 0x.., 1.5, 1e3) not part of an identifier.
                let previous_ident = out
                    .chars()
                    .last()
                    .is_some_and(|p| p.is_alphanumeric() || p == '_');
                if previous_ident {
                    out.push(c);
                    i += 1;
                    continue;
                }
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                    i += 1;
                }
                out.push('?');
                last_space = false;
            }
            _ => {
                out.push(c);
                i += 1;
                last_space = false;
            }
        }
    }
    // (?, ?, ?) lists of any length → (...)
    let text = out.trim().trim_end_matches(';').trim().to_string();
    let mut result = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '(' {
            let mut j = i + 1;
            let mut commas = 0;
            let mut only_values = true;
            while j < bytes.len() && bytes[j] != ')' {
                match bytes[j] {
                    '?' | ' ' => (),
                    ',' => commas += 1,
                    _ => only_values = false,
                }
                j += 1;
            }
            if j < bytes.len() && only_values && commas > 0 {
                result.push_str("(...)");
                i = j + 1;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    result
}
/// Aggregates a MariaDB slow query log (file format) into statement statistics.
pub fn aggregate_slow_log(text: &str) -> Vec<StatementStat> {
    let mut groups: HashMap<(String, String), StatementStat> = HashMap::new();
    let mut database = String::new();
    let (mut query_time, mut rows_sent, mut rows_examined) = (0.0f64, 0u64, 0u64);
    let mut time = String::new();
    let mut sql = String::new();
    let mut in_entry = false;
    let mut flush = |sql: &mut String,
                     database: &str,
                     time: &str,
                     query_time: f64,
                     rows_sent: u64,
                     rows_examined: u64| {
        let statement = sql.trim().to_string();
        sql.clear();
        if statement.is_empty() {
            return;
        }
        let key = (normalize_query(&statement), database.to_string());
        let entry = groups.entry(key.clone()).or_insert_with(|| StatementStat {
            query: key.0.clone(),
            schema: key.1.clone(),
            ..Default::default()
        });
        entry.count += 1;
        entry.total_seconds += query_time;
        entry.max_seconds = entry.max_seconds.max(query_time);
        entry.rows_sent += rows_sent;
        entry.rows_examined += rows_examined;
        if !time.is_empty() {
            entry.last_seen = time.to_string();
        }
        if entry.sample.is_none() || query_time >= entry.max_seconds {
            entry.sample = Some(statement.chars().take(20_000).collect());
        }
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# Time: ") {
            if in_entry {
                flush(
                    &mut sql,
                    &database,
                    &time,
                    query_time,
                    rows_sent,
                    rows_examined,
                );
            }
            time = rest.trim().to_string();
            in_entry = false;
        } else if line.starts_with("# User@Host:") {
            if in_entry {
                flush(
                    &mut sql,
                    &database,
                    &time,
                    query_time,
                    rows_sent,
                    rows_examined,
                );
            }
            in_entry = false;
        } else if let Some(rest) = line.strip_prefix("# Query_time:") {
            let value = |key: &str| {
                rest.split_whitespace()
                    .skip_while(|w| *w != key)
                    .nth(1)
                    .unwrap_or("0")
                    .to_string()
            };
            query_time = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            rows_sent = value("Rows_sent:").parse().unwrap_or(0);
            rows_examined = value("Rows_examined:").parse().unwrap_or(0);
            in_entry = true;
        } else if line.starts_with('#') {
            continue;
        } else if in_entry {
            let trimmed = line.trim();
            if let Some(db) = trimmed
                .strip_prefix("use ")
                .and_then(|r| r.strip_suffix(';'))
            {
                database = db.trim_matches('`').to_string();
            } else if trimmed.starts_with("SET timestamp=") && sql.is_empty() {
                continue;
            } else {
                if !sql.is_empty() {
                    sql.push('\n');
                }
                sql.push_str(line);
            }
        }
    }
    if in_entry {
        flush(
            &mut sql,
            &database,
            &time,
            query_time,
            rows_sent,
            rows_examined,
        );
    }
    let mut stats: Vec<StatementStat> = groups.into_values().collect();
    stats.sort_by(|a, b| b.total_seconds.total_cmp(&a.total_seconds));
    stats
}
/// Rows of mysql.slow_log (time, seconds, rows sent, rows examined, db, sql) aggregated.
pub fn aggregate_slow_rows(rows: &[(String, f64, u64, u64, String, String)]) -> Vec<StatementStat> {
    let mut text = String::new();
    for (time, seconds, sent, examined, db, sql) in rows {
        text.push_str(&format!(
            "# Time: {time}\n# User@Host: x\n# Query_time: {seconds}  Lock_time: 0  Rows_sent: {sent}  Rows_examined: {examined}\nuse {db};\n{sql};\n"
        ));
    }
    aggregate_slow_log(&text)
}

// ---------------------------------------------------------------- replication

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplicaChannel {
    /// Connection name; empty = the default connection.
    pub name: String,
    pub master: String,
    pub io_running: String,
    pub sql_running: String,
    pub seconds_behind: Option<u64>,
    pub using_gtid: String,
    pub gtid_io_pos: String,
    pub last_io_error: String,
    pub last_sql_error: String,
    pub relay_log_space: u64,
    pub state: String,
}
impl ReplicaChannel {
    pub fn healthy(&self) -> bool {
        self.io_running == "Yes" && self.sql_running == "Yes"
    }
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            "výchozí".into()
        } else {
            self.name.clone()
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Galera {
    pub cluster_name: String,
    pub node_name: String,
    pub cluster_size: u64,
    /// Primary / non-Primary / Disconnected
    pub cluster_status: String,
    /// Synced, Donor/Desynced, Joining…
    pub local_state: String,
    pub ready: bool,
    pub connected: bool,
    pub flow_control_paused: f64,
    pub recv_queue_avg: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BinaryLog {
    pub name: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Replication {
    pub server_id: u64,
    pub read_only: bool,
    pub log_bin: bool,
    pub binlog_format: String,
    pub gtid_binlog_pos: String,
    pub gtid_slave_pos: String,
    pub expire_seconds: f64,
    pub channels: Vec<ReplicaChannel>,
    /// Replicas registered on this primary (SHOW SLAVE HOSTS).
    pub replicas: Vec<String>,
    pub binlogs: Vec<BinaryLog>,
    pub galera: Option<Galera>,
    pub notes: Vec<String>,
}
impl Replication {
    pub fn binlog_bytes(&self) -> u64 {
        self.binlogs.iter().map(|b| b.bytes).sum()
    }
}
/// Binary log file names as MariaDB writes them.
pub fn validate_binlog_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err(Error::Validation("Neplatný název binárního logu.".into()));
    }
    Ok(())
}
/// Replication connection names: up to 64 characters, no quotes or control characters.
pub fn validate_connection_name(name: &str) -> Result<()> {
    if name.chars().count() > 64
        || name
            .chars()
            .any(|c| c.is_control() || c == '\'' || c == '\\')
    {
        return Err(Error::Validation(
            "Neplatný název replikačního spojení.".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------- console

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleKind {
    /// Runs inside START TRANSACTION READ ONLY … ROLLBACK.
    Read,
    /// Changes data or structure: needs explicit confirmation.
    Write,
}
/// Classifies one console statement. Several statements at once are refused, so a
/// harmless first statement cannot smuggle a second one through.
pub fn console_statement(sql: &str) -> Result<(String, ConsoleKind)> {
    let mut statements = backup::Statements::new(std::io::BufReader::new(sql.as_bytes()));
    let mut found = Vec::new();
    loop {
        match statements.next_statement() {
            Ok(Some(statement)) => found.push(statement),
            Ok(None) => break,
            // A missing final ';' is fine for a single console statement.
            Err(_) if found.is_empty() => {
                found.push(sql.trim().trim_end_matches(';').trim().to_string());
                break;
            }
            Err(error) => return Err(error),
        }
    }
    let statement = match found.as_slice() {
        [one] if !one.trim().is_empty() => one.trim().to_string(),
        [] => return Err(Error::Validation("Zadejte příkaz SQL.".into())),
        _ => {
            return Err(Error::Validation(
                "Konzole provádí jeden příkaz najednou; odstraňte další příkazy za středníkem."
                    .into(),
            ))
        }
    };
    // Executable comments may contain anything: treat as a write.
    let text = strip_leading_comments(&statement);
    if text.starts_with("/*!") || text.starts_with("/*M!") {
        return Ok((statement, ConsoleKind::Write));
    }
    let upper = text.to_ascii_uppercase();
    let first = upper
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|w| !w.is_empty())
        .unwrap_or("");
    let read = matches!(
        first,
        "SELECT"
            | "WITH"
            | "SHOW"
            | "DESCRIBE"
            | "DESC"
            | "EXPLAIN"
            | "ANALYZE"
            | "HELP"
            | "VALUES"
            | "TABLE"
    ) && !upper.contains(" INTO OUTFILE")
        && !upper.contains(" INTO DUMPFILE")
        // ANALYZE TABLE rewrites statistics; ANALYZE FORMAT=JSON SELECT only reads.
        && (first != "ANALYZE" || !upper.contains("TABLE") || upper.contains("FORMAT"));
    Ok((
        statement,
        if read {
            ConsoleKind::Read
        } else {
            ConsoleKind::Write
        },
    ))
}
fn strip_leading_comments(sql: &str) -> &str {
    let mut text = sql.trim_start();
    loop {
        if text.starts_with("/*") && !text.starts_with("/*!") && !text.starts_with("/*M!") {
            match text.find("*/") {
                Some(end) => text = text[end + 2..].trim_start(),
                None => return text,
            }
        } else if text.starts_with("-- ") || text.starts_with('#') {
            match text.find('\n') {
                Some(end) => text = text[end + 1..].trim_start(),
                None => return "",
            }
        } else {
            return text;
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryResult {
    pub statement: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows existed than were fetched.
    pub truncated: bool,
    pub affected: Option<u64>,
    pub elapsed_ms: u64,
}
impl QueryResult {
    /// CSV (RFC 4180, comma, CRLF) of the fetched rows; NULL as an empty field.
    pub fn csv(&self) -> String {
        let field = |v: &str| {
            if v.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", v.replace('"', "\"\""))
            } else {
                v.to_string()
            }
        };
        let mut out = self
            .columns
            .iter()
            .map(|c| field(c))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str("\r\n");
        for row in &self.rows {
            out.push_str(
                &row.iter()
                    .map(|v| v.as_deref().map(field).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            out.push_str("\r\n");
        }
        out
    }
}
pub const CONSOLE_ROW_LIMIT: usize = 1000;
pub const CONSOLE_SECONDS: u64 = 60;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queries_normalize_to_one_shape() {
        assert_eq!(
            normalize_query(
                "SELECT * FROM t1 WHERE id = 42 AND name='O''Brien'  AND x IN (1, 2,3);"
            ),
            "SELECT * FROM t1 WHERE id = ? AND name=? AND x IN (...)"
        );
        assert_eq!(
            normalize_query("select `col 1`, c2 from `db`.`t2` where v > 1.5e3 and h = 0xFF"),
            "select `col 1`, c2 from `db`.`t2` where v > ? and h = ?"
        );
        assert_eq!(normalize_query("SELECT \"a\\\"b\""), "SELECT ?");
    }
    #[test]
    fn slow_log_files_aggregate_by_statement_shape() {
        let log = "/usr/sbin/mariadbd, Version: 10.5.29-MariaDB-log. started with:\n\
            Tcp port: 3306  Unix socket: /var/lib/mysql/mysql.sock\n\
            Time\t\t    Id Command\tArgument\n\
            # Time: 261101 10:00:00\n\
            # User@Host: app[app] @ localhost []\n\
            # Thread_id: 8  Schema: shop  QC_hit: No\n\
            # Query_time: 2.500000  Lock_time: 0.000100  Rows_sent: 1  Rows_examined: 50000\n\
            # Rows_affected: 0  Bytes_sent: 60\n\
            use shop;\n\
            SET timestamp=1793541600;\n\
            SELECT * FROM orders WHERE customer = 17;\n\
            # Time: 261101 10:05:00\n\
            # User@Host: app[app] @ localhost []\n\
            # Query_time: 4.000000  Lock_time: 0.0  Rows_sent: 3  Rows_examined: 90000\n\
            SET timestamp=1793541900;\n\
            SELECT * FROM orders\n  WHERE customer = 99;\n\
            # User@Host: admin[admin] @ localhost []\n\
            # Query_time: 1.000000  Lock_time: 0.0  Rows_sent: 0  Rows_examined: 10\n\
            SET timestamp=1793541901;\n\
            UPDATE stock SET n = n - 1 WHERE id = 5;\n";
        let stats = aggregate_slow_log(log);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].query, "SELECT * FROM orders WHERE customer = ?");
        assert_eq!(stats[0].schema, "shop");
        assert_eq!(stats[0].count, 2);
        assert!((stats[0].total_seconds - 6.5).abs() < 1e-9);
        assert_eq!(stats[0].max_seconds, 4.0);
        assert_eq!(stats[0].rows_examined, 140_000);
        assert!(
            stats[0].sample.as_deref().unwrap().contains("99"),
            "slowest execution kept"
        );
        assert_eq!(stats[1].count, 1);
    }
    #[test]
    fn console_runs_one_statement_and_classifies_writes() {
        let kind = |sql: &str| console_statement(sql).map(|(_, k)| k);
        assert_eq!(kind("SELECT 1").unwrap(), ConsoleKind::Read);
        assert_eq!(kind("  /* x */ show tables;").unwrap(), ConsoleKind::Read);
        assert_eq!(
            kind("WITH a AS (SELECT 1) SELECT * FROM a").unwrap(),
            ConsoleKind::Read
        );
        assert_eq!(kind("EXPLAIN SELECT * FROM t").unwrap(), ConsoleKind::Read);
        for write in [
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "DROP TABLE t",
            "SELECT * FROM t INTO OUTFILE '/tmp/x'",
            "/*!50000 DROP TABLE t */",
            "ANALYZE TABLE t",
            "SET GLOBAL x = 1",
        ] {
            assert_eq!(kind(write).unwrap(), ConsoleKind::Write, "{write}");
        }
        assert!(
            kind("SELECT 1; DROP TABLE t").is_err(),
            "second statement refused"
        );
        assert!(kind("   ").is_err());
        assert_eq!(console_statement("SELECT ';'").unwrap().0, "SELECT ';'");
    }
    #[test]
    fn root_blockers_are_threads_that_block_without_waiting() {
        let wait = |waiting, blocking| LockWait {
            waiting_thread: waiting,
            blocking_thread: blocking,
            ..Default::default()
        };
        // 3 waits for 2, 2 waits for 1: only 1 is the root.
        assert_eq!(
            root_blockers(&[wait(3, 2), wait(2, 1), wait(4, 1)]),
            vec![1]
        );
        assert!(root_blockers(&[]).is_empty());
    }
    #[test]
    fn csv_export_quotes_fields() {
        let result = QueryResult {
            columns: vec!["a".into(), "b,c".into()],
            rows: vec![vec![Some("x\"y".into()), None]],
            ..Default::default()
        };
        assert_eq!(result.csv(), "a,\"b,c\"\r\n\"x\"\"y\",\r\n");
        assert!(validate_binlog_name("mysql-bin.000042").is_ok());
        assert!(validate_binlog_name("x'; DROP").is_err());
        assert!(validate_connection_name("east").is_ok());
        assert!(validate_connection_name("a'b").is_err());
    }
}
