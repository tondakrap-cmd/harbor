use crate::admin::*;
use crate::host::HostSettings;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Síť: server není dostupný nebo spojení zaniklo.")]
    Network,
    #[error("Přihlášení: nesprávný účet nebo heslo.")]
    Authentication,
    #[error("TLS: nelze ověřit zabezpečení nebo identitu serveru.")]
    Tls,
    #[error("Oprávnění: účet nemá právo provést tuto operaci.")]
    Permission,
    #[error("Nepodporovaná funkce: {0}")]
    Unsupported(String),
    #[error("Neplatný vstup: {0}")]
    Validation(String),
    #[error("Časový limit. U zápisu ověřte výsledek na serveru před opakováním.")]
    Timeout,
    #[error("Konfigurace: {0}")]
    Config(String),
    #[error("Interní chyba: databázové vlákno skončilo. Restartujte aplikaci.")]
    Worker,
    #[error(
        "Protokol: na adrese neodpovídá MariaDB/MySQL server (zkontrolujte port a typ služby)."
    )]
    Protocol,
    #[error("Databázová operace selhala (kód {0}). Citlivé údaje nejsou vypsány.")]
    Database(String),
    #[error("Soubor: {0}")]
    File(String),
    #[error("Zrušeno uživatelem.")]
    Cancelled,
    /// Result of a background job; the text is already complete.
    #[error("{0}")]
    Job(String),
}
impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::Tls(_) => Self::Tls,
            sqlx::Error::Io(ref error)
                if error
                    .get_ref()
                    .is_some_and(|source| source.is::<rustls::Error>()) =>
            {
                Self::Tls
            }
            sqlx::Error::Io(_) | sqlx::Error::PoolClosed | sqlx::Error::PoolTimedOut => {
                Self::Network
            }
            sqlx::Error::Database(e) => {
                let number = e
                    .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                    .map(|e| e.number());
                match number {
                    Some(1045 | 1698) => Self::Authentication,
                    Some(1044 | 1142 | 1143 | 1227) => Self::Permission,
                    Some(number) => Self::Database(number.to_string()),
                    None => Self::Database("neznámý".into()),
                }
            }
            sqlx::Error::ColumnDecode { index, .. } => {
                Self::Database(format!("nekompatibilní typ sloupce {index}"))
            }
            sqlx::Error::ColumnNotFound(index) => Self::Database(format!("chybí sloupec {index}")),
            sqlx::Error::Configuration(_) => {
                Self::Config("neplatné parametry připojení (hostitel, socket nebo CA).".into())
            }
            sqlx::Error::Protocol(_) => Self::Protocol,
            _ => Self::Database("ovladač".into()),
        }
    }
}
// Intentionally no Debug / Serialize: secrets must never enter logs or profiles.
#[derive(Clone, Default)]
pub struct Secret(pub Zeroizing<String>);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Transport {
    Tcp { host: String, port: u16 },
    Unix { path: String },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum TlsMode {
    #[default]
    VerifyIdentity,
    InsecureTestOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    /// Stable key of a saved profile. Empty = not saved yet. Names are only labels.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub transport: Transport,
    pub user: String,
    pub tls: TlsMode,
    pub ca: Option<String>,
    /// Production safety: no changes, restores, service actions or console writes.
    #[serde(default)]
    pub read_only: bool,
    /// Access to the server machine (service, logs, configuration); optional.
    #[serde(default)]
    pub host: HostSettings,
    /// Fields written by a newer Harbor version; kept so an older client does not drop them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Nové připojení".into(),
            transport: Transport::Tcp {
                host: "localhost".into(),
                port: 3306,
            },
            user: "harbor".into(),
            tls: TlsMode::VerifyIdentity,
            ca: None,
            read_only: false,
            host: HostSettings::default(),
            extra: serde_json::Map::new(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.user.is_empty() {
            return Err(Error::Validation("Vyplňte název a uživatele.".into()));
        }
        match &self.transport {
            Transport::Tcp { host, port }
                if host.is_empty() || *port == 0 || host.contains("://") =>
            {
                Err(Error::Validation(
                    "Zadejte hostitele a port, nikoli spojovací URL.".into(),
                ))
            }
            Transport::Unix { path } if !path.starts_with('/') => {
                Err(Error::Validation("Socket musí mít absolutní cestu.".into()))
            }
            _ => self.host.validate(),
        }
    }
    pub fn security_label(&self) -> &'static str {
        match self.transport {
            Transport::Unix { .. } => "Lokální Unix socket",
            _ if self.tls == TlsMode::VerifyIdentity => "TLS: ověření certifikátu a identity",
            _ => "NEŠIFROVANÉ testovací TCP",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub host: String,
    pub plugin: String,
}
impl Account {
    pub fn label(&self) -> String {
        if self.is_role() {
            format!("{} (role)", self.user)
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }
    /// MariaDB roles are rows of mysql.user with an empty host.
    pub fn is_role(&self) -> bool {
        self.host.is_empty()
    }
}
#[derive(Clone, Debug)]
pub struct Database {
    pub name: String,
    pub tables: u64,
    pub bytes: u64,
    pub charset: String,
    pub collation: String,
}
impl Database {
    pub fn system(&self) -> bool {
        Self::system_name(&self.name)
    }
    pub fn system_name(name: &str) -> bool {
        ["mysql", "information_schema", "performance_schema", "sys"].contains(&name)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub id: u64,
    /// MariaDB QUERY_ID of the running statement; KILL QUERY ID targets exactly this one.
    pub query_id: u64,
    pub user: String,
    pub host: String,
    pub database: String,
    pub command: String,
    pub seconds: u64,
    pub state: String,
    pub query: String,
}
impl Process {
    pub fn running_query(&self) -> bool {
        matches!(self.command.as_str(), "Query" | "Execute") && !self.query.is_empty()
    }
    pub fn long_query(&self) -> bool {
        self.running_query() && self.seconds >= 30
    }
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub version: String,
    pub uptime: u64,
    pub questions: u64,
    pub connections: u64,
    pub max_connections: u64,
    pub databases: Vec<Database>,
    pub accounts: Vec<Account>,
    pub processes: Vec<Process>,
    pub notices: Vec<String>,
    pub timezone: String,
    pub demo: bool,
    /// Server connection the counters were read on; a reconnect restarts rate sampling.
    pub source: u64,
    /// @@datadir and @@log_error (for disk usage and the log viewer).
    pub datadir: String,
    pub log_error: String,
}
impl Snapshot {
    pub fn bytes(&self) -> u64 {
        self.databases.iter().map(|d| d.bytes).sum()
    }
    pub fn long_queries(&self) -> usize {
        self.processes.iter().filter(|p| p.long_query()).count()
    }
}
#[derive(Clone)]
pub enum Change {
    CreateDatabase {
        name: String,
        collation: String,
    },
    DropDatabase {
        name: String,
    },
    CreateAccount {
        user: String,
        host: String,
        password: Secret,
    },
    /// DROP USER, or DROP ROLE for a role.
    DropAccount {
        account: Account,
    },
    ChangePassword {
        account: Account,
        password: Secret,
    },
    /// GRANT and/or REVOKE at one scope; entries come from the scope's whitelist.
    Privileges {
        account: Account,
        scope: GrantScope,
        grant: Vec<String>,
        revoke: Vec<String>,
    },
    CreateRole {
        name: String,
    },
    RoleMembership {
        role: String,
        account: Account,
        grant: bool,
    },
    DefaultRole {
        account: Account,
        role: Option<String>,
    },
    /// One ALTER USER with all account properties; `tls: None` keeps the current
    /// requirement (used for REQUIRE ISSUER/SUBJECT that Harbor does not rewrite).
    AlterAccount {
        account: Account,
        locked: bool,
        expire_password: bool,
        tls: Option<TlsRequirement>,
        limits: Limits,
    },
    /// SET GLOBAL; `variable` is the metadata the user saw (re-checked on the server).
    SetGlobal {
        variable: Variable,
        value: String,
    },
    Maintenance {
        database: String,
        table: String,
        action: Maintenance,
    },
    KillQuery {
        target: Process,
    },
    /// KILL CONNECTION: ends the whole session (rolls back its open transaction).
    KillConnection {
        target: Process,
    },
    /// START SLAVE / STOP SLAVE for one replication connection ('' = default).
    ReplicaControl {
        connection: String,
        start: bool,
    },
    /// PURGE BINARY LOGS TO 'name': deletes all older binary logs.
    PurgeBinlogs {
        to: String,
    },
    /// Clears performance_schema statement statistics.
    ResetStatementStats,
}
impl Change {
    pub fn summary(&self) -> String {
        match self {
            Self::CreateDatabase { name, collation } => {
                format!("Vytvořit databázi {name} ({collation})")
            }
            Self::DropDatabase { name } => {
                format!("SMAZAT databázi {name} včetně všech tabulek a dat")
            }
            Self::CreateAccount { user, host, .. } => {
                format!("Vytvořit účet {user}@{host}; heslo skryto")
            }
            Self::DropAccount { account } => format!("SMAZAT účet {}", account.label()),
            Self::ChangePassword { account, .. } => {
                format!("Změnit heslo účtu {}; heslo skryto", account.label())
            }
            Self::Privileges {
                account,
                scope,
                grant,
                revoke,
            } => {
                let mut parts = Vec::new();
                if !grant.is_empty() {
                    parts.push(format!("přidat {}", grant.join(", ")));
                }
                if !revoke.is_empty() {
                    parts.push(format!("odebrat {}", revoke.join(", ")));
                }
                format!(
                    "Oprávnění {} na {}: {}",
                    account.label(),
                    scope.label(),
                    parts.join("; ")
                )
            }
            Self::CreateRole { name } => format!("Vytvořit roli {name}"),
            Self::RoleMembership {
                role,
                account,
                grant,
            } => format!(
                "{} roli {role} účtu {}",
                if *grant { "Přidělit" } else { "Odebrat" },
                account.label()
            ),
            Self::DefaultRole { account, role } => format!(
                "Výchozí role účtu {}: {}",
                account.label(),
                role.as_deref().unwrap_or("žádná")
            ),
            Self::AlterAccount {
                account,
                locked,
                expire_password,
                tls,
                limits,
            } => format!(
                "Vlastnosti účtu {}: {}, TLS {}, limity {}/{}/{}/{}{}",
                account.label(),
                if *locked { "ZAMČEN" } else { "odemčen" },
                tls.map(|t| t.label()).unwrap_or("beze změny"),
                limits.queries_per_hour,
                limits.updates_per_hour,
                limits.connections_per_hour,
                limits.user_connections,
                if *expire_password {
                    ", heslo vyprší (změna při příštím přihlášení)"
                } else {
                    ""
                }
            ),
            Self::SetGlobal { variable, value } => format!(
                "SET GLOBAL {} = {value} (nyní {}; platí do restartu serveru)",
                variable.name, variable.value
            ),
            Self::Maintenance {
                database,
                table,
                action,
            } => format!(
                "{} {database}.{table}: {}",
                action.keyword(),
                action.description()
            ),
            Self::KillQuery { target } => format!(
                "Ukončit dotaz #{} účtu {} v {} (relace zůstane otevřena)",
                target.id, target.user, target.database
            ),
            Self::KillConnection { target } => format!(
                "Ukončit SPOJENÍ #{} účtu {}@{}; jeho otevřená transakce se vrátí zpět",
                target.id, target.user, target.host
            ),
            Self::ReplicaControl { connection, start } => format!(
                "{} replikaci (spojení {})",
                if *start { "Spustit" } else { "Zastavit" },
                if connection.is_empty() { "výchozí" } else { connection }
            ),
            Self::PurgeBinlogs { to } => format!(
                "SMAZAT všechny binární logy starší než {to}; repliky, které je ještě nepřečetly, přestanou fungovat"
            ),
            Self::ResetStatementStats => {
                "Vynulovat statistiky dotazů v performance_schema".into()
            }
        }
    }
    /// Irreversible changes; UIs require typing the object's name to confirm.
    pub fn destructive_name(&self) -> Option<&str> {
        match self {
            Self::DropDatabase { name } => Some(name),
            Self::DropAccount { account } => Some(&account.user),
            Self::PurgeBinlogs { to } => Some(to),
            _ => None,
        }
    }
}
/// Unique enough for a per-user config file; collisions are additionally checked on insert.
pub fn new_profile_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!(
        "p{nanos:x}-{:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
/// Case-insensitive search shared by both UIs: empty filter matches everything.
pub fn matches_filter(filter: &str, fields: &[&str]) -> bool {
    let filter = filter.trim().to_lowercase();
    filter.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&filter))
}
pub fn identifier(value: &str) -> Result<String> {
    if value.is_empty()
        || value.chars().count() > 64
        || value
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\' || c == '.')
        || value.ends_with(' ')
    {
        return Err(Error::Validation(
            "Název musí mít 1–64 znaků, bez řídicích znaků, teček a lomítek.".into(),
        ));
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}
/// SQL string literal; sessions run with NO_BACKSLASH_ESCAPES, so only quotes double.
pub fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
pub fn safe_text(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                '�'
            } else {
                c
            }
        })
        .collect()
}
pub fn bytes_label(bytes: u64) -> String {
    format!("≈ {:.1} MiB", bytes as f64 / 1_048_576.0)
}
#[derive(Default)]
pub struct Rate {
    previous: Option<(u64, u64, u64, Instant)>,
}
impl Rate {
    /// `source` identifies the counter's origin (server connection); a change, a server
    /// restart (uptime going back) or a counter going back starts sampling over.
    pub fn sample(&mut self, count: u64, uptime: u64, source: u64, now: Instant) -> Option<f64> {
        let result = self.previous.and_then(|(old, old_uptime, old_source, at)| {
            let duration = now.saturating_duration_since(at);
            (source == old_source
                && count >= old
                && uptime >= old_uptime
                && duration >= Duration::from_millis(1))
            .then(|| (count - old) as f64 / duration.as_secs_f64())
        });
        self.previous = Some((count, uptime, source, now));
        result
    }
}
