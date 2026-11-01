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
    #[error("Databázová operace selhala (kód {0}). Citlivé údaje nejsou vypsány.")]
    Database(String),
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
    pub name: String,
    pub transport: Transport,
    pub user: String,
    pub tls: TlsMode,
    pub ca: Option<String>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Nové připojení".into(),
            transport: Transport::Tcp {
                host: "localhost".into(),
                port: 3306,
            },
            user: "harbor".into(),
            tls: TlsMode::VerifyIdentity,
            ca: None,
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
            _ => Ok(()),
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
        format!("{}@{}", self.user, self.host)
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
        ["mysql", "information_schema", "performance_schema", "sys"].contains(&self.name.as_str())
    }
}
#[derive(Clone, Debug)]
pub struct TableInfo {
    pub name: String,
    pub engine: String,
    pub rows: u64,
    pub bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub id: u64,
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
}
impl Snapshot {
    pub fn bytes(&self) -> u64 {
        self.databases.iter().map(|d| d.bytes).sum()
    }
    pub fn long_queries(&self) -> usize {
        self.processes.iter().filter(|p| p.long_query()).count()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Privilege {
    Select,
    Insert,
    Update,
    Delete,
}
impl Privilege {
    pub const ALL: [Self; 4] = [Self::Select, Self::Insert, Self::Update, Self::Delete];
    pub fn sql(self) -> &'static str {
        match self {
            Self::Select => "SELECT",
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
        }
    }
}
#[derive(Clone)]
pub enum Change {
    CreateDatabase {
        name: String,
        collation: String,
    },
    CreateAccount {
        user: String,
        host: String,
        password: Secret,
    },
    ChangePassword {
        account: Account,
        password: Secret,
    },
    DatabaseGrant {
        account: Account,
        database: String,
        privilege: Privilege,
        grant: bool,
    },
    KillQuery {
        target: Process,
    },
}
impl Change {
    pub fn summary(&self) -> String {
        match self {
            Self::CreateDatabase { name, collation } => {
                format!("Vytvořit databázi {name} ({collation})")
            }
            Self::CreateAccount { user, host, .. } => {
                format!("Vytvořit účet {user}@{host}; heslo skryto")
            }
            Self::ChangePassword { account, .. } => {
                format!("Změnit heslo účtu {}; heslo skryto", account.label())
            }
            Self::DatabaseGrant {
                account,
                database,
                privilege,
                grant,
            } => format!(
                "{} {} na {} pro {}",
                if *grant { "Přidat" } else { "Odebrat" },
                privilege.sql(),
                database,
                account.label()
            ),
            Self::KillQuery { target } => format!(
                "Ukončit dotaz #{} účtu {} v {} (relace zůstane otevřena)",
                target.id, target.user, target.database
            ),
        }
    }
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
    previous: Option<(u64, u64, Instant)>,
}
impl Rate {
    pub fn sample(&mut self, count: u64, uptime: u64, now: Instant) -> Option<f64> {
        let result = self.previous.and_then(|(old, old_uptime, at)| {
            let duration = now.saturating_duration_since(at);
            (count >= old && uptime >= old_uptime && duration >= Duration::from_millis(1))
                .then(|| (count - old) as f64 / duration.as_secs_f64())
        });
        self.previous = Some((count, uptime, now));
        result
    }
}
