//! Administration data (database/table details, privileges, variables, health checks)
//! and the pure rules around them. No I/O here, so everything is unit-testable.
use crate::*;
use std::collections::BTreeSet;

// ---------------------------------------------------------------- databases

#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub name: String,
    /// "BASE TABLE", "VIEW", "SEQUENCE", "SYSTEM VERSIONED"…
    pub kind: String,
    pub engine: String,
    /// Estimate from information_schema (exact only for MyISAM/Aria).
    pub rows: u64,
    pub data_bytes: u64,
    pub index_bytes: u64,
    /// Allocated but unused space (fragmentation, or free space of a shared tablespace).
    pub free_bytes: u64,
    pub collation: String,
    pub row_format: String,
    pub created: String,
    pub updated: String,
    pub auto_increment: Option<u64>,
    pub primary_key: bool,
    pub comment: String,
}
impl TableInfo {
    pub fn bytes(&self) -> u64 {
        self.data_bytes + self.index_bytes
    }
    pub fn view(&self) -> bool {
        self.kind == "VIEW"
    }
    /// Reasons this table deserves attention; empty = nothing to report.
    pub fn warnings(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.view() {
            return out;
        }
        if !self.primary_key {
            out.push("bez primárního klíče");
        }
        if matches!(self.engine.as_str(), "MyISAM" | "MRG_MyISAM") {
            out.push("MyISAM (bez transakcí a obnovy po pádu)");
        }
        if self.fragmented() {
            out.push("velké nevyužité místo (OPTIMIZE)");
        }
        out
    }
    /// At least 100 MiB and 20 % of the table unused.
    pub fn fragmented(&self) -> bool {
        self.free_bytes >= 100 * 1_048_576 && self.free_bytes * 5 >= self.bytes().max(1)
    }
}
#[derive(Clone, Debug, Default)]
pub struct DatabaseDetail {
    pub name: String,
    pub collation: String,
    pub tables: Vec<TableInfo>,
    pub views: u64,
    pub routines: u64,
    pub triggers: u64,
    pub events: u64,
}
impl DatabaseDetail {
    pub fn data_bytes(&self) -> u64 {
        self.tables.iter().map(|t| t.data_bytes).sum()
    }
    pub fn index_bytes(&self) -> u64 {
        self.tables.iter().map(|t| t.index_bytes).sum()
    }
    pub fn free_bytes(&self) -> u64 {
        self.tables.iter().map(|t| t.free_bytes).sum()
    }
    pub fn base_tables(&self) -> impl Iterator<Item = &TableInfo> {
        self.tables.iter().filter(|t| !t.view())
    }
    /// Summary lines for the database header (counts of problematic tables).
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let count = |f: &dyn Fn(&TableInfo) -> bool| self.base_tables().filter(|t| f(t)).count();
        let no_pk = count(&|t| !t.primary_key);
        if no_pk > 0 {
            out.push(format!(
                "{no_pk} tabulek bez primárního klíče (replikace, výkon, Galera)"
            ));
        }
        let myisam = count(&|t| matches!(t.engine.as_str(), "MyISAM" | "MRG_MyISAM"));
        if myisam > 0 {
            out.push(format!(
                "{myisam} tabulek MyISAM: bez transakcí, po pádu nutná oprava"
            ));
        }
        let collation = count(&|t| !t.collation.is_empty() && t.collation != self.collation);
        if collation > 0 {
            out.push(format!(
                "{collation} tabulek má jinou kolaci než databáze ({})",
                self.collation
            ));
        }
        let fragmented = count(&|t| t.fragmented());
        if fragmented > 0 {
            out.push(format!(
                "{fragmented} tabulek s velkým nevyužitým místem (zvažte OPTIMIZE)"
            ));
        }
        out
    }
}
#[derive(Clone, Debug, Default)]
pub struct Column {
    pub name: String,
    pub column_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// PRI / UNI / MUL
    pub key: String,
    pub extra: String,
    pub collation: String,
    pub comment: String,
}
#[derive(Clone, Debug, Default)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    pub columns: Vec<String>,
    pub index_type: String,
    pub cardinality: u64,
}
#[derive(Clone, Debug, Default)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}
#[derive(Clone, Debug, Default)]
pub struct TableDetail {
    pub database: String,
    pub table: String,
    pub columns: Vec<Column>,
    pub indexes: Vec<Index>,
    pub foreign_keys: Vec<ForeignKey>,
    pub create_sql: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maintenance {
    Analyze,
    Check,
    Optimize,
    Repair,
}
impl Maintenance {
    pub const ALL: [Self; 4] = [Self::Analyze, Self::Check, Self::Optimize, Self::Repair];
    pub fn sql(self) -> &'static str {
        match self {
            Self::Analyze => "ANALYZE TABLE",
            Self::Check => "CHECK TABLE",
            Self::Optimize => "OPTIMIZE TABLE",
            Self::Repair => "REPAIR TABLE",
        }
    }
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Analyze => "ANALYZE",
            Self::Check => "CHECK",
            Self::Optimize => "OPTIMIZE",
            Self::Repair => "REPAIR",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.keyword().eq_ignore_ascii_case(value.trim()))
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Analyze => "přepočítá statistiky pro optimalizátor (rychlé, krátký zámek)",
            Self::Check => "zkontroluje tabulku a indexy na chyby (jen čte)",
            Self::Optimize => {
                "přestaví tabulku a uvolní nevyužité místo (u velkých tabulek dlouho, zatíží disk)"
            }
            Self::Repair => "opraví poškozenou tabulku MyISAM/Aria (InnoDB nepodporuje)",
        }
    }
}

