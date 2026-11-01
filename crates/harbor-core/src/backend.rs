use crate::*;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    ConnectOptions, MySqlPool, Row,
};
use std::{collections::HashMap, time::Duration};
use zeroize::Zeroizing;

pub struct Backend {
    inner: Kind,
}
enum Kind {
    Demo(Snapshot),
    MariaDb(MySqlPool),
}
impl Backend {
    pub fn demo() -> Self {
        Self {
            inner: Kind::Demo(Snapshot {
                version: "11.4 • simulace".into(),
                uptime: 86400,
                questions: 1000,
                connections: 4,
                max_connections: 150,
                databases: [
                    ("commerce", 128, 28_400_000_000),
                    ("analytics", 64, 11_200_000_000),
                    ("identity", 18, 2_100_000_000),
                    ("mysql", 31, 8_400_000),
                ]
                .into_iter()
                .map(|(name, tables, bytes)| Database {
                    name: name.into(),
                    tables,
                    bytes,
                    charset: "utf8mb4".into(),
                    collation: "utf8mb4_unicode_ci".into(),
                })
                .collect(),
                accounts: vec![
                    Account {
                        user: "harbor_demo".into(),
                        host: "localhost".into(),
                        plugin: "mysql_native_password".into(),
                    },
                    Account {
                        user: "analytics_ro".into(),
                        host: "10.20.6.%".into(),
                        plugin: "mysql_native_password".into(),
                    },
                ],
                processes: vec![
                    Process {
                        id: 1842,
                        user: "analytics_ro".into(),
                        host: "demo:5000".into(),
                        database: "analytics".into(),
                        command: "Query".into(),
                        seconds: 47,
                        state: "Sending data".into(),
                        query: "SELECT date, SUM(total) FROM daily_sales GROUP BY date".into(),
                    },
                    Process {
                        id: 1871,
                        user: "commerce_app".into(),
                        host: "demo:5001".into(),
                        database: "commerce".into(),
                        command: "Sleep".into(),
                        seconds: 90,
                        state: "".into(),
                        query: "".into(),
                    },
                ],
                notices: vec!["DEMO: ukázková data, změny platí jen v paměti do odpojení.".into()],
                timezone: "Nezjištěno (demo)".into(),
                demo: true,
            }),
        }
    }
    pub async fn connect(profile: &Profile, password: Secret) -> Result<Self> {
        profile.validate()?;
        let mut options = MySqlConnectOptions::new()
            .username(&profile.user)
            .disable_statement_logging();
        if !password.0.is_empty() {
            options = options.password(&password.0);
        }
        options =
            match &profile.transport {
                Transport::Tcp { host, port } => options.host(host).port(*port).ssl_mode(
                    if profile.tls == TlsMode::VerifyIdentity {
                        MySqlSslMode::VerifyIdentity
                    } else {
                        MySqlSslMode::Disabled
                    },
                ),
                Transport::Unix { path } => options.socket(path).ssl_mode(MySqlSslMode::Disabled),
            };
        if let Some(ca) = &profile.ca {
            options = options.ssl_ca(ca);
        }
        let pool = MySqlPoolOptions::new()
            .max_connections(1)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(8))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    // Quoting of DDL string literals below has exactly this documented SQL mode.
                    sqlx::query(
                        "SET SESSION sql_mode = 'NO_BACKSLASH_ESCAPES,NO_AUTO_CREATE_USER'",
                    )
                    .execute(connection)
                    .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        let version: String = sqlx::query_scalar("SELECT VERSION()")
            .fetch_one(&pool)
            .await?;
        if !version.contains("MariaDB") {
            pool.close().await;
            return Err(Error::Unsupported(
                "Tento klient je určený pro MariaDB.".into(),
            ));
        }
        Ok(Self {
            inner: Kind::MariaDb(pool),
        })
    }
    pub async fn snapshot(&mut self) -> Result<Snapshot> {
        let pool = match &mut self.inner {
            Kind::Demo(snapshot) => {
                snapshot.questions += 125;
                snapshot.uptime += 5;
                return Ok(snapshot.clone());
            }
            Kind::MariaDb(pool) => pool,
        };
        let row = sqlx::query("SELECT VERSION() AS version, @@time_zone AS timezone, CAST(@@max_connections AS UNSIGNED) AS maximum").fetch_one(&*pool).await?;
        let mut snapshot = Snapshot {
            version: row.try_get("version")?,
            timezone: row.try_get("timezone")?,
            max_connections: row.try_get("maximum")?,
            ..Default::default()
        };
        let status: HashMap<String, String> = sqlx::query(
            "SHOW GLOBAL STATUS WHERE Variable_name IN ('Uptime','Questions','Threads_connected')",
        )
        .fetch_all(&*pool)
        .await?
        .into_iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
        let metric = |name: &str| status.get(name).and_then(|v| v.parse().ok()).unwrap_or(0);
        snapshot.uptime = metric("Uptime");
        snapshot.questions = metric("Questions");
        snapshot.connections = metric("Threads_connected");
        snapshot.databases = sqlx::query("SELECT s.SCHEMA_NAME AS name, s.DEFAULT_CHARACTER_SET_NAME AS charset, s.DEFAULT_COLLATION_NAME AS collation, CAST(COUNT(t.TABLE_NAME) AS UNSIGNED) AS tables, CAST(COALESCE(SUM(t.DATA_LENGTH + t.INDEX_LENGTH),0) AS UNSIGNED) AS bytes FROM information_schema.SCHEMATA s LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA=s.SCHEMA_NAME GROUP BY s.SCHEMA_NAME,s.DEFAULT_CHARACTER_SET_NAME,s.DEFAULT_COLLATION_NAME ORDER BY s.SCHEMA_NAME").fetch_all(&*pool).await?.iter().map(|r| Ok(Database { name: r.try_get("name")?, charset: r.try_get("charset")?, collation: r.try_get("collation")?, tables: r.try_get("tables")?, bytes: r.try_get("bytes")? })).collect::<std::result::Result<_, sqlx::Error>>()?;
        match sqlx::query("SELECT CONVERT(User USING utf8mb4) COLLATE utf8mb4_unicode_ci AS User, CONVERT(Host USING utf8mb4) COLLATE utf8mb4_unicode_ci AS Host, CONVERT(plugin USING utf8mb4) COLLATE utf8mb4_unicode_ci AS plugin FROM mysql.user ORDER BY User,Host")
            .fetch_all(&*pool)
            .await
        {
            Ok(rows) => {
                snapshot.accounts = rows
                    .iter()
                    .map(|r| {
                        Ok(Account {
                            user: r.try_get("User")?,
                            host: r.try_get("Host")?,
                            plugin: r
                                .try_get::<Option<String>, _>("plugin")?
                                .unwrap_or_default(),
                        })
                    })
                    .collect::<std::result::Result<_, sqlx::Error>>()?
            }
            Err(error) => {
                snapshot.notices.push(format!(
                    "Seznam všech účtů není dostupný: {}",
                    Error::from(error)
                ));
                let current: String = sqlx::query_scalar("SELECT CURRENT_USER()")
                    .fetch_one(&*pool)
                    .await?;
                if let Some((user, host)) = current.rsplit_once('@') {
                    snapshot.accounts.push(Account {
                        user: user.into(),
                        host: host.into(),
                        plugin: "Nezjištěno".into(),
                    });
                }
            }
        }
        snapshot.processes = processes(pool).await?;
        snapshot.notices.push("Procesy a databáze jsou viditelné jen v rozsahu práv účtu. Operační systém: Nezjištěno. Velikosti jsou přibližné.".into());
        Ok(snapshot)
    }
    pub async fn tables(&self, database: &str) -> Result<Vec<TableInfo>> {
        match &self.inner {
            Kind::Demo(_) => Ok(vec![TableInfo { name: "daily_sales (demo)".into(), engine: "InnoDB".into(), rows: 4096, bytes: 1_048_576 }]),
            Kind::MariaDb(pool) => Ok(sqlx::query("SELECT TABLE_NAME, COALESCE(ENGINE,'VIEW') AS engine, CAST(COALESCE(TABLE_ROWS,0) AS UNSIGNED) AS rows_count, CAST(COALESCE(DATA_LENGTH+INDEX_LENGTH,0) AS UNSIGNED) AS bytes FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME").bind(database).fetch_all(pool).await?.iter().map(|r| Ok(TableInfo { name: r.try_get("TABLE_NAME")?, engine: r.try_get("engine")?, rows: r.try_get("rows_count")?, bytes: r.try_get("bytes")? })).collect::<std::result::Result<_, sqlx::Error>>()?),
        }
    }
    pub async fn grants(&self, account: &Account) -> Result<Vec<String>> {
        match &self.inner {
            Kind::Demo(_) => Ok(vec![format!(
                "DEMO: SELECT na analytics pro {}",
                account.label()
            )]),
            Kind::MariaDb(pool) => {
                let current: String = sqlx::query_scalar("SELECT CURRENT_USER()")
                    .fetch_one(pool)
                    .await?;
                let query = if current == account.label() {
                    "SHOW GRANTS".into()
                } else {
                    format!("SHOW GRANTS FOR {}", account_sql(account)?)
                };
                // MariaDB can include authentication hashes in SHOW GRANTS. Never return them to the UI.
                Ok(sqlx::query(&query)
                    .fetch_all(pool)
                    .await?
                    .iter()
                    .map(|r| redact_grant(r.get::<String, _>(0)))
                    .collect())
            }
        }
    }
    pub async fn apply(&mut self, change: Change) -> Result<()> {
        if let Kind::Demo(snapshot) = &mut self.inner {
            match change {
                Change::CreateDatabase { name, collation } => { identifier(&name)?; if snapshot.databases.iter().any(|d| d.name == name) { return Err(Error::Validation("Databáze už existuje.".into())); } snapshot.databases.push(Database { name, tables: 0, bytes: 0, charset: "utf8mb4".into(), collation }); },
                Change::CreateAccount { user, host, .. } => { validate_account(&user, &host)?; snapshot.accounts.push(Account { user, host, plugin: "mysql_native_password".into() }); },
                Change::KillQuery { target } => { let p = snapshot.processes.iter_mut().find(|p| p.id == target.id && p.query == target.query).ok_or_else(|| Error::Validation("Cílový dotaz se změnil.".into()))?; p.command = "Sleep".into(); p.query.clear(); p.seconds = 0; },
                _ => return Err(Error::Unsupported("Demo simuluje vytvoření databáze/účtu a ukončení dotazu. Ostatní zápisy vyžadují testovací server.".into())),
            }
            return Ok(());
        }
        let Kind::MariaDb(pool) = &self.inner else {
            unreachable!()
        };
        let sql = match change {
            Change::CreateDatabase { name, collation } => {
                let charset: Option<String> = sqlx::query_scalar("SELECT CHARACTER_SET_NAME FROM information_schema.COLLATIONS WHERE COLLATION_NAME=?").bind(&collation).fetch_optional(pool).await?;
                let charset = charset
                    .ok_or_else(|| Error::Validation("Server tuto kolaci nepodporuje.".into()))?;
                format!(
                    "CREATE DATABASE {} CHARACTER SET {} COLLATE {}",
                    identifier(&name)?,
                    identifier(&charset)?,
                    identifier(&collation)?
                )
            }
            Change::CreateAccount {
                user,
                host,
                password,
            } => {
                validate_password(&password)?;
                let account = Account {
                    user,
                    host,
                    plugin: "mysql_native_password".into(),
                };
                format!(
                    "CREATE USER {} IDENTIFIED BY {}",
                    account_sql(&account)?,
                    quoted(&password.0)
                )
            }
            Change::ChangePassword { account, password } => {
                validate_password(&password)?;
                let plugin: Option<String> =
                    sqlx::query_scalar("SELECT CONVERT(plugin USING utf8mb4) COLLATE utf8mb4_unicode_ci FROM mysql.user WHERE User=? AND Host=?")
                        .bind(&account.user)
                        .bind(&account.host)
                        .fetch_optional(pool)
                        .await?;
                // mysql.user alone does not expose alternate auth methods: inspect global_priv too.
                let methods: Option<u64> = sqlx::query_scalar("SELECT CAST(COALESCE(JSON_LENGTH(JSON_EXTRACT(Priv,'$.auth_or')),0) AS UNSIGNED) FROM mysql.global_priv WHERE User=? AND Host=?").bind(&account.user).bind(&account.host).fetch_optional(pool).await?;
                if plugin.as_deref() != Some("mysql_native_password") || methods != Some(0) {
                    return Err(Error::Unsupported("Změna hesla je povolena pouze pro ověřený samostatný mysql_native_password; jiné mechanismy se nepřepisují.".into()));
                }
                format!(
                    "SET PASSWORD FOR {} = PASSWORD({})",
                    account_sql(&account)?,
                    quoted(&password.0)
                )
            }
            Change::DatabaseGrant {
                account,
                database,
                privilege,
                grant,
            } => format!(
                "{} {} ON {}.* {} {}",
                if grant { "GRANT" } else { "REVOKE" },
                privilege.sql(),
                grant_identifier(&database)?,
                if grant { "TO" } else { "FROM" },
                account_sql(&account)?
            ),
            Change::KillQuery { target } => {
                let current = processes(pool)
                    .await?
                    .into_iter()
                    .find(|p| p.id == target.id)
                    .ok_or_else(|| Error::Validation("Dotaz už neběží.".into()))?;
                if current.user != target.user
                    || current.host != target.host
                    || current.database != target.database
                    || current.query != target.query
                    || !current.running_query()
                    || current.seconds < target.seconds
                {
                    return Err(Error::Validation(
                        "Cíl se změnil; znovu otevřete detail.".into(),
                    ));
                }
                format!("KILL QUERY {}", target.id)
            }
        };
        let sql = Zeroizing::new(sql);
        sqlx::query(&sql).execute(pool).await?;
        Ok(())
    }
}
async fn processes(pool: &MySqlPool) -> Result<Vec<Process>> {
    Ok(sqlx::query("SELECT CAST(ID AS UNSIGNED) AS ID,USER,HOST,COALESCE(DB,'') AS db_name,COMMAND,CAST(TIME AS UNSIGNED) AS elapsed,COALESCE(STATE,'') AS state_name,COALESCE(INFO,'') AS query_text FROM information_schema.PROCESSLIST WHERE ID <> CONNECTION_ID() ORDER BY TIME DESC").fetch_all(pool).await?.iter().map(|r| Ok(Process { id: r.try_get("ID")?, user: r.try_get("USER")?, host: r.try_get("HOST")?, database: r.try_get("db_name")?, command: r.try_get("COMMAND")?, seconds: r.try_get("elapsed")?, state: r.try_get("state_name")?, query: r.try_get("query_text")? })).collect::<std::result::Result<_, sqlx::Error>>()?)
}
fn validate_password(password: &Secret) -> Result<()> {
    if password.0.is_empty() {
        Err(Error::Validation("Nové heslo nesmí být prázdné.".into()))
    } else {
        Ok(())
    }
}
fn validate_account(user: &str, host: &str) -> Result<()> {
    if user.is_empty()
        || user.len() > 80
        || host.is_empty()
        || host.len() > 255
        || user.chars().chain(host.chars()).any(char::is_control)
    {
        Err(Error::Validation("Neplatný uživatel nebo hostitel.".into()))
    } else {
        Ok(())
    }
}
fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn grant_identifier(value: &str) -> Result<String> {
    identifier(value)?;
    // GRANT treats % and _ as patterns even inside quoted database identifiers.
    Ok(format!(
        "`{}`",
        value
            .replace('`', "``")
            .replace('_', "\\_")
            .replace('%', "\\%")
    ))
}
fn account_sql(account: &Account) -> Result<String> {
    validate_account(&account.user, &account.host)?;
    Ok(format!(
        "{}@{}",
        quoted(&account.user),
        quoted(&account.host)
    ))
}
fn redact_grant(grant: String) -> String {
    let upper = grant.to_ascii_uppercase();
    match upper.find(" IDENTIFIED ") {
        Some(at) => format!("{} [autentizační údaje skryty]", &grant[..at]),
        None => grant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literals_and_grants_do_not_expose_or_escape_secrets() {
        assert_eq!(quoted("a'b\\c"), "'a''b\\c'");
        assert_eq!(grant_identifier("db_%`x").unwrap(), "`db\\_\\%``x`");
        assert!(!redact_grant(
            "GRANT USAGE ON *.* TO 'u'@'h' IDENTIFIED BY PASSWORD 'hash'".into()
        )
        .contains("hash"));
    }
}
