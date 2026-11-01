use crate::{sql, *};
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode},
    ConnectOptions, MySqlPool, Row,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub struct Backend {
    inner: Kind,
}
// One backend per session; boxing the server would only add indirection.
#[allow(clippy::large_enum_variant)]
enum Kind {
    Demo(Snapshot),
    MariaDb(Server),
}
/// Per-schema table counts and sizes need a scan of information_schema.TABLES, which is
/// slow on servers with many tables. It runs at most once per interval, not every refresh.
pub const SIZE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
/// Schema name -> (table count, bytes), with the time it was measured.
type SchemaSizes = (Instant, HashMap<String, (u64, u64)>);
struct Server {
    pool: MySqlPool,
    /// For extra connections of background jobs (backup, restore); the password lives
    /// in memory exactly as long as the pool, which needs it to reconnect anyway.
    options: MySqlConnectOptions,
    sizes: Option<SchemaSizes>,
    size_interval: Duration,
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
                        query_id: 90_211,
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
                        query_id: 0,
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
                datadir: "/var/lib/mysql/".into(),
                log_error: String::new(),
                demo: true,
                source: 0,
            }),
        }
    }
    pub async fn connect(profile: &Profile, password: Secret) -> Result<Self> {
        let options = connect_options(profile, &password)?;
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
            .connect_with(options.clone())
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
            inner: Kind::MariaDb(Server {
                pool,
                options,
                sizes: None,
                size_interval: SIZE_REFRESH_INTERVAL,
            }),
        })
    }
    /// Connection settings for a background job; None in demo mode.
    pub(crate) fn job_options(&self) -> Option<MySqlConnectOptions> {
        match &self.inner {
            Kind::MariaDb(server) => Some(server.options.clone()),
            Kind::Demo(_) => None,
        }
    }
    /// How often per-database sizes and table counts are re-measured (default 60 s).
    pub fn set_size_refresh_interval(&mut self, interval: Duration) {
        if let Kind::MariaDb(server) = &mut self.inner {
            server.size_interval = interval;
        }
    }
    pub async fn snapshot(&mut self) -> Result<Snapshot> {
        let server = match &mut self.inner {
            Kind::Demo(snapshot) => {
                snapshot.questions += 125;
                snapshot.uptime += 5;
                return Ok(snapshot.clone());
            }
            Kind::MariaDb(server) => server,
        };
        let pool = &server.pool;
        // One statement, so global and own-session counters are read at the same instant.
        // Questions/s then excludes Harbor's own statements (global minus this session).
        // sqlx sets the *session* time_zone to +00:00, so @@time_zone would always say UTC.
        let row = sqlx::query(
            "SELECT VERSION() AS version, \
             IF(@@global.time_zone = 'SYSTEM', CONCAT('SYSTEM (', @@system_time_zone, ')'), @@global.time_zone) AS timezone, \
             CAST(@@max_connections AS UNSIGNED) AS maximum, \
             CAST(CONNECTION_ID() AS UNSIGNED) AS source, \
             CAST((SELECT VARIABLE_VALUE FROM information_schema.GLOBAL_STATUS WHERE VARIABLE_NAME = 'UPTIME') AS UNSIGNED) AS uptime, \
             CAST((SELECT VARIABLE_VALUE FROM information_schema.GLOBAL_STATUS WHERE VARIABLE_NAME = 'QUESTIONS') AS UNSIGNED) AS questions, \
             CAST((SELECT VARIABLE_VALUE FROM information_schema.SESSION_STATUS WHERE VARIABLE_NAME = 'QUESTIONS') AS UNSIGNED) AS own_questions, \
             CAST((SELECT VARIABLE_VALUE FROM information_schema.GLOBAL_STATUS WHERE VARIABLE_NAME = 'THREADS_CONNECTED') AS UNSIGNED) AS threads, \
             CONVERT(@@datadir USING utf8mb4) AS datadir, CONVERT(@@log_error USING utf8mb4) AS log_error",
        )
        .fetch_one(pool)
        .await?;
        let counter = |name: &str| -> std::result::Result<u64, sqlx::Error> {
            Ok(row.try_get::<Option<u64>, _>(name)?.unwrap_or(0))
        };
        let mut snapshot = Snapshot {
            version: row.try_get("version")?,
            timezone: row.try_get("timezone")?,
            max_connections: row.try_get("maximum")?,
            source: counter("source")?,
            uptime: counter("uptime")?,
            questions: counter("questions")?.saturating_sub(counter("own_questions")?),
            connections: counter("threads")?,
            datadir: row
                .try_get::<Option<String>, _>("datadir")?
                .unwrap_or_default(),
            log_error: row
                .try_get::<Option<String>, _>("log_error")?
                .unwrap_or_default(),
            ..Default::default()
        };
        let due = server
            .sizes
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed() >= server.size_interval);
        if due {
            let sizes = sqlx::query("SELECT TABLE_SCHEMA AS name, CAST(COUNT(*) AS UNSIGNED) AS tables, CAST(COALESCE(SUM(DATA_LENGTH + INDEX_LENGTH), 0) AS UNSIGNED) AS bytes FROM information_schema.TABLES GROUP BY TABLE_SCHEMA")
                .fetch_all(pool)
                .await?
                .iter()
                .map(|r| Ok((r.try_get("name")?, (r.try_get("tables")?, r.try_get("bytes")?))))
                .collect::<std::result::Result<HashMap<String, (u64, u64)>, sqlx::Error>>()?;
            server.sizes = Some((Instant::now(), sizes));
        }
        let (measured, sizes) = server.sizes.as_ref().expect("measured above");
        snapshot.databases = sqlx::query("SELECT SCHEMA_NAME AS name, DEFAULT_CHARACTER_SET_NAME AS charset, DEFAULT_COLLATION_NAME AS collation FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| {
                let name: String = r.try_get("name")?;
                let (tables, bytes) = sizes.get(&name).copied().unwrap_or((0, 0));
                Ok(Database {
                    name,
                    charset: r.try_get("charset")?,
                    collation: r.try_get("collation")?,
                    tables,
                    bytes,
                })
            })
            .collect::<std::result::Result<_, sqlx::Error>>()?;
        snapshot.notices.push(format!(
            "Počty tabulek a velikosti změřeny před {} s; měří se nejvýše jednou za {} s.",
            measured.elapsed().as_secs(),
            server.size_interval.as_secs()
        ));
        match sqlx::query("SELECT CONVERT(User USING utf8mb4) COLLATE utf8mb4_unicode_ci AS User, CONVERT(Host USING utf8mb4) COLLATE utf8mb4_unicode_ci AS Host, CONVERT(plugin USING utf8mb4) COLLATE utf8mb4_unicode_ci AS plugin FROM mysql.user ORDER BY User,Host")
            .fetch_all(pool)
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
                    .fetch_one(pool)
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
    /// Tables, sizes and object counts of one database.
    pub async fn database_detail(&self, database: &str) -> Result<DatabaseDetail> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_database(database)),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let collation: Option<String> = sqlx::query_scalar(
            "SELECT DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?",
        )
        .bind(database)
        .fetch_optional(pool)
        .await?;
        let collation = collation.ok_or_else(|| {
            Error::Validation("Databáze neexistuje nebo k ní účet nemá přístup.".into())
        })?;
        let keyed: std::collections::HashSet<String> = sqlx::query_scalar(
            "SELECT TABLE_NAME FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA=? AND CONSTRAINT_TYPE='PRIMARY KEY'",
        )
        .bind(database)
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
        let tables = sqlx::query(
            "SELECT TABLE_NAME AS name, TABLE_TYPE AS kind, COALESCE(ENGINE,'') AS engine, \
             CAST(COALESCE(TABLE_ROWS,0) AS UNSIGNED) AS rows_count, \
             CAST(COALESCE(DATA_LENGTH,0) AS UNSIGNED) AS data_bytes, \
             CAST(COALESCE(INDEX_LENGTH,0) AS UNSIGNED) AS index_bytes, \
             CAST(COALESCE(DATA_FREE,0) AS UNSIGNED) AS free_bytes, \
             COALESCE(TABLE_COLLATION,'') AS collation, COALESCE(ROW_FORMAT,'') AS row_format, \
             COALESCE(CAST(CREATE_TIME AS CHAR),'') AS created, COALESCE(CAST(UPDATE_TIME AS CHAR),'') AS updated, \
             CAST(AUTO_INCREMENT AS UNSIGNED) AS auto_inc, COALESCE(TABLE_COMMENT,'') AS comment \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME",
        )
        .bind(database)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| {
            let name: String = r.try_get("name")?;
            Ok(TableInfo {
                primary_key: keyed.contains(&name),
                name,
                kind: r.try_get("kind")?,
                engine: r.try_get("engine")?,
                rows: r.try_get("rows_count")?,
                data_bytes: r.try_get("data_bytes")?,
                index_bytes: r.try_get("index_bytes")?,
                free_bytes: r.try_get("free_bytes")?,
                collation: r.try_get("collation")?,
                row_format: r.try_get("row_format")?,
                created: r.try_get("created")?,
                updated: r.try_get("updated")?,
                auto_increment: r.try_get("auto_inc")?,
                comment: r.try_get("comment")?,
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()?;
        let count = |sql: &'static str| async move {
            sqlx::query_scalar::<_, u64>(sql)
                .bind(database)
                .fetch_one(pool)
                .await
        };
        Ok(DatabaseDetail {
            name: database.into(),
            collation,
            views: tables.iter().filter(|t| t.view()).count() as u64,
            tables,
            routines: count("SELECT CAST(COUNT(*) AS UNSIGNED) FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA=?").await?,
            triggers: count("SELECT CAST(COUNT(*) AS UNSIGNED) FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=?").await?,
            events: count("SELECT CAST(COUNT(*) AS UNSIGNED) FROM information_schema.EVENTS WHERE EVENT_SCHEMA=?").await?,
        })
    }
    /// Columns, indexes, foreign keys and the CREATE statement of one table or view.
    pub async fn table_detail(&self, database: &str, table: &str) -> Result<TableDetail> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_table(database, table)),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let columns = sqlx::query(
            "SELECT COLUMN_NAME AS name, COLUMN_TYPE AS column_type, IS_NULLABLE AS nullable, \
             COLUMN_DEFAULT AS default_value, COLUMN_KEY AS column_key, EXTRA AS extra, \
             COALESCE(COLLATION_NAME,'') AS collation, COLUMN_COMMENT AS comment \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool)
        .await?
        .iter()
        .map(|r| {
            Ok(Column {
                name: r.try_get("name")?,
                column_type: r.try_get("column_type")?,
                nullable: r.try_get::<String, _>("nullable")? == "YES",
                default: r.try_get("default_value")?,
                key: r.try_get("column_key")?,
                extra: r.try_get("extra")?,
                collation: r.try_get("collation")?,
                comment: r.try_get("comment")?,
            })
        })
        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()?;
        if columns.is_empty() {
            return Err(Error::Validation(
                "Tabulka neexistuje nebo k ní účet nemá přístup.".into(),
            ));
        }
        let mut indexes: Vec<Index> = Vec::new();
        for r in sqlx::query(
            "SELECT INDEX_NAME AS name, CAST(NON_UNIQUE AS UNSIGNED) AS non_unique, COLUMN_NAME AS column_name, \
             INDEX_TYPE AS index_type, CAST(COALESCE(CARDINALITY,0) AS UNSIGNED) AS cardinality \
             FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY INDEX_NAME='PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX",
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool)
        .await?
        {
            let name: String = r.try_get("name")?;
            let column: String = r.try_get("column_name")?;
            match indexes.last_mut() {
                Some(index) if index.name == name => index.columns.push(column),
                _ => indexes.push(Index {
                    name,
                    unique: r.try_get::<u64, _>("non_unique")? == 0,
                    columns: vec![column],
                    index_type: r.try_get("index_type")?,
                    cardinality: r.try_get("cardinality")?,
                }),
            }
        }
        let mut foreign_keys: Vec<ForeignKey> = Vec::new();
        for r in sqlx::query(
            "SELECT k.CONSTRAINT_NAME AS name, k.COLUMN_NAME AS column_name, \
             CONCAT(k.REFERENCED_TABLE_SCHEMA, '.', k.REFERENCED_TABLE_NAME) AS referenced, \
             k.REFERENCED_COLUMN_NAME AS referenced_column, r.UPDATE_RULE AS on_update, r.DELETE_RULE AS on_delete \
             FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r \
             ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME \
             WHERE k.TABLE_SCHEMA=? AND k.TABLE_NAME=? AND k.REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool)
        .await?
        {
            let name: String = r.try_get("name")?;
            let column: String = r.try_get("column_name")?;
            let referenced_column: String = r.try_get("referenced_column")?;
            match foreign_keys.last_mut() {
                Some(fk) if fk.name == name => {
                    fk.columns.push(column);
                    fk.referenced_columns.push(referenced_column);
                }
                _ => foreign_keys.push(ForeignKey {
                    name,
                    columns: vec![column],
                    referenced_table: r.try_get("referenced")?,
                    referenced_columns: vec![referenced_column],
                    on_update: r.try_get("on_update")?,
                    on_delete: r.try_get("on_delete")?,
                }),
            }
        }
        // Text protocol: the statement text varies by table and must not fill the
        // per-connection prepared statement cache.
        let show = format!(
            "SHOW CREATE TABLE {}.{}",
            identifier(database)?,
            identifier(table)?
        );
        let create_sql = match sqlx::raw_sql(&show).fetch_one(pool).await {
            Ok(row) => text_column(&row, 1)?,
            Err(error) => format!("Definici nelze načíst: {}", Error::from(error)),
        };
        Ok(TableDetail {
            database: database.into(),
            table: table.into(),
            columns,
            indexes,
            foreign_keys,
            create_sql,
        })
    }
    /// Grants (raw and structured), roles and login properties of an account or role.
    pub async fn account_detail(&self, account: &Account) -> Result<AccountDetail> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_account(account)),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let current: String = sqlx::query_scalar("SELECT CURRENT_USER()")
            .fetch_one(pool)
            .await?;
        let query = if !account.is_role() && current == account.label() {
            "SHOW GRANTS".into()
        } else {
            format!("SHOW GRANTS FOR {}", sql::account_sql(account)?)
        };
        // MariaDB can include authentication hashes in SHOW GRANTS. Never return them to the UI.
        let grants = sqlx::raw_sql(&query)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| text_column(r, 0).map(sql::redact_grant))
            .collect::<Result<Vec<_>>>()?;
        // information_schema builds GRANTEE by plain concatenation.
        let grantee = format!("'{}'@'{}'", account.user, account.host);
        let mut sets: Vec<PrivilegeSet> = Vec::new();
        let mut add = |scope: GrantScope, privilege: String, grantable: bool| {
            let index = match sets.iter().position(|s| s.scope == scope) {
                Some(index) => index,
                None => {
                    sets.push(PrivilegeSet {
                        scope,
                        privileges: Default::default(),
                        grantable: false,
                    });
                    sets.len() - 1
                }
            };
            let set = &mut sets[index];
            set.grantable |= grantable;
            if privilege != "USAGE" {
                set.privileges.insert(privilege);
            }
        };
        for r in sqlx::query("SELECT PRIVILEGE_TYPE AS privilege, IS_GRANTABLE AS grantable FROM information_schema.USER_PRIVILEGES WHERE GRANTEE=?")
            .bind(&grantee)
            .fetch_all(pool)
            .await?
        {
            add(
                GrantScope::Global,
                r.try_get("privilege")?,
                r.try_get::<String, _>("grantable")? == "YES",
            );
        }
        for r in sqlx::query("SELECT TABLE_SCHEMA AS db, PRIVILEGE_TYPE AS privilege, IS_GRANTABLE AS grantable FROM information_schema.SCHEMA_PRIVILEGES WHERE GRANTEE=?")
            .bind(&grantee)
            .fetch_all(pool)
            .await?
        {
            add(
                database_grant_scope(&r.try_get::<String, _>("db")?),
                r.try_get("privilege")?,
                r.try_get::<String, _>("grantable")? == "YES",
            );
        }
        for r in sqlx::query("SELECT TABLE_SCHEMA AS db, TABLE_NAME AS name, PRIVILEGE_TYPE AS privilege, IS_GRANTABLE AS grantable FROM information_schema.TABLE_PRIVILEGES WHERE GRANTEE=?")
            .bind(&grantee)
            .fetch_all(pool)
            .await?
        {
            add(
                GrantScope::Table(r.try_get("db")?, r.try_get("name")?),
                r.try_get("privilege")?,
                r.try_get::<String, _>("grantable")? == "YES",
            );
        }
        // Global USAGE alone means "no global privileges": not worth a row.
        sets.retain(|s| {
            !(s.scope == GrantScope::Global && s.privileges.is_empty() && !s.grantable)
        });
        sets.sort_by(|a, b| a.scope.cmp(&b.scope));
        let columns = sqlx::query("SELECT CONCAT(PRIVILEGE_TYPE, ' (', COLUMN_NAME, ') na ', TABLE_SCHEMA, '.', TABLE_NAME) AS line FROM information_schema.COLUMN_PRIVILEGES WHERE GRANTEE=? ORDER BY TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME")
            .bind(&grantee)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| r.try_get("line"))
            .collect::<std::result::Result<Vec<String>, _>>()?;
        // mysql.* is readable only for administrators; without it the detail still works.
        let roles = sqlx::query_scalar::<_, String>(
            "SELECT CONVERT(Role USING utf8mb4) FROM mysql.roles_mapping WHERE User=? AND Host=? ORDER BY Role",
        )
        .bind(&account.user)
        .bind(&account.host)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let priv_json: Option<String> = sqlx::query_scalar(
            "SELECT CONVERT(Priv USING utf8mb4) FROM mysql.global_priv WHERE User=? AND Host=?",
        )
        .bind(&account.user)
        .bind(&account.host)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        let (properties, default_role) = match priv_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        {
            Some(json) => {
                let number = |key: &str| json.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
                let role = json
                    .get("default_role")
                    .and_then(|v| v.as_str())
                    .filter(|r| !r.is_empty())
                    .map(String::from);
                let properties = (!account.is_role()).then(|| AccountProperties {
                    plugin: json
                        .get("plugin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    locked: json
                        .get("account_locked")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    // PASSWORD EXPIRE stores 0; a missing key (old accounts) means "never".
                    password_expired: json.get("password_last_changed").and_then(|v| v.as_u64())
                        == Some(0),
                    tls: match number("ssl_type") {
                        1 => TlsRequirement::Ssl,
                        2 => TlsRequirement::X509,
                        3 => TlsRequirement::Specified,
                        _ => TlsRequirement::None,
                    },
                    limits: Limits {
                        queries_per_hour: number("max_questions"),
                        updates_per_hour: number("max_updates"),
                        connections_per_hour: number("max_connections"),
                        user_connections: number("max_user_connections"),
                    },
                });
                (properties, role)
            }
            None => (None, None),
        };
        Ok(AccountDetail {
            account: Some(account.clone()),
            grants,
            privileges: sets,
            columns,
            roles,
            default_role,
            properties,
        })
    }
    /// Kept for callers of the 0.1 API: SHOW GRANTS lines only.
    pub async fn grants(&self, account: &Account) -> Result<Vec<String>> {
        Ok(self.account_detail(account).await?.grants)
    }
    /// Kept for callers of the 0.1 API: the table list of a database.
    pub async fn tables(&self, database: &str) -> Result<Vec<TableInfo>> {
        Ok(self.database_detail(database).await?.tables)
    }
    /// All server variables with origin, default and allowed range.
    pub async fn variables(&self) -> Result<Vec<Variable>> {
        let pool = match &self.inner {
            Kind::Demo(snapshot) => return Ok(demo_variables(snapshot)),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        Ok(
            sqlx::query(&format!("{VARIABLES_SELECT} ORDER BY VARIABLE_NAME"))
                .fetch_all(pool)
                .await?
                .iter()
                .map(variable_from_row)
                .collect::<std::result::Result<_, sqlx::Error>>()?,
        )
    }
    /// Facts for the rule-based health check. Each part that the account cannot read
    /// stays None and is reported as skipped.
    pub async fn health(&self) -> Result<Vec<Finding>> {
        let pool = match &self.inner {
            Kind::Demo(snapshot) => return Ok(evaluate_health(&demo_health(snapshot))),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let names = |sql: &'static str| async move {
            sqlx::query_scalar::<_, String>(sql)
                .fetch_all(pool)
                .await
                .ok()
        };
        let mut facts = HealthFacts {
            anonymous_accounts: names("SELECT CONVERT(CONCAT('''''@''', Host, '''') USING utf8mb4) FROM mysql.user WHERE User=''").await,
            accounts_without_password: names("SELECT CONVERT(CONCAT('''', User, '''@''', Host, '''') USING utf8mb4) FROM mysql.global_priv \
             WHERE User<>'' AND COALESCE(JSON_EXTRACT(Priv,'$.is_role'),'false')<>'true' \
             AND COALESCE(JSON_EXTRACT(Priv,'$.account_locked'),'false')<>'true' \
             AND COALESCE(JSON_VALUE(Priv,'$.plugin'),'mysql_native_password') IN ('mysql_native_password','mysql_old_password','') \
             AND COALESCE(JSON_VALUE(Priv,'$.authentication_string'),'')='' \
             AND JSON_LENGTH(COALESCE(JSON_EXTRACT(Priv,'$.auth_or'),'[]'))=0").await,
            remote_root: names("SELECT CONVERT(CONCAT('root@', Host) USING utf8mb4) FROM mysql.user WHERE User='root' AND Host NOT IN ('localhost','127.0.0.1','::1')").await,
            long_transactions: names("SELECT CONCAT('spojení #', trx_mysql_thread_id, ', ', TIMESTAMPDIFF(SECOND, trx_started, NOW()), ' s') FROM information_schema.INNODB_TRX WHERE trx_started < NOW() - INTERVAL 60 SECOND ORDER BY trx_started").await,
            ..Default::default()
        };
        facts.test_database = sqlx::query_scalar::<_, u64>(
            "SELECT CAST(COUNT(*) AS UNSIGNED) FROM information_schema.SCHEMATA WHERE SCHEMA_NAME='test'",
        )
        .fetch_one(pool)
        .await
        .ok()
        .map(|n| n > 0);
        let status: HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
            "SELECT VARIABLE_NAME, VARIABLE_VALUE FROM information_schema.GLOBAL_STATUS WHERE VARIABLE_NAME IN ('MAX_USED_CONNECTIONS','CONNECTIONS','ABORTED_CONNECTS','INNODB_BUFFER_POOL_READS','INNODB_BUFFER_POOL_READ_REQUESTS','CREATED_TMP_TABLES','CREATED_TMP_DISK_TABLES','UPTIME')",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
        let number = |map: &HashMap<String, String>, key: &str| {
            map.get(key).and_then(|v| v.trim().parse::<f64>().ok())
        };
        let count = |key: &str| number(&status, key).map(|v| v as u64);
        facts.max_used_connections = count("MAX_USED_CONNECTIONS");
        facts.connections_total = count("CONNECTIONS");
        facts.aborted_connects = count("ABORTED_CONNECTS");
        facts.buffer_pool_reads = count("INNODB_BUFFER_POOL_READS");
        facts.buffer_pool_read_requests = count("INNODB_BUFFER_POOL_READ_REQUESTS");
        facts.tmp_tables = count("CREATED_TMP_TABLES");
        facts.tmp_disk_tables = count("CREATED_TMP_DISK_TABLES");
        facts.uptime = count("UPTIME").unwrap_or(0);
        let variables: HashMap<String, String> = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT VARIABLE_NAME, GLOBAL_VALUE FROM information_schema.SYSTEM_VARIABLES WHERE VARIABLE_NAME IN ('MAX_CONNECTIONS','INNODB_BUFFER_POOL_SIZE','LOG_BIN','EXPIRE_LOGS_DAYS','BINLOG_EXPIRE_LOGS_SECONDS','SLOW_QUERY_LOG','HAVE_SSL','VERSION')",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(k, v)| (k, v.unwrap_or_default()))
        .collect();
        facts.max_connections = number(&variables, "MAX_CONNECTIONS").unwrap_or(0.0) as u64;
        facts.buffer_pool_size = number(&variables, "INNODB_BUFFER_POOL_SIZE").map(|v| v as u64);
        facts.log_bin = variables.get("LOG_BIN").map(|v| v == "ON");
        facts.slow_query_log = variables.get("SLOW_QUERY_LOG").map(|v| v == "ON");
        facts.have_ssl = variables.get("HAVE_SSL").cloned();
        facts.version = variables.get("VERSION").cloned().unwrap_or_default();
        facts.binlog_expire_seconds = number(&variables, "BINLOG_EXPIRE_LOGS_SECONDS")
            .or_else(|| number(&variables, "EXPIRE_LOGS_DAYS").map(|d| d * 86_400.0));
        let keyed: std::collections::HashSet<(String, String)> = sqlx::query_as(
            "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.TABLE_CONSTRAINTS WHERE CONSTRAINT_TYPE='PRIMARY KEY'",
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
        let tables: Vec<(String, String, String, u64, u64)> = sqlx::query_as(
            "SELECT TABLE_SCHEMA, TABLE_NAME, COALESCE(ENGINE,''), CAST(COALESCE(DATA_LENGTH,0)+COALESCE(INDEX_LENGTH,0) AS UNSIGNED), CAST(COALESCE(DATA_FREE,0) AS UNSIGNED) \
             FROM information_schema.TABLES WHERE TABLE_TYPE='BASE TABLE' AND TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') ORDER BY TABLE_SCHEMA, TABLE_NAME",
        )
        .fetch_all(pool)
        .await?;
        let label = |db: &str, t: &str| format!("{db}.{t}");
        facts.tables_without_pk = Some(
            tables
                .iter()
                .filter(|(db, t, ..)| !keyed.contains(&(db.clone(), t.clone())))
                .map(|(db, t, ..)| label(db, t))
                .collect(),
        );
        facts.myisam_tables = Some(
            tables
                .iter()
                .filter(|(_, _, engine, ..)| engine == "MyISAM")
                .map(|(db, t, ..)| label(db, t))
                .collect(),
        );
        facts.fragmented_tables = Some(
            tables
                .iter()
                .filter(|(.., bytes, free)| {
                    TableInfo {
                        data_bytes: *bytes,
                        free_bytes: *free,
                        ..Default::default()
                    }
                    .fragmented()
                })
                .map(|(db, t, ..)| label(db, t))
                .collect(),
        );
        facts.innodb_bytes = Some(
            tables
                .iter()
                .filter(|(_, _, engine, ..)| engine == "InnoDB")
                .map(|(.., bytes, _)| *bytes)
                .sum(),
        );
        if let Ok(replication) = self.replication().await {
            facts.replicas = replication.channels;
            facts.galera = replication.galera;
        }
        Ok(evaluate_health(&facts))
    }
    /// Open InnoDB transactions, lock waits and metadata lock waits.
    pub async fn locks(&self) -> Result<LockReport> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_locks()),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let mut report = LockReport::default();
        match sqlx::query(
            "SELECT CAST(t.trx_mysql_thread_id AS UNSIGNED) AS thread, t.trx_state AS state, \
             CAST(GREATEST(TIMESTAMPDIFF(SECOND, t.trx_started, NOW()), 0) AS UNSIGNED) AS seconds, \
             CAST(t.trx_rows_locked AS UNSIGNED) AS rows_locked, CAST(t.trx_rows_modified AS UNSIGNED) AS rows_modified, \
             CAST(t.trx_tables_locked AS UNSIGNED) AS tables_locked, CAST(COALESCE(t.trx_isolation_level,'') AS CHAR(32)) AS isolation, \
             COALESCE(t.trx_query,'') AS query, COALESCE(p.USER,'') AS user, COALESCE(p.HOST,'') AS host, \
             COALESCE(p.DB,'') AS db, COALESCE(p.COMMAND,'') AS command \
             FROM information_schema.INNODB_TRX t LEFT JOIN information_schema.PROCESSLIST p ON p.ID = t.trx_mysql_thread_id \
             ORDER BY t.trx_started",
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => {
                for r in rows {
                    report.transactions.push(Transaction {
                        thread: r.try_get("thread")?,
                        state: r.try_get("state")?,
                        seconds: r.try_get("seconds")?,
                        rows_locked: r.try_get("rows_locked")?,
                        rows_modified: r.try_get("rows_modified")?,
                        tables_locked: r.try_get("tables_locked")?,
                        isolation: r.try_get("isolation")?,
                        query: r.try_get("query")?,
                        user: r.try_get("user")?,
                        host: r.try_get("host")?,
                        database: r.try_get("db")?,
                        command: r.try_get("command")?,
                    });
                }
            }
            Err(error) => report.notes.push(format!(
                "Transakce nelze číst ({}); potřebné je právo PROCESS.",
                Error::from(error)
            )),
        }
        if let Ok(rows) = sqlx::query(
            "SELECT CAST(r.trx_mysql_thread_id AS UNSIGNED) AS waiting_thread, COALESCE(r.trx_query,'') AS waiting_query, \
             CAST(GREATEST(TIMESTAMPDIFF(SECOND, r.trx_wait_started, NOW()), 0) AS UNSIGNED) AS waiting_seconds, \
             CAST(b.trx_mysql_thread_id AS UNSIGNED) AS blocking_thread, COALESCE(b.trx_query,'') AS blocking_query, \
             COALESCE(l.lock_table,'') AS lock_table, COALESCE(l.lock_mode,'') AS lock_mode \
             FROM information_schema.INNODB_LOCK_WAITS w \
             JOIN information_schema.INNODB_TRX r ON r.trx_id = w.requesting_trx_id \
             JOIN information_schema.INNODB_TRX b ON b.trx_id = w.blocking_trx_id \
             LEFT JOIN information_schema.INNODB_LOCKS l ON l.lock_id = w.requested_lock_id \
             ORDER BY waiting_seconds DESC",
        )
        .fetch_all(pool)
        .await
        {
            for r in rows {
                report.waits.push(LockWait {
                    waiting_thread: r.try_get("waiting_thread")?,
                    waiting_query: r.try_get("waiting_query")?,
                    waiting_seconds: r.try_get("waiting_seconds")?,
                    blocking_thread: r.try_get("blocking_thread")?,
                    blocking_query: r.try_get("blocking_query")?,
                    lock_table: r.try_get("lock_table")?,
                    lock_mode: r.try_get("lock_mode")?,
                });
            }
        }
        report.root_blockers = root_blockers(&report.waits);
        for r in sqlx::query(
            "SELECT CAST(ID AS UNSIGNED) AS thread, CAST(TIME AS UNSIGNED) AS seconds, COALESCE(DB,'') AS db, COALESCE(INFO,'') AS query \
             FROM information_schema.PROCESSLIST WHERE STATE LIKE 'Waiting for table metadata lock%' ORDER BY TIME DESC",
        )
        .fetch_all(pool)
        .await?
        {
            report.metadata_waits.push(MetadataWait {
                thread: r.try_get("thread")?,
                seconds: r.try_get("seconds")?,
                database: r.try_get("db")?,
                query: r.try_get("query")?,
            });
        }
        Ok(report)
    }
    /// Statement statistics from performance_schema or mysql.slow_log; for a slow log
    /// file the source is returned and the file is read through host access.
    pub async fn performance(&self) -> Result<Performance> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_performance()),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let (ps, slow, long, output, file): (i64, i64, f64, String, String) = sqlx::query_as(
            "SELECT CAST(@@GLOBAL.performance_schema AS SIGNED), CAST(@@GLOBAL.slow_query_log AS SIGNED), CAST(@@GLOBAL.long_query_time AS DOUBLE), \
             CONVERT(@@GLOBAL.log_output USING utf8mb4), CONVERT(@@GLOBAL.slow_query_log_file USING utf8mb4)",
        )
        .fetch_one(pool)
        .await?;
        let mut performance = Performance {
            performance_schema: ps == 1,
            slow_query_log: slow == 1,
            long_query_time: long,
            log_output: output.clone(),
            slow_log_file: file.clone(),
            ..Default::default()
        };
        if performance.performance_schema {
            performance.source = Some(StatsSource::PerformanceSchema);
            performance.statements = sqlx::query(
                "SELECT COALESCE(DIGEST_TEXT,'') AS query, COALESCE(SCHEMA_NAME,'') AS schema_name, \
                 CAST(COUNT_STAR AS UNSIGNED) AS count, CAST(SUM_TIMER_WAIT / 1e12 AS DOUBLE) AS total, \
                 CAST(MAX_TIMER_WAIT / 1e12 AS DOUBLE) AS max_seconds, CAST(SUM_ROWS_EXAMINED AS UNSIGNED) AS examined, \
                 CAST(SUM_ROWS_SENT AS UNSIGNED) AS sent, CAST(SUM_NO_INDEX_USED AS UNSIGNED) AS no_index, \
                 COALESCE(CAST(LAST_SEEN AS CHAR),'') AS last_seen \
                 FROM performance_schema.events_statements_summary_by_digest ORDER BY SUM_TIMER_WAIT DESC LIMIT 200",
            )
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| {
                Ok(StatementStat {
                    query: r.try_get("query")?,
                    schema: r.try_get("schema_name")?,
                    count: r.try_get("count")?,
                    total_seconds: r.try_get("total")?,
                    max_seconds: r.try_get("max_seconds")?,
                    rows_examined: r.try_get("examined")?,
                    rows_sent: r.try_get("sent")?,
                    no_index: r.try_get("no_index")?,
                    last_seen: r.try_get("last_seen")?,
                    sample: None,
                })
            })
            .collect::<std::result::Result<_, sqlx::Error>>()?;
        } else if performance.slow_query_log && output.to_ascii_uppercase().contains("TABLE") {
            performance.source = Some(StatsSource::SlowLogTable);
            let rows: Vec<(String, f64, u64, u64, String, String)> = sqlx::query_as(
                "SELECT CAST(start_time AS CHAR), CAST(TIME_TO_SEC(query_time) + MICROSECOND(query_time) / 1e6 AS DOUBLE), \
                 CAST(rows_sent AS UNSIGNED), CAST(rows_examined AS UNSIGNED), CONVERT(db USING utf8mb4), CONVERT(sql_text USING utf8mb4) \
                 FROM mysql.slow_log ORDER BY start_time DESC LIMIT 20000",
            )
            .fetch_all(pool)
            .await?;
            performance.statements = aggregate_slow_rows(&rows);
        } else if performance.slow_query_log {
            performance.source = Some(StatsSource::SlowLogFile(file));
        } else {
            performance.source = Some(StatsSource::Off);
        }
        Ok(performance)
    }
    /// Replication channels, primary state, binary logs and Galera.
    pub async fn replication(&self) -> Result<Replication> {
        let pool = match &self.inner {
            Kind::Demo(_) => return Ok(demo_replication()),
            Kind::MariaDb(Server { pool, .. }) => pool,
        };
        let (server_id, read_only, log_bin, format, binlog_pos, slave_pos): (u64, i64, i64, String, String, String) =
            sqlx::query_as(
                "SELECT CAST(@@server_id AS UNSIGNED), CAST(@@read_only AS SIGNED), CAST(@@log_bin AS SIGNED), \
                 CONVERT(@@GLOBAL.binlog_format USING utf8mb4), CONVERT(@@gtid_binlog_pos USING utf8mb4), CONVERT(@@gtid_slave_pos USING utf8mb4)",
            )
            .fetch_one(pool)
            .await?;
        let mut replication = Replication {
            server_id,
            read_only: read_only == 1,
            log_bin: log_bin == 1,
            binlog_format: format,
            gtid_binlog_pos: binlog_pos,
            gtid_slave_pos: slave_pos,
            ..Default::default()
        };
        let expire: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT VARIABLE_NAME, GLOBAL_VALUE FROM information_schema.SYSTEM_VARIABLES WHERE VARIABLE_NAME IN ('BINLOG_EXPIRE_LOGS_SECONDS','EXPIRE_LOGS_DAYS')",
        )
        .fetch_all(pool)
        .await?;
        let value = |name: &str| {
            expire
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, v)| v.as_deref())
                .and_then(|v| v.parse::<f64>().ok())
        };
        replication.expire_seconds = value("BINLOG_EXPIRE_LOGS_SECONDS")
            .or_else(|| value("EXPIRE_LOGS_DAYS").map(|d| d * 86_400.0))
            .unwrap_or(0.0);
        match sqlx::raw_sql("SHOW ALL SLAVES STATUS").fetch_all(pool).await {
            Ok(rows) => {
                for row in rows {
                    let col = |name: &str| named(&row, name);
                    replication.channels.push(ReplicaChannel {
                        name: col("Connection_name"),
                        master: format!("{}:{}", col("Master_Host"), col("Master_Port")),
                        io_running: col("Slave_IO_Running"),
                        sql_running: col("Slave_SQL_Running"),
                        seconds_behind: col("Seconds_Behind_Master").parse().ok(),
                        using_gtid: col("Using_Gtid"),
                        gtid_io_pos: col("Gtid_IO_Pos"),
                        last_io_error: col("Last_IO_Error"),
                        last_sql_error: col("Last_SQL_Error"),
                        relay_log_space: col("Relay_Log_Space").parse().unwrap_or(0),
                        state: col("Slave_SQL_Running_State"),
                    });
                }
            }
            Err(error) => replication.notes.push(format!(
                "Stav replik nelze číst ({}); potřebné je právo REPLICATION CLIENT / SLAVE MONITOR.",
                Error::from(error)
            )),
        }
        if replication.log_bin {
            match sqlx::raw_sql("SHOW BINARY LOGS").fetch_all(pool).await {
                Ok(rows) => {
                    for row in rows {
                        replication.binlogs.push(BinaryLog {
                            name: named(&row, "Log_name"),
                            bytes: named(&row, "File_size").parse().unwrap_or(0),
                        });
                    }
                }
                Err(error) => replication.notes.push(format!(
                    "Binární logy nelze vypsat ({}).",
                    Error::from(error)
                )),
            }
            if let Ok(rows) = sqlx::raw_sql("SHOW SLAVE HOSTS").fetch_all(pool).await {
                for row in rows {
                    replication.replicas.push(format!(
                        "server_id {} • {}:{}",
                        named(&row, "Server_id"),
                        named(&row, "Host"),
                        named(&row, "Port")
                    ));
                }
            }
        }
        let wsrep: HashMap<String, String> = sqlx::query_as::<_, (String, String)>(
            "SELECT VARIABLE_NAME, VARIABLE_VALUE FROM information_schema.GLOBAL_STATUS WHERE VARIABLE_NAME IN \
             ('WSREP_ON','WSREP_CLUSTER_SIZE','WSREP_CLUSTER_STATUS','WSREP_LOCAL_STATE_COMMENT','WSREP_READY','WSREP_CONNECTED','WSREP_FLOW_CONTROL_PAUSED','WSREP_LOCAL_RECV_QUEUE_AVG')",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect();
        let galera_on: Option<String> = sqlx::query_scalar(
            "SELECT GLOBAL_VALUE FROM information_schema.SYSTEM_VARIABLES WHERE VARIABLE_NAME='WSREP_ON'",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
        if galera_on.as_deref() == Some("ON") {
            let names: Vec<(String, Option<String>)> = sqlx::query_as(
                "SELECT VARIABLE_NAME, GLOBAL_VALUE FROM information_schema.SYSTEM_VARIABLES WHERE VARIABLE_NAME IN ('WSREP_CLUSTER_NAME','WSREP_NODE_NAME')",
            )
            .fetch_all(pool)
            .await
            .unwrap_or_default();
            let var = |n: &str| {
                names
                    .iter()
                    .find(|(k, _)| k == n)
                    .and_then(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            replication.galera = Some(galera_from_status(
                &wsrep,
                var("WSREP_CLUSTER_NAME"),
                var("WSREP_NODE_NAME"),
            ));
        }
        Ok(replication)
    }
    /// Runs a confirmed change. Returns the server's messages (maintenance output).
    pub async fn apply(&mut self, change: Change) -> Result<Vec<String>> {
        if let Kind::Demo(snapshot) = &mut self.inner {
            return demo_apply(snapshot, change);
        }
        let Kind::MariaDb(server) = &mut self.inner else {
            unreachable!()
        };
        let pool = &server.pool;
        // Checks that need the server; the SQL itself comes from sql::statement.
        match &change {
            Change::CreateDatabase { collation, .. } => {
                let known: Option<String> = sqlx::query_scalar("SELECT COLLATION_NAME FROM information_schema.COLLATIONS WHERE COLLATION_NAME=?").bind(collation).fetch_optional(pool).await?;
                if known.is_none() {
                    return Err(Error::Validation("Server tuto kolaci nepodporuje.".into()));
                }
            }
            Change::DropAccount { account } | Change::AlterAccount { account, .. }
                if !account.is_role() =>
            {
                let current: String = sqlx::query_scalar("SELECT CURRENT_USER()")
                    .fetch_one(pool)
                    .await?;
                if current == account.label() && matches!(change, Change::DropAccount { .. }) {
                    return Err(Error::Validation(
                        "Účet, kterým je Harbor připojen, nelze smazat.".into(),
                    ));
                }
                if current == account.label()
                    && matches!(change, Change::AlterAccount { locked: true, .. })
                {
                    return Err(Error::Validation(
                        "Účet, kterým je Harbor připojen, nelze zamknout.".into(),
                    ));
                }
            }
            Change::ChangePassword { account, password } => {
                sql::validate_password(password)?;
                sql::validate_account(&account.user, &account.host)?;
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
            }
            Change::SetGlobal { variable, .. } => {
                let current = sqlx::query(&format!(
                    "{VARIABLES_SELECT} WHERE VARIABLE_NAME = UPPER(?)"
                ))
                .bind(&variable.name)
                .fetch_optional(pool)
                .await?
                .map(|r| variable_from_row(&r))
                .transpose()?
                .ok_or_else(|| Error::Validation("Server tuto proměnnou nezná.".into()))?;
                if current.var_type != variable.var_type
                    || current.read_only != variable.read_only
                    || current.scope != variable.scope
                {
                    return Err(Error::Validation(
                        "Vlastnosti proměnné se změnily; načtěte seznam znovu.".into(),
                    ));
                }
            }
            Change::KillConnection { target } => {
                let current = processes(pool)
                    .await?
                    .into_iter()
                    .find(|p| p.id == target.id)
                    .ok_or_else(|| Error::Validation("Spojení už neexistuje.".into()))?;
                if current.user != target.user || current.host != target.host {
                    return Err(Error::Validation(
                        "Pod tímto číslem je teď jiné spojení; nic nebylo ukončeno.".into(),
                    ));
                }
            }
            Change::PurgeBinlogs { to } => {
                let logs: Vec<String> = sqlx::raw_sql("SHOW BINARY LOGS")
                    .fetch_all(pool)
                    .await?
                    .iter()
                    .map(|r| named(r, "Log_name"))
                    .collect();
                if !logs.contains(to) {
                    return Err(Error::Validation(
                        "Takový binární log na serveru není.".into(),
                    ));
                }
                if logs.first() == Some(to) {
                    return Err(Error::Validation(
                        "To je nejstarší log; není co mazat.".into(),
                    ));
                }
            }
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
                    || current.query_id != target.query_id
                    || current.query_id == 0
                    || !current.running_query()
                    || current.seconds < target.seconds
                {
                    return Err(Error::Validation(
                        "Cíl se změnil; znovu otevřete detail.".into(),
                    ));
                }
            }
            _ => (),
        }
        let statement = sql::statement(&change, true)?;
        let kill = matches!(change, Change::KillQuery { .. });
        let oldest_before = if matches!(change, Change::PurgeBinlogs { .. }) {
            sqlx::raw_sql("SHOW BINARY LOGS")
                .fetch_all(pool)
                .await?
                .first()
                .map(|r| named(r, "Log_name"))
        } else {
            None
        };
        // Text protocol: DDL must not be prepared and kept in sqlx's per-connection
        // statement cache (keyed by the SQL text) or as an open statement on the server.
        let result = if matches!(change, Change::Maintenance { .. }) {
            sqlx::raw_sql(&statement)
                .fetch_all(pool)
                .await
                .and_then(|rows| {
                    rows.iter()
                        .map(|r| {
                            Ok(format!(
                                "{} • {}: {}",
                                r.try_get::<String, _>(0)?,
                                r.try_get::<String, _>(2)?,
                                r.try_get::<String, _>(3)?
                            ))
                        })
                        .collect::<std::result::Result<Vec<_>, sqlx::Error>>()
                })
        } else {
            sqlx::raw_sql(&statement)
                .execute(pool)
                .await
                .map(|_| Vec::new())
        };
        match result {
            Ok(mut output) => {
                // Our own write can change counts and sizes: measure on the next snapshot.
                server.sizes = None;
                if let Change::PurgeBinlogs { to } = &change {
                    // The server silently keeps logs a connected replica still reads.
                    let first = sqlx::raw_sql("SHOW BINARY LOGS")
                        .fetch_all(pool)
                        .await?
                        .first()
                        .map(|r| named(r, "Log_name"))
                        .unwrap_or_default();
                    output.push(if &first == to {
                        format!("Smazány binární logy starší než {to}.")
                    } else if Some(&first) == oldest_before.as_ref() {
                        format!(
                            "Nic nebylo smazáno: log {first} ještě čte připojená replika."
                        )
                    } else {
                        format!(
                            "Server smazal jen logy starší než {first}: novější ještě čte připojená replika."
                        )
                    });
                }
                Ok(output)
            }
            // ER_NO_SUCH_QUERY: the statement finished between the check and KILL.
            Err(sqlx::Error::Database(e))
                if kill
                    && e.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                        .map(|e| e.number())
                        == Some(1957) =>
            {
                Err(Error::Validation(
                    "Dotaz mezitím skončil; nic nebylo ukončeno.".into(),
                ))
            }
            Err(error) => Err(error.into()),
        }
    }
}
async fn processes(pool: &MySqlPool) -> Result<Vec<Process>> {
    Ok(sqlx::query("SELECT CAST(ID AS UNSIGNED) AS ID,CAST(QUERY_ID AS UNSIGNED) AS query_id,USER,HOST,COALESCE(DB,'') AS db_name,COMMAND,CAST(TIME AS UNSIGNED) AS elapsed,COALESCE(STATE,'') AS state_name,COALESCE(INFO,'') AS query_text FROM information_schema.PROCESSLIST WHERE ID <> CONNECTION_ID() ORDER BY TIME DESC").fetch_all(pool).await?.iter().map(|r| Ok(Process { id: r.try_get("ID")?, query_id: r.try_get("query_id")?, user: r.try_get("USER")?, host: r.try_get("HOST")?, database: r.try_get("db_name")?, command: r.try_get("COMMAND")?, seconds: r.try_get("elapsed")?, state: r.try_get("state_name")?, query: r.try_get("query_text")? })).collect::<std::result::Result<_, sqlx::Error>>()?)
}
/// Connection settings of a profile (shared by the monitoring pool and job connections).
pub(crate) fn connect_options(profile: &Profile, password: &Secret) -> Result<MySqlConnectOptions> {
    profile.validate()?;
    let mut options = MySqlConnectOptions::new()
        .username(&profile.user)
        .disable_statement_logging();
    if !password.0.is_empty() {
        options = options.password(&password.0);
    }
    options = match &profile.transport {
        Transport::Tcp { host, port } => {
            options
                .host(host)
                .port(*port)
                .ssl_mode(if profile.tls == TlsMode::VerifyIdentity {
                    MySqlSslMode::VerifyIdentity
                } else {
                    MySqlSslMode::Disabled
                })
        }
        Transport::Unix { path } => options.socket(path).ssl_mode(MySqlSslMode::Disabled),
    };
    if let Some(ca) = &profile.ca {
        if matches!(profile.transport, Transport::Tcp { .. })
            && profile.tls == TlsMode::VerifyIdentity
        {
            check_ca_file(ca)?;
        }
        options = options.ssl_ca(ca);
    }
    Ok(options)
}
fn check_ca_file(path: &str) -> Result<()> {
    let data = std::fs::read(path)
        .map_err(|_| Error::Config(format!("soubor CA „{}“ nelze přečíst.", safe_text(path))))?;
    if !data
        .windows(27)
        .any(|w| w == b"-----BEGIN CERTIFICATE-----")
    {
        return Err(Error::Config(
            "soubor CA neobsahuje certifikát ve formátu PEM.".into(),
        ));
    }
    Ok(())
}
/// Text of a result column; SHOW statements may return it as binary.
fn text_column(row: &sqlx::mysql::MySqlRow, index: usize) -> Result<String> {
    if let Ok(text) = row.try_get::<String, _>(index) {
        return Ok(text);
    }
    let bytes: Vec<u8> = row.try_get(index)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
const VARIABLES_SELECT: &str = "SELECT LOWER(VARIABLE_NAME) AS name, COALESCE(GLOBAL_VALUE, SESSION_VALUE, '') AS value, \
     DEFAULT_VALUE AS default_value, COALESCE(GLOBAL_VALUE_ORIGIN,'') AS origin, VARIABLE_SCOPE AS scope, \
     VARIABLE_TYPE AS var_type, NUMERIC_MIN_VALUE AS min_value, NUMERIC_MAX_VALUE AS max_value, \
     ENUM_VALUE_LIST AS allowed, READ_ONLY AS read_only, COALESCE(VARIABLE_COMMENT,'') AS comment \
     FROM information_schema.SYSTEM_VARIABLES";
fn variable_from_row(r: &sqlx::mysql::MySqlRow) -> std::result::Result<Variable, sqlx::Error> {
    Ok(Variable {
        name: r.try_get("name")?,
        value: r.try_get("value")?,
        default: r.try_get("default_value")?,
        origin: r.try_get("origin")?,
        scope: r.try_get("scope")?,
        var_type: r.try_get("var_type")?,
        min: r.try_get("min_value")?,
        max: r.try_get("max_value")?,
        allowed: r
            .try_get::<Option<String>, _>("allowed")?
            .map(|list| list.split(',').map(String::from).collect())
            .unwrap_or_default(),
        read_only: r.try_get::<String, _>("read_only")? == "YES",
        comment: r.try_get("comment")?,
    })
}

/// A column of a SHOW result by name, as text (types differ between versions).
fn named(row: &sqlx::mysql::MySqlRow, name: &str) -> String {
    use sqlx::Column;
    let Some(index) = row.columns().iter().position(|c| c.name() == name) else {
        return String::new();
    };
    if let Ok(Some(text)) = row.try_get_unchecked::<Option<String>, _>(index) {
        return text;
    }
    row.try_get_unchecked::<Option<Vec<u8>>, _>(index)
        .ok()
        .flatten()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}
pub(crate) fn galera_from_status(
    status: &HashMap<String, String>,
    cluster: String,
    node: String,
) -> Galera {
    let get = |k: &str| status.get(k).cloned().unwrap_or_default();
    Galera {
        cluster_name: cluster,
        node_name: node,
        cluster_size: get("WSREP_CLUSTER_SIZE").parse().unwrap_or(0),
        cluster_status: get("WSREP_CLUSTER_STATUS"),
        local_state: get("WSREP_LOCAL_STATE_COMMENT"),
        ready: get("WSREP_READY") == "ON",
        connected: get("WSREP_CONNECTED") == "ON",
        flow_control_paused: get("WSREP_FLOW_CONTROL_PAUSED").parse().unwrap_or(0.0),
        recv_queue_avg: get("WSREP_LOCAL_RECV_QUEUE_AVG").parse().unwrap_or(0.0),
    }
}
fn demo_locks() -> LockReport {
    let waits = vec![LockWait {
        waiting_thread: 1871,
        waiting_query: "UPDATE stock SET n = n - 1 WHERE id = 5 (demo)".into(),
        waiting_seconds: 38,
        blocking_thread: 1842,
        blocking_query: String::new(),
        lock_table: "`commerce`.`stock`".into(),
        lock_mode: "X".into(),
    }];
    LockReport {
        transactions: vec![
            Transaction {
                thread: 1842,
                state: "RUNNING".into(),
                seconds: 312,
                rows_locked: 1,
                rows_modified: 1,
                tables_locked: 1,
                isolation: "REPEATABLE READ".into(),
                user: "commerce_app".into(),
                host: "demo:5001".into(),
                database: "commerce".into(),
                command: "Sleep".into(),
                ..Default::default()
            },
            Transaction {
                thread: 1871,
                state: "LOCK WAIT".into(),
                seconds: 38,
                rows_locked: 0,
                isolation: "REPEATABLE READ".into(),
                query: "UPDATE stock SET n = n - 1 WHERE id = 5 (demo)".into(),
                user: "commerce_app".into(),
                host: "demo:5002".into(),
                database: "commerce".into(),
                command: "Query".into(),
                ..Default::default()
            },
        ],
        root_blockers: root_blockers(&waits),
        waits,
        metadata_waits: vec![],
        notes: vec![],
    }
}
fn demo_performance() -> Performance {
    let stat = |query: &str, count, total, max, examined| StatementStat {
        query: query.into(),
        schema: "commerce".into(),
        count,
        total_seconds: total,
        max_seconds: max,
        rows_examined: examined,
        rows_sent: count * 10,
        sample: Some(query.replace('?', "42")),
        last_seen: "2026-11-01 09:12:00".into(),
        ..Default::default()
    };
    Performance {
        source: Some(StatsSource::SlowLogTable),
        statements: vec![
            stat(
                "SELECT * FROM orders WHERE customer_id = ? ORDER BY created_at DESC",
                1840,
                2210.5,
                9.8,
                92_000_000,
            ),
            stat(
                "SELECT COUNT(*) FROM audit_log WHERE created_at > ?",
                96,
                410.2,
                12.1,
                8_400_000_000,
            ),
            stat(
                "UPDATE stock SET n = n - ? WHERE id = ?",
                22_000,
                120.0,
                38.0,
                22_000,
            ),
        ],
        performance_schema: false,
        slow_query_log: true,
        long_query_time: 1.0,
        log_output: "TABLE".into(),
        slow_log_file: String::new(),
    }
}
fn demo_replication() -> Replication {
    Replication {
        server_id: 2,
        read_only: true,
        log_bin: true,
        binlog_format: "ROW".into(),
        gtid_binlog_pos: "0-1-48211".into(),
        gtid_slave_pos: "0-1-48211".into(),
        expire_seconds: 7.0 * 86_400.0,
        channels: vec![ReplicaChannel {
            name: String::new(),
            master: "db-primary.demo:3306".into(),
            io_running: "Yes".into(),
            sql_running: "Yes".into(),
            seconds_behind: Some(3),
            using_gtid: "Slave_Pos".into(),
            gtid_io_pos: "0-1-48211".into(),
            state: "Slave has read all relay log; waiting for more updates".into(),
            ..Default::default()
        }],
        replicas: vec![],
        binlogs: vec![
            BinaryLog {
                name: "mariadb-bin.000041".into(),
                bytes: 1_073_741_824,
            },
            BinaryLog {
                name: "mariadb-bin.000042".into(),
                bytes: 524_288_000,
            },
        ],
        galera: None,
        notes: vec!["DEMO: simulovaná replika.".into()],
    }
}
// ---------------------------------------------------------------- demo data

fn demo_database(database: &str) -> DatabaseDetail {
    let table = |name: &str, engine: &str, rows, data: u64, index: u64, free: u64, pk| TableInfo {
        name: name.into(),
        kind: "BASE TABLE".into(),
        engine: engine.into(),
        rows,
        data_bytes: data,
        index_bytes: index,
        free_bytes: free,
        collation: "utf8mb4_unicode_ci".into(),
        row_format: "Dynamic".into(),
        created: "2026-01-12 08:00:00".into(),
        primary_key: pk,
        ..Default::default()
    };
    let mut tables = vec![
        table(
            "orders (demo)",
            "InnoDB",
            4_096_000,
            1_900_000_000,
            600_000_000,
            4_000_000,
            true,
        ),
        table(
            "order_items (demo)",
            "InnoDB",
            12_800_000,
            3_100_000_000,
            900_000_000,
            400_000_000,
            true,
        ),
        table(
            "audit_log (demo)",
            "InnoDB",
            88_000_000,
            9_700_000_000,
            0,
            0,
            false,
        ),
        table(
            "legacy_stats (demo)",
            "MyISAM",
            42_000,
            18_000_000,
            2_000_000,
            0,
            true,
        ),
    ];
    tables.push(TableInfo {
        name: "daily_sales (demo)".into(),
        kind: "VIEW".into(),
        ..Default::default()
    });
    DatabaseDetail {
        name: database.into(),
        collation: "utf8mb4_unicode_ci".into(),
        tables,
        views: 1,
        routines: 3,
        triggers: 2,
        events: 0,
    }
}
fn demo_table(database: &str, table: &str) -> TableDetail {
    let column = |name: &str, column_type: &str, key: &str, extra: &str| Column {
        name: name.into(),
        column_type: column_type.into(),
        key: key.into(),
        extra: extra.into(),
        ..Default::default()
    };
    TableDetail {
        database: database.into(),
        table: table.into(),
        columns: vec![
            column("id", "bigint(20) unsigned", "PRI", "auto_increment"),
            column("customer_id", "bigint(20) unsigned", "MUL", ""),
            column("created_at", "datetime", "", ""),
            column("total", "decimal(12,2)", "", ""),
        ],
        indexes: vec![
            Index {
                name: "PRIMARY".into(),
                unique: true,
                columns: vec!["id".into()],
                index_type: "BTREE".into(),
                cardinality: 4_096_000,
            },
            Index {
                name: "customer".into(),
                unique: false,
                columns: vec!["customer_id".into(), "created_at".into()],
                index_type: "BTREE".into(),
                cardinality: 120_000,
            },
        ],
        foreign_keys: vec![ForeignKey {
            name: "fk_customer".into(),
            columns: vec!["customer_id".into()],
            referenced_table: format!("{database}.customers"),
            referenced_columns: vec!["id".into()],
            on_update: "RESTRICT".into(),
            on_delete: "CASCADE".into(),
        }],
        create_sql: "CREATE TABLE `orders` (\n  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,\n  ...\n) ENGINE=InnoDB (demo)".into(),
    }
}
fn demo_account(account: &Account) -> AccountDetail {
    let set = |scope: GrantScope, privileges: &[&str]| PrivilegeSet {
        scope,
        privileges: privileges.iter().map(|p| p.to_string()).collect(),
        grantable: false,
    };
    AccountDetail {
        account: Some(account.clone()),
        grants: vec![format!(
            "DEMO: GRANT SELECT ON `analytics`.* TO {}",
            account.label()
        )],
        privileges: vec![
            set(
                GrantScope::Database("analytics".into()),
                &["SELECT", "SHOW VIEW"],
            ),
            set(
                GrantScope::Table("commerce".into(), "orders".into()),
                &["SELECT"],
            ),
        ],
        columns: Vec::new(),
        roles: vec!["reporting (demo)".into()],
        default_role: None,
        properties: (!account.is_role()).then(|| AccountProperties {
            plugin: account.plugin.clone(),
            tls: TlsRequirement::Ssl,
            limits: Limits {
                user_connections: 20,
                ..Default::default()
            },
            ..Default::default()
        }),
    }
}
fn demo_variables(snapshot: &Snapshot) -> Vec<Variable> {
    let variable =
        |name: &str, value: &str, default: &str, var_type: &str, origin: &str| Variable {
            name: name.into(),
            value: value.into(),
            default: Some(default.into()),
            origin: origin.into(),
            scope: "GLOBAL".into(),
            var_type: var_type.into(),
            min: var_type.contains("INT").then(|| "0".into()),
            max: var_type
                .contains("INT")
                .then(|| "18446744073709551615".into()),
            allowed: if var_type == "BOOLEAN" {
                vec!["OFF".into(), "ON".into()]
            } else {
                Vec::new()
            },
            read_only: false,
            comment: "Ukázková proměnná (demo)".into(),
        };
    let mut list = vec![
        variable(
            "innodb_buffer_pool_size",
            "8589934592",
            "134217728",
            "BIGINT UNSIGNED",
            "CONFIG",
        ),
        variable(
            "long_query_time",
            "2.000000",
            "10.000000",
            "DOUBLE",
            "CONFIG",
        ),
        variable(
            "max_connections",
            &snapshot.max_connections.to_string(),
            "151",
            "BIGINT UNSIGNED",
            "CONFIG",
        ),
        variable("slow_query_log", "ON", "OFF", "BOOLEAN", "CONFIG"),
        variable(
            "wait_timeout",
            "28800",
            "28800",
            "BIGINT UNSIGNED",
            "COMPILE-TIME",
        ),
    ];
    list.iter_mut()
        .filter(|v| v.name == "max_connections")
        .for_each(|v| v.min = Some("10".into()));
    list
}
fn demo_health(snapshot: &Snapshot) -> HealthFacts {
    HealthFacts {
        anonymous_accounts: Some(vec![]),
        accounts_without_password: Some(vec![]),
        remote_root: Some(vec![]),
        tables_without_pk: Some(vec!["commerce.audit_log".into()]),
        myisam_tables: Some(vec!["commerce.legacy_stats".into()]),
        fragmented_tables: Some(vec!["commerce.order_items".into()]),
        long_transactions: Some(vec!["spojení #1842, 312 s (demo)".into()]),
        max_connections: snapshot.max_connections,
        max_used_connections: Some(snapshot.max_connections * 9 / 10),
        slow_query_log: Some(true),
        have_ssl: Some("YES".into()),
        uptime: snapshot.uptime,
        version: snapshot.version.clone(),
        ..Default::default()
    }
}
fn demo_apply(snapshot: &mut Snapshot, change: Change) -> Result<Vec<String>> {
    // Same validation as on a server; only the effect is simulated.
    sql::statement(&change, false)?;
    match change {
        Change::CreateDatabase { name, collation } => {
            if snapshot.databases.iter().any(|d| d.name == name) {
                return Err(Error::Validation("Databáze už existuje.".into()));
            }
            snapshot.databases.push(Database {
                name,
                tables: 0,
                bytes: 0,
                charset: "utf8mb4".into(),
                collation,
            });
        }
        Change::DropDatabase { name } => {
            let before = snapshot.databases.len();
            snapshot.databases.retain(|d| d.name != name);
            if before == snapshot.databases.len() {
                return Err(Error::Validation("Databáze neexistuje.".into()));
            }
        }
        Change::CreateAccount { user, host, .. } => snapshot.accounts.push(Account {
            user,
            host,
            plugin: "mysql_native_password".into(),
        }),
        Change::CreateRole { name } => snapshot.accounts.push(Account {
            user: name,
            host: String::new(),
            plugin: String::new(),
        }),
        Change::DropAccount { account } => snapshot.accounts.retain(|a| a != &account),
        Change::KillQuery { target } => {
            let p = snapshot
                .processes
                .iter_mut()
                .find(|p| p.id == target.id && p.query == target.query)
                .ok_or_else(|| Error::Validation("Cílový dotaz se změnil.".into()))?;
            p.command = "Sleep".into();
            p.query.clear();
            p.seconds = 0;
        }
        Change::SetGlobal { variable, value } => {
            if variable.name == "max_connections" {
                snapshot.max_connections = variable.literal(&value)?.parse().unwrap_or(151);
            }
        }
        Change::KillConnection { target } => {
            let before = snapshot.processes.len();
            snapshot.processes.retain(|p| p.id != target.id);
            if before == snapshot.processes.len() {
                return Err(Error::Validation("Spojení už neexistuje.".into()));
            }
        }
        Change::ReplicaControl { start, .. } => {
            return Ok(vec![format!(
                "DEMO: replikace {}.",
                if start { "spuštěna" } else { "zastavena" }
            )])
        }
        Change::PurgeBinlogs { to } => {
            return Ok(vec![format!("DEMO: smazány binární logy starší než {to}.")])
        }
        Change::ResetStatementStats => return Ok(vec!["DEMO: statistiky vynulovány.".into()]),
        Change::Maintenance {
            database,
            table,
            action,
        } => {
            return Ok(vec![format!(
                "{database}.{table} • status: OK ({} — demo)",
                action.keyword()
            )])
        }
        _ => {
            return Err(Error::Unsupported(
                "Demo simuluje databáze, účty, role, proměnnou max_connections, údržbu a ukončení dotazu. Ostatní zápisy vyžadují testovací server.".into(),
            ))
        }
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn demo_simulates_writes_with_real_validation() {
        let mut backend = Backend::demo();
        assert!(backend
            .apply(Change::DropDatabase {
                name: "mysql".into()
            })
            .await
            .is_err());
        backend
            .apply(Change::DropDatabase {
                name: "identity".into(),
            })
            .await
            .unwrap();
        assert!(!backend
            .snapshot()
            .await
            .unwrap()
            .databases
            .iter()
            .any(|d| d.name == "identity"));
        let output = backend
            .apply(Change::Maintenance {
                database: "commerce".into(),
                table: "orders".into(),
                action: Maintenance::Check,
            })
            .await
            .unwrap();
        assert!(output[0].contains("OK"));
        assert!(!backend.health().await.unwrap().is_empty());
        assert!(!backend.variables().await.unwrap().is_empty());
    }
}