// ---------------------------------------------------------------- privileges

/// Privileges grantable at the global level on MariaDB 10.11 and 11.4.
pub const GLOBAL_PRIVILEGES: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "CREATE TEMPORARY TABLES",
    "LOCK TABLES",
    "EXECUTE",
    "CREATE VIEW",
    "SHOW VIEW",
    "CREATE ROUTINE",
    "ALTER ROUTINE",
    "EVENT",
    "TRIGGER",
    "DELETE HISTORY",
    "SHOW DATABASES",
    "PROCESS",
    "RELOAD",
    "SHUTDOWN",
    "FILE",
    "CREATE USER",
    "CREATE TABLESPACE",
    "SUPER",
    "REPLICATION SLAVE",
    "BINLOG MONITOR",
    "BINLOG ADMIN",
    "BINLOG REPLAY",
    "SLAVE MONITOR",
    "REPLICATION SLAVE ADMIN",
    "REPLICATION MASTER ADMIN",
    "CONNECTION ADMIN",
    "READ_ONLY ADMIN",
    "FEDERATED ADMIN",
    "SET USER",
];
pub const DATABASE_PRIVILEGES: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "CREATE TEMPORARY TABLES",
    "LOCK TABLES",
    "EXECUTE",
    "CREATE VIEW",
    "SHOW VIEW",
    "CREATE ROUTINE",
    "ALTER ROUTINE",
    "EVENT",
    "TRIGGER",
    "DELETE HISTORY",
];
pub const TABLE_PRIVILEGES: &[&str] = &[
    "SELECT",
    "INSERT",
    "UPDATE",
    "DELETE",
    "CREATE",
    "DROP",
    "ALTER",
    "INDEX",
    "REFERENCES",
    "CREATE VIEW",
    "SHOW VIEW",
    "TRIGGER",
    "DELETE HISTORY",
];
/// Pseudo-privilege: rendered as WITH GRANT OPTION / REVOKE GRANT OPTION.
pub const GRANT_OPTION: &str = "GRANT OPTION";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GrantScope {
    Global,
    Database(String),
    /// An existing database-level grant whose name uses unescaped `_`/`%` wildcards
    /// (e.g. made by `GRANT … ON shop_db.*`). Kept verbatim so edits hit the same grant.
    DatabasePattern(String),
    Table(String, String),
}
impl GrantScope {
    pub fn label(&self) -> String {
        match self {
            Self::Global => "*.* (celý server)".into(),
            Self::Database(db) => format!("{db}.*"),
            Self::DatabasePattern(pattern) => format!("{pattern}.* (vzor se zástupnými znaky)"),
            Self::Table(db, table) => format!("{db}.{table}"),
        }
    }
    /// Privileges valid at this level (GRANT OPTION is allowed everywhere).
    pub fn privileges(&self) -> &'static [&'static str] {
        match self {
            Self::Global => GLOBAL_PRIVILEGES,
            Self::Database(_) | Self::DatabasePattern(_) => DATABASE_PRIVILEGES,
            Self::Table(..) => TABLE_PRIVILEGES,
        }
    }
    /// "*.*", "db.*" or "db.table"; names may be written with or without backticks.
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        if value == "*.*" || value == "*" {
            return Ok(Self::Global);
        }
        let parts = split_qualified(value).ok_or_else(|| {
            Error::Validation("Rozsah zadejte jako *.*, databáze.* nebo databáze.tabulka.".into())
        })?;
        let scope = match parts {
            (db, None) => Self::Database(db),
            (db, Some(t)) if t == "*" => Self::Database(db),
            (db, Some(t)) => Self::Table(db, t),
        };
        match &scope {
            Self::Database(db) => {
                identifier(db)?;
            }
            Self::Table(db, t) => {
                identifier(db)?;
                identifier(t)?;
            }
            Self::Global | Self::DatabasePattern(_) => (),
        }
        Ok(scope)
    }
}
/// `db`, `db.table`, `` `my.db`.`t` `` -> (db, table). Backticks allow dots in names.
fn split_qualified(value: &str) -> Option<(String, Option<String>)> {
    let mut parts = Vec::new();
    let mut chars = value.chars().peekable();
    while chars.peek().is_some() {
        let mut part = String::new();
        if chars.peek() == Some(&'`') {
            chars.next();
            loop {
                match chars.next()? {
                    '`' if chars.peek() == Some(&'`') => {
                        chars.next();
                        part.push('`');
                    }
                    '`' => break,
                    c => part.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == '.' {
                    break;
                }
                part.push(c);
                chars.next();
            }
        }
        parts.push(part);
        match chars.next() {
            Some('.') => (),
            None => break,
            Some(_) => return None,
        }
    }
    match parts.as_slice() {
        [db] if !db.is_empty() => Some((db.clone(), None)),
        [db, t] if !db.is_empty() && !t.is_empty() => Some((db.clone(), Some(t.clone()))),
        _ => None,
    }
}
/// Database name stored in a grant (mysql.db) -> scope. `\_`/`\%` are escaped
/// literals; a bare `_`/`%` makes the grant a wildcard pattern.
pub fn database_grant_scope(stored: &str) -> GrantScope {
    let mut name = String::new();
    let mut pattern = false;
    let mut chars = stored.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => name.extend(chars.next()),
            '_' | '%' => {
                pattern = true;
                name.push(c);
            }
            c => name.push(c),
        }
    }
    if pattern {
        GrantScope::DatabasePattern(stored.into())
    } else {
        GrantScope::Database(name)
    }
}
/// Privileges an account holds at one scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivilegeSet {
    pub scope: GrantScope,
    pub privileges: BTreeSet<String>,
    pub grantable: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TlsRequirement {
    #[default]
    None,
    Ssl,
    X509,
    /// REQUIRE ISSUER/SUBJECT/CIPHER: shown, never rewritten by Harbor.
    Specified,
}
impl TlsRequirement {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "nevyžaduje",
            Self::Ssl => "TLS (SSL)",
            Self::X509 => "TLS s klientským certifikátem (X509)",
            Self::Specified => "konkrétní vydavatel/subjekt/šifra",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub queries_per_hour: u64,
    pub updates_per_hour: u64,
    pub connections_per_hour: u64,
    pub user_connections: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountProperties {
    pub plugin: String,
    pub locked: bool,
    pub password_expired: bool,
    pub tls: TlsRequirement,
    pub limits: Limits,
}
#[derive(Clone, Debug, Default)]
pub struct AccountDetail {
    pub account: Option<Account>,
    /// SHOW GRANTS output with authentication data removed.
    pub grants: Vec<String>,
    pub privileges: Vec<PrivilegeSet>,
    /// Column-level privileges, read-only summary lines.
    pub columns: Vec<String>,
    pub roles: Vec<String>,
    pub default_role: Option<String>,
    /// None when mysql.global_priv is not readable for the connected account.
    pub properties: Option<AccountProperties>,
}
impl AccountDetail {
    pub fn privileges_at(&self, scope: &GrantScope) -> Option<&PrivilegeSet> {
        self.privileges.iter().find(|p| &p.scope == scope)
    }
}
/// The change needed to move from `current` to `wanted` at one scope (GRANT OPTION
/// included as a pseudo-privilege). Returns (to grant, to revoke).
pub fn privilege_diff(
    current: &BTreeSet<String>,
    wanted: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    (
        wanted.difference(current).cloned().collect(),
        current.difference(wanted).cloned().collect(),
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    ReadOnly,
    Application,
    Backup,
    Monitoring,
}
impl Template {
    pub const ALL: [Self; 4] = [
        Self::ReadOnly,
        Self::Application,
        Self::Backup,
        Self::Monitoring,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Jen čtení (databáze)",
            Self::Application => "Aplikační účet (databáze)",
            Self::Backup => "Zálohovací účet (server)",
            Self::Monitoring => "Monitoring (server)",
        }
    }
    pub fn keyword(self) -> &'static str {
        match self {
            Self::ReadOnly => "READONLY",
            Self::Application => "APP",
            Self::Backup => "BACKUP",
            Self::Monitoring => "MONITOR",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.keyword().eq_ignore_ascii_case(value.trim()))
    }
    pub fn needs_database(self) -> bool {
        matches!(self, Self::ReadOnly | Self::Application)
    }
    pub fn privileges(self) -> &'static [&'static str] {
        match self {
            Self::ReadOnly => &["SELECT", "SHOW VIEW"],
            Self::Application => &[
                "SELECT",
                "INSERT",
                "UPDATE",
                "DELETE",
                "EXECUTE",
                "CREATE TEMPORARY TABLES",
                "LOCK TABLES",
            ],
            // A consistent logical dump: data, views, triggers, events, routines.
            Self::Backup => &[
                "SELECT",
                "SHOW VIEW",
                "TRIGGER",
                "EVENT",
                "LOCK TABLES",
                "RELOAD",
                "PROCESS",
                "BINLOG MONITOR",
            ],
            Self::Monitoring => &["PROCESS", "BINLOG MONITOR", "SLAVE MONITOR"],
        }
    }
    /// The change granting this template; database templates need a database name.
    pub fn change(self, account: Account, database: &str) -> Result<Change> {
        let scope = if self.needs_database() {
            identifier(database)?;
            GrantScope::Database(database.into())
        } else {
            GrantScope::Global
        };
        Ok(Change::Privileges {
            account,
            scope,
            grant: self.privileges().iter().map(|p| p.to_string()).collect(),
            revoke: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------- variables

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Variable {
    /// Lowercase name as used in SET GLOBAL.
    pub name: String,
    pub value: String,
    pub default: Option<String>,
    /// COMPILE-TIME, CONFIG, COMMAND-LINE, AUTO, SQL, ENVIRONMENT.
    pub origin: String,
    /// GLOBAL, SESSION, SESSION ONLY.
    pub scope: String,
    pub var_type: String,
    pub min: Option<String>,
    pub max: Option<String>,
    pub allowed: Vec<String>,
    pub read_only: bool,
    pub comment: String,
}
impl Variable {
    /// Can be changed at runtime with SET GLOBAL.
    pub fn settable(&self) -> bool {
        !self.read_only && self.scope != "SESSION ONLY"
    }
    /// Value differs from the compiled-in default (set by config, command line or SQL).
    pub fn changed(&self) -> bool {
        self.origin != "COMPILE-TIME"
    }
    /// Changed at runtime: lost on the next server restart unless also in the config.
    pub fn runtime_only(&self) -> bool {
        self.origin == "SQL"
    }
    pub fn key_description(&self) -> Option<&'static str> {
        KEY_VARIABLES
            .iter()
            .find(|(name, _)| *name == self.name)
            .map(|(_, d)| *d)
    }
    /// SQL literal for `value`, validated against the server's metadata for this
    /// variable. Sizes accept K/M/G/T suffixes (powers of 1024).
    pub fn literal(&self, value: &str) -> Result<String> {
        let value = value.trim();
        if value.chars().any(char::is_control) {
            return Err(Error::Validation("Hodnota obsahuje řídicí znaky.".into()));
        }
        let invalid = |why: String| Err(Error::Validation(format!("{}: {why}", self.name)));
        match self.var_type.as_str() {
            "BOOLEAN" => match value.to_ascii_uppercase().as_str() {
                "ON" | "1" | "TRUE" | "YES" => Ok("'ON'".into()),
                "OFF" | "0" | "FALSE" | "NO" => Ok("'OFF'".into()),
                _ => invalid("zadejte ON nebo OFF.".into()),
            },
            "INT" | "BIGINT" | "INT UNSIGNED" | "BIGINT UNSIGNED" => {
                let number = parse_size(value).ok_or_else(|| {
                    Error::Validation(format!(
                        "{}: zadejte celé číslo (lze s příponou K/M/G/T).",
                        self.name
                    ))
                })?;
                let min = self.min.as_deref().and_then(|m| m.parse::<i128>().ok());
                let max = self.max.as_deref().and_then(|m| m.parse::<i128>().ok());
                if self.var_type.ends_with("UNSIGNED") && number < 0 {
                    return invalid("hodnota nesmí být záporná.".into());
                }
                if min.is_some_and(|m| number < m) || max.is_some_and(|m| number > m) {
                    return invalid(format!(
                        "povolený rozsah je {} až {}.",
                        self.min.as_deref().unwrap_or("?"),
                        self.max.as_deref().unwrap_or("?")
                    ));
                }
                Ok(number.to_string())
            }
            "DOUBLE" => {
                let number: f64 = value
                    .parse()
                    .ok()
                    .filter(|n: &f64| n.is_finite())
                    .ok_or_else(|| Error::Validation(format!("{}: zadejte číslo.", self.name)))?;
                let min = self.min.as_deref().and_then(|m| m.parse::<f64>().ok());
                let max = self.max.as_deref().and_then(|m| m.parse::<f64>().ok());
                if min.is_some_and(|m| number < m) || max.is_some_and(|m| number > m) {
                    return invalid(format!(
                        "povolený rozsah je {} až {}.",
                        self.min.as_deref().unwrap_or("?"),
                        self.max.as_deref().unwrap_or("?")
                    ));
                }
                Ok(number.to_string())
            }
            "ENUM" => self
                .allowed
                .iter()
                .find(|a| a.eq_ignore_ascii_case(value))
                .map(|a| quoted(a))
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "{}: povolené hodnoty jsou {}.",
                        self.name,
                        self.allowed.join(", ")
                    ))
                }),
            "SET" => {
                let items: Vec<&str> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut canonical = Vec::new();
                for item in items {
                    match self.allowed.iter().find(|a| a.eq_ignore_ascii_case(item)) {
                        Some(a) => canonical.push(a.as_str()),
                        None => {
                            return invalid(format!(
                                "„{item}“ není povoleno; povolené: {}.",
                                self.allowed.join(", ")
                            ))
                        }
                    }
                }
                Ok(quoted(&canonical.join(",")))
            }
            // VARCHAR, FLAGSET (e.g. optimizer_switch): the server validates the content.
            _ => Ok(quoted(value)),
        }
    }
}
/// "512M" -> 536870912; plain integers pass through.
pub fn parse_size(value: &str) -> Option<i128> {
    let value = value.trim();
    let (digits, factor) = match value.chars().last()?.to_ascii_uppercase() {
        'K' => (&value[..value.len() - 1], 1i128 << 10),
        'M' => (&value[..value.len() - 1], 1i128 << 20),
        'G' => (&value[..value.len() - 1], 1i128 << 30),
        'T' => (&value[..value.len() - 1], 1i128 << 40),
        _ => (value, 1),
    };
    digits.trim().parse::<i128>().ok()?.checked_mul(factor)
}
/// Byte counts in a readable unit (variables, sizes).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
/// The parameters an administrator tunes most often, with a short explanation.
pub const KEY_VARIABLES: &[(&str, &str)] = &[
    ("innodb_buffer_pool_size", "Cache dat a indexů InnoDB. Na dedikovaném serveru obvykle 50–75 % RAM."),
    ("innodb_log_file_size", "Velikost redo logu. Větší = plynulejší zápisy, delší obnova po pádu."),
    ("innodb_flush_log_at_trx_commit", "1 = plná odolnost (výchozí); 2 nebo 0 = rychlejší, při pádu hrozí ztráta poslední sekundy."),
    ("innodb_io_capacity", "Odhad IOPS disku pro práci na pozadí; u SSD výrazně víc než výchozí 200."),
    ("innodb_file_per_table", "Každá tabulka ve vlastním souboru; OPTIMIZE pak vrací místo systému."),
    ("innodb_lock_wait_timeout", "Kolik sekund čeká transakce na zámek řádku, než skončí chybou."),
    ("max_connections", "Maximální počet souběžných spojení. Každé spojení spotřebuje paměť."),
    ("max_connect_errors", "Po tolika chybných pokusech se hostitel zablokuje (FLUSH HOSTS odblokuje)."),
    ("wait_timeout", "Po kolika sekundách nečinnosti server zavře neinteraktivní spojení."),
    ("interactive_timeout", "Totéž pro interaktivní klienty (konzole)."),
    ("max_allowed_packet", "Největší paket / řádek / dotaz; velké BLOBy a importy potřebují víc."),
    ("tmp_table_size", "Limit dočasné tabulky v paměti; nad ním jde na disk (spolu s max_heap_table_size)."),
    ("max_heap_table_size", "Limit tabulek MEMORY a dočasných tabulek v paměti."),
    ("table_open_cache", "Počet otevřených tabulek v cache; nízká hodnota zvyšuje Opened_tables."),
    ("table_definition_cache", "Počet definic tabulek v cache."),
    ("thread_cache_size", "Počet vláken připravených pro nová spojení."),
    ("max_statement_time", "Limit doby běhu dotazu v sekundách (0 = bez limitu)."),
    ("lock_wait_timeout", "Čekání na zámky metadat (ALTER, DDL) v sekundách."),
    ("slow_query_log", "Zapíná log pomalých dotazů."),
    ("long_query_time", "Od kolika sekund je dotaz pomalý."),
    ("log_queries_not_using_indexes", "Logovat i dotazy bez indexu (pozor na objem logu)."),
    ("general_log", "Log všech dotazů — jen krátce pro ladění, velmi zatěžuje."),
    ("expire_logs_days", "Po kolika dnech se mažou binární logy (0 = nikdy)."),
    ("binlog_expire_logs_seconds", "Totéž v sekundách (novější varianta)."),
    ("sync_binlog", "1 = binlog zapsán na disk při každém commitu (bezpečné, pomalejší)."),
    ("binlog_format", "ROW je nejbezpečnější pro replikaci."),
    ("read_only", "Zákaz zápisu pro běžné účty (repliky, údržba)."),
    ("character_set_server", "Výchozí znaková sada nových databází."),
    ("collation_server", "Výchozí kolace nových databází."),
    ("sql_mode", "Přísnost SQL (STRICT_TRANS_TABLES apod.)."),
    ("query_cache_type", "Query cache: na vícejádrových serverech s častými zápisy spíš brzdí."),
    ("query_cache_size", "Velikost query cache (0 = vypnuto)."),
];

// ---------------------------------------------------------------- health

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}
impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "KRITICKÉ",
            Self::Warning => "VAROVÁNÍ",
            Self::Info => "INFO",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub recommendation: String,
}
/// Everything the health check looks at. `None` = could not be read (privileges);
/// such checks are listed as skipped instead of silently passing.
#[derive(Clone, Debug, Default)]
pub struct HealthFacts {
    pub anonymous_accounts: Option<Vec<String>>,
    pub accounts_without_password: Option<Vec<String>>,
    pub remote_root: Option<Vec<String>>,
    pub test_database: Option<bool>,
    pub max_connections: u64,
    pub max_used_connections: Option<u64>,
    pub connections_total: Option<u64>,
    pub aborted_connects: Option<u64>,
    pub buffer_pool_reads: Option<u64>,
    pub buffer_pool_read_requests: Option<u64>,
    pub buffer_pool_size: Option<u64>,
    pub innodb_bytes: Option<u64>,
    pub tmp_tables: Option<u64>,
    pub tmp_disk_tables: Option<u64>,
    pub tables_without_pk: Option<Vec<String>>,
    pub myisam_tables: Option<Vec<String>>,
    pub fragmented_tables: Option<Vec<String>>,
    pub log_bin: Option<bool>,
    pub binlog_expire_seconds: Option<f64>,
    pub slow_query_log: Option<bool>,
    pub long_transactions: Option<Vec<String>>,
    pub have_ssl: Option<String>,
    pub uptime: u64,
    pub version: String,
    /// Replication channels of this server (empty when it is no replica).
    pub replicas: Vec<ReplicaChannel>,
    pub galera: Option<Galera>,
}
fn list(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(8).map(String::as_str).collect();
    let more = items.len().saturating_sub(shown.len());
    if more > 0 {
        format!("{} a dalších {more}", shown.join(", "))
    } else {
        shown.join(", ")
    }
}
/// Rule-based review of a server; ordered by severity.
pub fn evaluate_health(f: &HealthFacts) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let mut add = |severity, title: &str, detail: String, recommendation: &str| {
        out.push(Finding {
            severity,
            title: title.into(),
            detail,
            recommendation: recommendation.into(),
        })
    };
    match &f.anonymous_accounts {
        Some(a) if !a.is_empty() => add(
            Severity::Critical,
            "Anonymní účty",
            format!("Účty bez jména umožňují přihlášení komukoli: {}", list(a)),
            "Odstraňte je (Uživatelé → Smazat účet) nebo spusťte mariadb-secure-installation.",
        ),
        Some(_) => (),
        None => skipped.push("anonymní účty"),
    }
    match &f.accounts_without_password {
        Some(a) if !a.is_empty() => add(
            Severity::Critical,
            "Účty bez hesla",
            format!("Heslem nechráněné účty: {}", list(a)),
            "Nastavte heslo, přepněte na unix_socket, nebo účet zamkněte.",
        ),
        Some(_) => (),
        None => skipped.push("účty bez hesla"),
    }
    match &f.remote_root {
        Some(a) if !a.is_empty() => add(
            Severity::Warning,
            "root je přístupný vzdáleně",
            format!("Účty root mimo localhost: {}", list(a)),
            "Pro vzdálenou správu použijte samostatný pojmenovaný účet a root omezte na localhost.",
        ),
        Some(_) => (),
        None => skipped.push("vzdálený root"),
    }
    if f.test_database == Some(true) {
        add(
            Severity::Info,
            "Existuje databáze test",
            "Ukázková databáze test bývá dostupná všem účtům.".into(),
            "Pokud ji nepoužíváte, odstraňte ji.",
        );
    }
    if let Some(used) = f.max_used_connections {
        if f.max_connections > 0 && used * 100 >= f.max_connections * 85 {
            add(
                Severity::Warning,
                "Spojení se blíží limitu",
                format!(
                    "Nejvíc současně {used} z {} povolených spojení (od startu).",
                    f.max_connections
                ),
                "Zvyšte max_connections (pozor na paměť) nebo zkontrolujte, zda aplikace spojení vrací (pooling).",
            );
        }
    }
    if let (Some(total), Some(aborted)) = (f.connections_total, f.aborted_connects) {
        if total >= 100 && aborted * 10 >= total {
            add(
                Severity::Warning,
                "Mnoho neúspěšných připojení",
                format!("{aborted} z {total} pokusů o spojení selhalo (špatné heslo, síť, limity)."),
                "Zkontrolujte error log a přihlašovací údaje aplikací; může jít i o útok hrubou silou.",
            );
        }
    }
    if let (Some(reads), Some(requests)) = (f.buffer_pool_reads, f.buffer_pool_read_requests) {
        if requests >= 100_000 && reads * 100 > requests {
            add(
                Severity::Warning,
                "Buffer pool nestačí",
                format!(
                    "{:.2} % čtení InnoDB muselo jít na disk.",
                    reads as f64 * 100.0 / requests as f64
                ),
                "Zvyšte innodb_buffer_pool_size (Proměnné), pokud to dovolí RAM.",
            );
        }
    }
    if let (Some(pool), Some(data)) = (f.buffer_pool_size, f.innodb_bytes) {
        if data > pool * 2 && pool <= 128 * 1_048_576 {
            add(
                Severity::Info,
                "Výchozí velikost buffer poolu",
                format!(
                    "Data InnoDB mají {}, buffer pool jen {}.",
                    human_bytes(data),
                    human_bytes(pool)
                ),
                "Na dedikovaném serveru nastavte innodb_buffer_pool_size na 50–75 % RAM.",
            );
        }
    }
    if let (Some(all), Some(disk)) = (f.tmp_tables, f.tmp_disk_tables) {
        if all >= 1000 && disk * 4 >= all {
            add(
                Severity::Info,
                "Dočasné tabulky na disku",
                format!("{disk} z {all} dočasných tabulek muselo na disk."),
                "Zvyšte tmp_table_size a max_heap_table_size, nebo upravte dotazy (TEXT/BLOB jdou na disk vždy).",
            );
        }
    }
    match &f.tables_without_pk {
        Some(t) if !t.is_empty() => add(
            Severity::Warning,
            "Tabulky bez primárního klíče",
            format!("{} tabulek: {}", t.len(), list(t)),
            "Přidejte primární klíč; bez něj trpí replikace (ROW), Galera i výkon InnoDB.",
        ),
        Some(_) => (),
        None => skipped.push("primární klíče"),
    }
    match &f.myisam_tables {
        Some(t) if !t.is_empty() => add(
            Severity::Info,
            "Tabulky MyISAM",
            format!("{} tabulek: {}", t.len(), list(t)),
            "Zvažte převod na InnoDB (ALTER TABLE … ENGINE=InnoDB) kvůli transakcím a obnově po pádu.",
        ),
        Some(_) => (),
        None => skipped.push("MyISAM"),
    }
    if let Some(t) = &f.fragmented_tables {
        if !t.is_empty() {
            add(
                Severity::Info,
                "Fragmentované tabulky",
                format!("Velké nevyužité místo: {}", list(t)),
                "OPTIMIZE TABLE v době nízkého provozu (Databáze → tabulka → Údržba).",
            );
        }
    }
    if f.log_bin == Some(true) && f.binlog_expire_seconds == Some(0.0) {
        add(
            Severity::Warning,
            "Binární logy se nemažou",
            "Binlog je zapnutý a expirace je 0, logy porostou do zaplnění disku.".into(),
            "Nastavte expire_logs_days nebo binlog_expire_logs_seconds (Proměnné) a také v konfiguraci.",
        );
    }
    if f.slow_query_log == Some(false) {
        add(
            Severity::Info,
            "Log pomalých dotazů je vypnutý",
            "Bez něj se špatně hledají dotazy, které server zpomalují.".into(),
            "Zapněte slow_query_log a nastavte long_query_time (např. 1–2 s).",
        );
    }
    match &f.long_transactions {
        Some(t) if !t.is_empty() => add(
            Severity::Warning,
            "Dlouhé otevřené transakce",
            format!("Transakce běžící přes 60 s: {}", list(t)),
            "Dlouhé transakce drží zámky a brání čištění historie (purge). Zjistěte klienta v Aktivních spojeních.",
        ),
        Some(_) => (),
        None => skipped.push("dlouhé transakce (vyžaduje PROCESS)"),
    }
    if let Some(ssl) = &f.have_ssl {
        if ssl != "YES" {
            add(
                Severity::Info,
                "Server nemá zapnuté TLS",
                format!("have_ssl = {ssl}; vzdálená spojení nejdou šifrovat."),
                "Nastavte ssl_cert/ssl_key v konfiguraci serveru, pokud se k němu připojuje síť.",
            );
        }
    }
    if f.uptime > 0 && f.uptime < 86_400 {
        add(
            Severity::Info,
            "Krátká doba běhu",
            format!(
                "Server běží {} h; statistiky (buffer pool, spojení) ještě nemusí být reprezentativní.",
                f.uptime / 3600
            ),
            "Kontrolu zopakujte po delším provozu.",
        );
    }
    for channel in &f.replicas {
        if !channel.healthy() {
            let error = [&channel.last_io_error, &channel.last_sql_error]
                .into_iter()
                .filter(|e| !e.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            add(
                Severity::Critical,
                "Replikace neběží",
                format!(
                    "Spojení {} z {}: IO {}, SQL {}{}",
                    channel.label(),
                    channel.master,
                    channel.io_running,
                    channel.sql_running,
                    if error.is_empty() { String::new() } else { format!(" • {error}") }
                ),
                "Obrazovka Replikace: opravte příčinu chyby a replikaci spusťte; při chybě SQL zvažte přeskočení transakce jen po pečlivé kontrole.",
            );
        } else if channel.seconds_behind.is_some_and(|s| s > 300) {
            add(
                Severity::Warning,
                "Replika se zpožďuje",
                format!(
                    "Spojení {} je {} s za primárním serverem.",
                    channel.label(),
                    channel.seconds_behind.unwrap_or(0)
                ),
                "Zkontrolujte zatížení repliky (disk, dlouhé dotazy) a velké transakce na primárním serveru.",
            );
        }
    }
    if let Some(galera) = &f.galera {
        if galera.cluster_status != "Primary" {
            add(
                Severity::Critical,
                "Uzel Galery není v primární komponentě",
                format!("wsrep_cluster_status = {}; uzel nepřijímá zápisy.", galera.cluster_status),
                "Zkontrolujte síť mezi uzly a stav ostatních uzlů; při úplném výpadku je nutný bootstrap nejnovějšího uzlu.",
            );
        } else if galera.local_state != "Synced" {
            add(
                Severity::Warning,
                "Uzel Galery není synchronizovaný",
                format!("Stav uzlu: {}.", galera.local_state),
                "Počkejte na dokončení synchronizace (IST/SST); pokud trvá dlouho, zkontrolujte log.",
            );
        }
        if galera.flow_control_paused > 0.1 {
            add(
                Severity::Warning,
                "Galera brzdí zápisy (flow control)",
                format!("Cluster byl {:.0} % času pozastaven řízením toku.", galera.flow_control_paused * 100.0),
                "Nejpomalejší uzel nestíhá aplikovat změny; zkontrolujte jeho disk a wsrep_slave_threads.",
            );
        }
    }
    if !skipped.is_empty() {
        add(
            Severity::Info,
            "Některé kontroly nešlo provést",
            format!("Chybí oprávnění: {}", skipped.join(", ")),
            "Pro úplnou kontrolu se připojte účtem s právem čtení mysql.* a PROCESS.",
        );
    }
    out.sort_by_key(|f| f.severity);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn variable(
        var_type: &str,
        min: Option<&str>,
        max: Option<&str>,
        allowed: &[&str],
    ) -> Variable {
        Variable {
            name: "x".into(),
            var_type: var_type.into(),
            min: min.map(Into::into),
            max: max.map(Into::into),
            allowed: allowed.iter().map(|s| s.to_string()).collect(),
            scope: "GLOBAL".into(),
            ..Default::default()
        }
    }
    #[test]
    fn variable_literals_are_typed_and_range_checked() {
        let size = variable(
            "BIGINT UNSIGNED",
            Some("2097152"),
            Some("18446744073701163008"),
            &[],
        );
        assert_eq!(size.literal("256M").unwrap(), "268435456");
        assert!(size.literal("1K").is_err(), "below minimum");
        assert!(size.literal("-5").is_err());
        assert!(size.literal("1; DROP").is_err());
        let boolean = variable("BOOLEAN", None, None, &["OFF", "ON"]);
        assert_eq!(boolean.literal("on").unwrap(), "'ON'");
        assert!(boolean.literal("maybe").is_err());
        let format = variable("ENUM", None, None, &["MIXED", "STATEMENT", "ROW"]);
        assert_eq!(format.literal("row").unwrap(), "'ROW'");
        assert!(format.literal("ROW'; DROP").is_err());
        let set = variable("SET", None, None, &["STRICT_TRANS_TABLES", "NO_ZERO_DATE"]);
        assert_eq!(
            set.literal("strict_trans_tables, NO_ZERO_DATE").unwrap(),
            "'STRICT_TRANS_TABLES,NO_ZERO_DATE'"
        );
        let text = variable("VARCHAR", None, None, &[]);
        assert_eq!(text.literal("a'b").unwrap(), "'a''b'");
        assert!(text.literal("a\nb").is_err());
        let double = variable("DOUBLE", Some("0"), Some("99"), &[]);
        assert_eq!(double.literal("7").unwrap(), "7");
        assert!(double.literal("100").is_err());
        assert!(double.literal("NaN").is_err());
    }
    #[test]
    fn scopes_parse_with_and_without_backticks() {
        assert_eq!(GrantScope::parse("*.*").unwrap(), GrantScope::Global);
        assert_eq!(
            GrantScope::parse("shop.*").unwrap(),
            GrantScope::Database("shop".into())
        );
        assert_eq!(
            GrantScope::parse("shop").unwrap(),
            GrantScope::Database("shop".into())
        );
        assert_eq!(
            GrantScope::parse("`my db`.`a``b`").unwrap(),
            GrantScope::Table("my db".into(), "a`b".into())
        );
        assert!(GrantScope::parse("a.b.c").is_err());
        assert_eq!(
            database_grant_scope("shop\\_db"),
            GrantScope::Database("shop_db".into())
        );
        assert_eq!(
            database_grant_scope("shop_db"),
            GrantScope::DatabasePattern("shop_db".into())
        );
        assert!(GrantScope::parse("").is_err());
    }
    #[test]
    fn diff_grants_only_what_is_missing() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        let (grant, revoke) = privilege_diff(
            &set(&["SELECT", "INSERT"]),
            &set(&["SELECT", "UPDATE", GRANT_OPTION]),
        );
        assert_eq!(grant, vec!["GRANT OPTION".to_string(), "UPDATE".into()]);
        assert_eq!(revoke, vec!["INSERT".to_string()]);
    }
    #[test]
    fn health_rules_report_problems_and_skipped_checks() {
        let facts = HealthFacts {
            anonymous_accounts: Some(vec!["''@'localhost'".into()]),
            accounts_without_password: Some(vec![]),
            remote_root: None,
            max_connections: 100,
            max_used_connections: Some(90),
            log_bin: Some(true),
            binlog_expire_seconds: Some(0.0),
            tables_without_pk: Some(vec!["shop.log".into()]),
            myisam_tables: Some(vec![]),
            long_transactions: Some(vec![]),
            ..Default::default()
        };
        let findings = evaluate_health(&facts);
        let titles: Vec<&str> = findings.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(findings[0].severity, Severity::Critical);
        assert!(titles.contains(&"Anonymní účty"));
        assert!(titles.contains(&"Spojení se blíží limitu"));
        assert!(titles.contains(&"Binární logy se nemažou"));
        assert!(titles.contains(&"Tabulky bez primárního klíče"));
        assert!(!titles.contains(&"Účty bez hesla"));
        let skipped = findings
            .iter()
            .find(|f| f.title == "Některé kontroly nešlo provést")
            .unwrap();
        assert!(skipped.detail.contains("vzdálený root"));
        // A healthy server yields no warnings.
        let healthy = HealthFacts {
            anonymous_accounts: Some(vec![]),
            accounts_without_password: Some(vec![]),
            remote_root: Some(vec![]),
            tables_without_pk: Some(vec![]),
            myisam_tables: Some(vec![]),
            long_transactions: Some(vec![]),
            slow_query_log: Some(true),
            have_ssl: Some("YES".into()),
            max_connections: 100,
            max_used_connections: Some(5),
            uptime: 10 * 86_400,
            ..Default::default()
        };
        assert!(evaluate_health(&healthy).is_empty());
    }
    #[test]
    fn templates_produce_whitelisted_grants() {
        let account = Account {
            user: "app".into(),
            host: "%".into(),
            plugin: String::new(),
        };
        for template in Template::ALL {
            let Change::Privileges { scope, grant, .. } =
                template.change(account.clone(), "shop").unwrap()
            else {
                panic!()
            };
            for p in &grant {
                assert!(scope.privileges().contains(&p.as_str()), "{p} at {scope:?}");
            }
        }
        assert!(Template::ReadOnly.change(account, "").is_err());
    }
    #[test]
    fn sizes_and_table_warnings() {
        assert_eq!(parse_size("1G"), Some(1 << 30));
        assert_eq!(human_bytes(1536), "1.5 KiB");
        let table = TableInfo {
            engine: "MyISAM".into(),
            data_bytes: 100 << 20,
            free_bytes: 200 << 20,
            kind: "BASE TABLE".into(),
            ..Default::default()
        };
        assert_eq!(table.warnings().len(), 3);
    }
}
