//! Logical backups into ZIP, restore and verification.
//!
//! Each job runs in its own thread with its own connection, so monitoring and the UI
//! continue meanwhile. The archive holds one SQL file per database (readable by the
//! `mariadb` client too: `unzip -p backup.zip shop.sql | mariadb`) and a manifest with
//! SHA-256 checksums.
use crate::*;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{mysql::MySqlConnectOptions, Connection, MySqlConnection, Row};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime},
};
use zip::{write::SimpleFileOptions, AesMode, CompressionMethod, ZipArchive, ZipWriter};

pub const BACKUP_FORMAT: &str = "harbor-backup";
pub const BACKUP_FORMAT_VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
/// Rows are grouped into INSERT statements of about this size.
const INSERT_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub harbor_version: String,
    pub server_version: String,
    /// "2026-11-01T14:03:00Z"
    pub created_utc: String,
    pub profile: String,
    /// SQL files are AES-256 encrypted (the manifest is not, so it can be listed).
    pub encrypted: bool,
    /// Transactional tables were read in one consistent snapshot.
    pub consistent: bool,
    pub databases: Vec<ManifestDatabase>,
    #[serde(default)]
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestDatabase {
    pub name: String,
    pub file: String,
    pub charset: String,
    pub collation: String,
    pub sha256: String,
    /// Uncompressed size of the SQL file.
    pub bytes: u64,
    pub tables: u64,
    pub rows: u64,
    /// Tables outside the consistent snapshot (MyISAM, Aria…).
    #[serde(default)]
    pub non_transactional: Vec<String>,
}
#[derive(Clone)]
pub struct BackupRequest {
    pub profile: String,
    pub databases: Vec<String>,
    pub path: PathBuf,
    /// Encrypts the SQL files with AES-256 when set.
    pub encryption: Option<Secret>,
}
#[derive(Clone)]
pub struct RestoreRequest {
    pub path: PathBuf,
    pub encryption: Option<Secret>,
    /// Database name inside the archive.
    pub database: String,
    /// Name to restore into (may differ from the original).
    pub target: String,
    /// Drop an existing target database first.
    pub replace: bool,
}
#[derive(Clone)]
pub enum JobSpec {
    Backup(BackupRequest),
    Restore(RestoreRequest),
    Verify {
        path: PathBuf,
        encryption: Option<Secret>,
    },
}
impl JobSpec {
    pub fn title(&self) -> String {
        match self {
            Self::Backup(r) => format!(
                "Záloha {} → {}",
                r.databases.join(", "),
                file_label(&r.path)
            ),
            Self::Restore(r) => format!(
                "Obnova {} z {} do {}{}",
                r.database,
                file_label(&r.path),
                r.target,
                if r.replace { " (nahrazení)" } else { "" }
            ),
            Self::Verify { path, .. } => format!("Ověření {}", file_label(path)),
        }
    }
    pub fn needs_server(&self) -> bool {
        !matches!(self, Self::Verify { .. })
    }
    pub fn is_restore(&self) -> bool {
        matches!(self, Self::Restore(_))
    }
}
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub phase: String,
    pub done: u64,
    /// 0 = unknown.
    pub total: u64,
    /// Ok(summary) or Err(message) once finished.
    pub result: Option<std::result::Result<String, String>>,
}
impl Progress {
    pub fn finished(&self) -> bool {
        self.result.is_some()
    }
    /// 0.0–1.0; estimates (row counts) never show 100 % before the end.
    pub fn fraction(&self) -> f32 {
        if self.finished() {
            1.0
        } else if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).min(0.99)
        }
    }
}
/// A running or finished background job; cheap to clone (shared state).
#[derive(Clone)]
pub struct Job {
    pub id: u64,
    pub title: String,
    pub restore: bool,
    pub started: Instant,
    state: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    reported: Arc<AtomicBool>,
}
impl Job {
    pub fn progress(&self) -> Progress {
        self.state.lock().map(|p| p.clone()).unwrap_or_default()
    }
    pub fn finished(&self) -> bool {
        self.progress().finished()
    }
    /// Asks the job to stop at the next safe point (between rows / statements).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    /// True exactly once after the job finished (for one-time notifications).
    pub fn take_finished_report(&self) -> bool {
        self.finished() && !self.reported.swap(true, Ordering::Relaxed)
    }
    /// Blocks until the job ends; returns its result.
    pub fn wait(&self, mut on_progress: impl FnMut(&Progress)) -> Result<String> {
        loop {
            let progress = self.progress();
            on_progress(&progress);
            if let Some(result) = progress.result {
                return result.map_err(Error::Job);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}
struct Reporter {
    state: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}
impl Reporter {
    fn update(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.state.lock() {
            f(&mut p);
        }
    }
    fn phase(&self, phase: impl Into<String>) {
        let phase = phase.into();
        self.update(|p| p.phase = phase);
    }
    fn total(&self, total: u64) {
        self.update(|p| {
            p.total = total;
            p.done = 0;
        });
    }
    fn advance(&self, n: u64) {
        self.update(|p| p.done += n);
    }
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
/// Starts a job in its own thread. `options` is required for backup and restore.
pub(crate) fn spawn(spec: JobSpec, options: Option<MySqlConnectOptions>) -> Job {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let job = Job {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        title: spec.title(),
        restore: spec.is_restore(),
        started: Instant::now(),
        state: Arc::new(Mutex::new(Progress {
            phase: "Spouštím…".into(),
            ..Default::default()
        })),
        cancel: Arc::new(AtomicBool::new(false)),
        reported: Arc::new(AtomicBool::new(false)),
    };
    let reporter = Reporter {
        state: Arc::clone(&job.state),
        cancel: Arc::clone(&job.cancel),
    };
    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run(spec, options, &reporter)),
            Err(_) => Err(Error::Worker),
        };
        reporter.update(|p| {
            p.result = Some(result.map_err(message));
            p.phase = if p.result.as_ref().is_some_and(|r| r.is_ok()) {
                "Hotovo".into()
            } else {
                "Skončilo chybou".into()
            };
        });
    });
    job
}
/// Starts a job for a profile outside the UI session (command line, timers).
pub fn start_job(spec: JobSpec, profile: Option<(&Profile, Secret)>) -> Result<Job> {
    let options = match profile {
        Some((profile, password)) => Some(crate::backend::connect_options(profile, &password)?),
        None if spec.needs_server() => {
            return Err(Error::Validation(
                "Úloha vyžaduje připojení k serveru.".into(),
            ))
        }
        None => None,
    };
    Ok(spawn(spec, options))
}
async fn run(spec: JobSpec, options: Option<MySqlConnectOptions>, r: &Reporter) -> Result<String> {
    let connect = || async {
        let options = options
            .clone()
            .ok_or_else(|| Error::Validation("Úloha vyžaduje připojení k serveru.".into()))?;
        r.phase("Připojuji…");
        let mut conn = MySqlConnection::connect_with(&options).await?;
        let version: String = sqlx::query_scalar("SELECT VERSION()")
            .fetch_one(&mut conn)
            .await?;
        if !version.contains("MariaDB") {
            return Err(Error::Unsupported(
                "Tento klient je určený pro MariaDB.".into(),
            ));
        }
        Ok::<_, Error>((conn, version))
    };
    match spec {
        JobSpec::Backup(request) => {
            let (mut conn, version) = connect().await?;
            let part = partial_path(&request.path);
            let result = backup(&mut conn, &version, &request, &part, r).await;
            if result.is_err() {
                let _ = fs::remove_file(&part);
            }
            result
        }
        JobSpec::Restore(request) => {
            // Checksum first: a damaged archive must not leave a half-restored database.
            let manifest = read_manifest(&request.path)?;
            let entry = manifest
                .databases
                .iter()
                .find(|d| d.name == request.database)
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "Záloha neobsahuje databázi {}.",
                        safe_text(&request.database)
                    ))
                })?
                .clone();
            verify_entry(&request.path, &manifest, &entry, &request.encryption, r)?;
            let (mut conn, _) = connect().await?;
            restore(&mut conn, &request, &entry, r).await
        }
        JobSpec::Verify { path, encryption } => verify(&path, &encryption, r),
    }
}
fn partial_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}
fn file_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::File(format!("{context}: {error}"))
}
pub(crate) fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}
fn create_database_line(name: &str, charset: &str, collation: &str) -> String {
    format!(
        "CREATE DATABASE IF NOT EXISTS {} CHARACTER SET {} COLLATE {}",
        quote_ident(name),
        quote_ident(charset),
        quote_ident(collation)
    )
}
fn use_line(name: &str) -> String {
    format!("USE {}", quote_ident(name))
}
fn text(row: &sqlx::mysql::MySqlRow, index: usize) -> Result<Option<String>> {
    if let Ok(value) = row.try_get::<Option<String>, _>(index) {
        return Ok(value);
    }
    let bytes: Option<Vec<u8>> = row.try_get(index)?;
    Ok(bytes.map(|b| String::from_utf8_lossy(&b).into_owned()))
}
/// One SQL file inside the archive: written through a SHA-256 hasher.
struct Entry<'a> {
    zip: &'a mut ZipWriter<fs::File>,
    hash: Sha256,
    bytes: u64,
}
impl Entry<'_> {
    fn put(&mut self, text: &str) -> Result<()> {
        self.hash.update(text.as_bytes());
        self.bytes += text.len() as u64;
        self.zip
            .write_all(text.as_bytes())
            .map_err(|e| file_error("zápis zálohy", e))
    }
}
async fn backup(
    conn: &mut MySqlConnection,
    version: &str,
    request: &BackupRequest,
    part: &Path,
    r: &Reporter,
) -> Result<String> {
    if request.databases.is_empty() {
        return Err(Error::Validation("Vyberte alespoň jednu databázi.".into()));
    }
    if request.path.exists() {
        return Err(Error::Validation(format!(
            "Soubor {} už existuje.",
            request.path.display()
        )));
    }
    if request
        .encryption
        .as_ref()
        .is_some_and(|p| p.0.chars().count() < 8)
    {
        return Err(Error::Validation(
            "Heslo pro šifrování zálohy musí mít alespoň 8 znaků.".into(),
        ));
    }
    // Minimal SQL mode: SHOW CREATE output stays portable (no ANSI_QUOTES etc.).
    for statement in [
        "SET SESSION sql_mode = 'NO_AUTO_VALUE_ON_ZERO'",
        "SET SESSION time_zone = '+00:00'",
        "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "START TRANSACTION WITH CONSISTENT SNAPSHOT",
    ] {
        sqlx::raw_sql(statement).execute(&mut *conn).await?;
    }
    let mut databases = Vec::new();
    for name in &request.databases {
        let found: Option<(String, String)> = sqlx::query_as(
            "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?",
        )
        .bind(name)
        .fetch_optional(&mut *conn)
        .await?;
        let (charset, collation) = found.ok_or_else(|| {
            Error::Validation(format!(
                "Databáze {} neexistuje nebo k ní účet nemá přístup.",
                safe_text(name)
            ))
        })?;
        if ["information_schema", "performance_schema"].contains(&name.as_str()) {
            return Err(Error::Validation(format!(
                "{name} je virtuální databáze serveru; nezálohuje se."
            )));
        }
        databases.push((name.clone(), charset, collation));
    }
    r.phase("Odhaduji rozsah…");
    let mut total = 0u64;
    for (name, ..) in &databases {
        let rows: u64 = sqlx::query_scalar(
            "SELECT CAST(COALESCE(SUM(TABLE_ROWS),0) AS UNSIGNED) FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_TYPE<>'VIEW'",
        )
        .bind(name)
        .fetch_one(&mut *conn)
        .await?;
        total += rows;
    }
    r.total(total.max(1));
    if let Some(parent) = request.path.parent() {
        fs::create_dir_all(parent).map_err(|e| file_error("adresář záloh", e))?;
    }
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        // Backups contain all data: readable by the owner only.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(part)
            .map_err(|e| file_error("vytvoření souboru zálohy", e))?
    };
    let mut zip = ZipWriter::new(file);
    let password = request.encryption.clone();
    let options = || {
        let base = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o600)
            .large_file(true);
        match &password {
            Some(secret) => base.with_aes_encryption(AesMode::Aes256, secret.0.as_str()),
            None => base,
        }
    };
    let mut manifest = Manifest {
        format: BACKUP_FORMAT.into(),
        format_version: BACKUP_FORMAT_VERSION,
        harbor_version: env!("CARGO_PKG_VERSION").into(),
        server_version: version.into(),
        created_utc: utc_timestamp(SystemTime::now()),
        profile: request.profile.clone(),
        encrypted: password.is_some(),
        consistent: true,
        databases: Vec::new(),
        warnings: Vec::new(),
    };
    for (name, charset, collation) in &databases {
        r.check()?;
        let file_name = format!("{}.sql", archive_name(name));
        zip.start_file(file_name.as_str(), options())
            .map_err(|e| file_error("zápis zálohy", e))?;
        let mut entry = Entry {
            zip: &mut zip,
            hash: Sha256::new(),
            bytes: 0,
        };
        let stats = dump_database(conn, &mut entry, name, charset, collation, &manifest, r).await?;
        let (tables, rows, non_transactional, warnings) = stats;
        if !non_transactional.is_empty() {
            manifest.consistent = false;
        }
        manifest.warnings.extend(warnings);
        manifest.databases.push(ManifestDatabase {
            name: name.clone(),
            file: file_name,
            charset: charset.clone(),
            collation: collation.clone(),
            sha256: hex(&entry.hash.finalize()),
            bytes: entry.bytes,
            tables,
            rows,
            non_transactional,
        });
    }
    sqlx::raw_sql("COMMIT").execute(&mut *conn).await?;
    r.phase("Dokončuji archiv…");
    zip.start_file(
        MANIFEST,
        SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o600),
    )
    .map_err(|e| file_error("zápis zálohy", e))?;
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| file_error("manifest", e))?;
    zip.write_all(&json)
        .map_err(|e| file_error("zápis zálohy", e))?;
    let file = zip.finish().map_err(|e| file_error("zápis zálohy", e))?;
    file.sync_all()
        .map_err(|e| file_error("uložení zálohy na disk", e))?;
    drop(file);
    fs::rename(part, &request.path).map_err(|e| file_error("přejmenování zálohy", e))?;
    let size = fs::metadata(&request.path).map(|m| m.len()).unwrap_or(0);
    let mut summary = format!(
        "Záloha uložena: {} ({}, {} databází, {} řádků).",
        request.path.display(),
        human_bytes(size),
        manifest.databases.len(),
        manifest.databases.iter().map(|d| d.rows).sum::<u64>()
    );
    if !manifest.consistent {
        summary.push_str(" Netransakční tabulky nejsou v konzistentním snímku.");
    }
    if !manifest.warnings.is_empty() {
        summary.push_str(&format!(" Upozornění: {}", manifest.warnings.join("; ")));
    }
    Ok(summary)
}
/// File name inside the archive: unique and safe for any database name.
fn archive_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe == name {
        safe
    } else {
        // Different names must not collide after sanitizing.
        format!("{safe}-{}", &hex(&Sha256::digest(name.as_bytes()))[..8])
    }
}
type DumpStats = (u64, u64, Vec<String>, Vec<String>);
async fn dump_database(
    conn: &mut MySqlConnection,
    out: &mut Entry<'_>,
    name: &str,
    charset: &str,
    collation: &str,
    manifest: &Manifest,
    r: &Reporter,
) -> Result<DumpStats> {
    r.phase(format!("Zálohuji {}", safe_text(name)));
    let db = quote_ident(name);
    // With the database selected, SHOW CREATE omits its name in views and foreign
    // keys, so the dump can be restored under another name.
    sqlx::raw_sql(&use_line(name)).execute(&mut *conn).await?;
    out.put(&format!(
        "-- Harbor {} • záloha databáze {} • server {} • {}\n\
         -- Obnova: Harbor, nebo `unzip -p záloha.zip soubor.sql | mariadb`\n\
         /*!40101 SET NAMES utf8mb4 */;\n\
         /*!40103 SET TIME_ZONE='+00:00' */;\n\
         /*!40014 SET FOREIGN_KEY_CHECKS=0 */;\n\
         /*!40014 SET UNIQUE_CHECKS=0 */;\n\
         /*!40101 SET SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;\n\
         /*!40111 SET SQL_NOTES=0 */;\n\
         {};\n{};\n",
        manifest.harbor_version,
        name.replace('\n', " "),
        manifest.server_version.replace('\n', " "),
        manifest.created_utc,
        create_database_line(name, charset, collation),
        use_line(name)
    ))?;
    let objects: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT TABLE_NAME, TABLE_TYPE, COALESCE(ENGINE,'') FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME",
    )
    .bind(name)
    .fetch_all(&mut *conn)
    .await?;
    let mut warnings = Vec::new();
    let mut non_transactional = Vec::new();
    let (mut tables, mut rows) = (0u64, 0u64);
    for (table, kind, engine) in objects.iter().filter(|(_, k, _)| k != "VIEW") {
        r.check()?;
        let qualified = format!("{db}.{}", quote_ident(table));
        if kind == "SEQUENCE" {
            let create = show_create(conn, &format!("SHOW CREATE SEQUENCE {qualified}"), 1).await?;
            let rows = sqlx::raw_sql(&format!("SELECT next_not_cached_value FROM {qualified}"))
                .fetch_all(&mut *conn)
                .await?;
            let next: Option<i64> = match rows.first() {
                Some(row) => Some(row.try_get(0)?),
                None => None,
            };
            out.put(&format!(
                "\n-- Sekvence {t}\nDROP SEQUENCE IF EXISTS {t};\n{create};\n",
                t = quote_ident(table)
            ))?;
            if let Some(next) = next {
                out.put(&format!("DO SETVAL({}, {next}, 0);\n", quote_ident(table)))?;
            }
            continue;
        }
        tables += 1;
        let create = show_create(conn, &format!("SHOW CREATE TABLE {qualified}"), 1).await?;
        out.put(&format!(
            "\n-- Tabulka {t}\nDROP TABLE IF EXISTS {t};\n{create};\n",
            t = quote_ident(table)
        ))?;
        if !matches!(engine.as_str(), "InnoDB" | "") {
            non_transactional.push(table.clone());
        }
        rows += dump_rows(conn, out, name, table, r).await?;
    }
    for (view, ..) in objects.iter().filter(|(_, k, _)| k == "VIEW") {
        r.check()?;
        match show_create(
            conn,
            &format!("SHOW CREATE VIEW {db}.{}", quote_ident(view)),
            1,
        )
        .await
        {
            Ok(create) => out.put(&format!(
                "\n-- Pohled {v}\nDROP TABLE IF EXISTS {v};\nDROP VIEW IF EXISTS {v};\n{create};\n",
                v = quote_ident(view)
            ))?,
            Err(error) => warnings.push(format!("pohled {name}.{view} vynechán: {error}")),
        }
    }
    let routines: Vec<(String, String)> = sqlx::query_as(
        "SELECT ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA=? ORDER BY ROUTINE_TYPE, ROUTINE_NAME",
    )
    .bind(name)
    .fetch_all(&mut *conn)
    .await?;
    for (routine, kind) in routines {
        let kind = if kind == "FUNCTION" {
            "FUNCTION"
        } else {
            "PROCEDURE"
        };
        let sql = format!("SHOW CREATE {kind} {db}.{}", quote_ident(&routine));
        match show_create(conn, &sql, 2).await {
            Ok(create) => out.put(&format!(
                "\n-- {kind} {r}\nDROP {kind} IF EXISTS {r};\nDELIMITER ;;\n{create};;\nDELIMITER ;\n",
                r = quote_ident(&routine)
            ))?,
            Err(error) => warnings.push(format!("{name}.{routine} vynecháno: {error}")),
        }
    }
    let triggers: Vec<String> = sqlx::query_scalar(
        "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA=? ORDER BY EVENT_OBJECT_TABLE, ACTION_ORDER",
    )
    .bind(name)
    .fetch_all(&mut *conn)
    .await?;
    for trigger in triggers {
        let sql = format!("SHOW CREATE TRIGGER {db}.{}", quote_ident(&trigger));
        match show_create(conn, &sql, 2).await {
            Ok(create) => out.put(&format!(
                "\n-- Trigger {t}\nDROP TRIGGER IF EXISTS {t};\nDELIMITER ;;\n{create};;\nDELIMITER ;\n",
                t = quote_ident(&trigger)
            ))?,
            Err(error) => warnings.push(format!("trigger {name}.{trigger} vynechán: {error}")),
        }
    }
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA=? ORDER BY EVENT_NAME",
    )
    .bind(name)
    .fetch_all(&mut *conn)
    .await?;
    for event in events {
        let sql = format!("SHOW CREATE EVENT {db}.{}", quote_ident(&event));
        match show_create(conn, &sql, 3).await {
            Ok(create) => out.put(&format!(
                "\n-- Událost {e}\nDROP EVENT IF EXISTS {e};\nDELIMITER ;;\n{create};;\nDELIMITER ;\n",
                e = quote_ident(&event)
            ))?,
            Err(error) => warnings.push(format!("událost {name}.{event} vynechána: {error}")),
        }
    }
    out.put("\n/*!40014 SET FOREIGN_KEY_CHECKS=1 */;\n/*!40014 SET UNIQUE_CHECKS=1 */;\n")?;
    Ok((tables, rows, non_transactional, warnings))
}
async fn show_create(conn: &mut MySqlConnection, sql: &str, column: usize) -> Result<String> {
    let row = sqlx::raw_sql(sql).fetch_one(&mut *conn).await?;
    text(&row, column)?.ok_or(Error::Permission)
}
/// Streams a table as INSERT statements. The server renders each row as one SQL tuple
/// (QUOTE for text and numbers, hex for binary data), so no type is decoded client-side.
async fn dump_rows(
    conn: &mut MySqlConnection,
    out: &mut Entry<'_>,
    database: &str,
    table: &str,
    r: &Reporter,
) -> Result<u64> {
    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT COLUMN_NAME, DATA_TYPE FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND IS_GENERATED='NEVER' ORDER BY ORDINAL_POSITION",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await?;
    if columns.is_empty() {
        return Ok(0);
    }
    let expressions: Vec<String> = columns
        .iter()
        .map(|(name, data_type)| value_expression(&quote_ident(name), data_type))
        .collect();
    let list = columns
        .iter()
        .map(|(name, _)| quote_ident(name))
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT CONCAT('(', {}, ')') FROM {}.{}",
        expressions.join(", ',', "),
        quote_ident(database),
        quote_ident(table)
    );
    let insert = format!("INSERT INTO {} ({list}) VALUES\n", quote_ident(table));
    let mut batch = String::new();
    let mut count = 0u64;
    let mut since_check = 0u64;
    let mut stream = sqlx::raw_sql(&query).fetch(&mut *conn);
    while let Some(row) = stream.try_next().await? {
        let tuple = text(&row, 0)?.unwrap_or_default();
        if batch.is_empty() {
            batch.push_str(&insert);
        } else {
            batch.push_str(",\n");
        }
        batch.push_str(&tuple);
        count += 1;
        since_check += 1;
        if batch.len() >= INSERT_BYTES {
            batch.push_str(";\n");
            out.put(&batch)?;
            batch.clear();
        }
        if since_check >= 500 {
            r.advance(since_check);
            since_check = 0;
            r.check()?;
        }
    }
    if !batch.is_empty() {
        batch.push_str(";\n");
        out.put(&batch)?;
    }
    r.advance(since_check);
    Ok(count)
}
/// SQL expression producing the literal of one column value.
fn value_expression(column: &str, data_type: &str) -> String {
    match data_type.to_ascii_lowercase().as_str() {
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" | "geometry"
        | "point" | "linestring" | "polygon" | "multipoint" | "multilinestring"
        | "multipolygon" | "geometrycollection" => format!(
            "IF({column} IS NULL, 'NULL', IF(OCTET_LENGTH({column}) = 0, '''''', CONCAT('0x', HEX({column}))))"
        ),
        "bit" => format!("IF({column} IS NULL, 'NULL', CONCAT('b''', BIN({column}), ''''))"),
        _ => format!("QUOTE(CONVERT({column} USING utf8mb4))"),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- reading archives

pub fn read_manifest(path: &Path) -> Result<Manifest> {
    let file = fs::File::open(path).map_err(|e| file_error("otevření zálohy", e))?;
    let mut archive = ZipArchive::new(file).map_err(|e| file_error("čtení archivu", e))?;
    let mut entry = archive
        .by_name(MANIFEST)
        .map_err(|_| Error::File("archiv není záloha Harboru (chybí manifest).".into()))?;
    let mut json = String::new();
    entry
        .read_to_string(&mut json)
        .map_err(|e| file_error("čtení manifestu", e))?;
    let manifest: Manifest =
        serde_json::from_str(&json).map_err(|e| file_error("neplatný manifest", e))?;
    if manifest.format != BACKUP_FORMAT || manifest.format_version > BACKUP_FORMAT_VERSION {
        return Err(Error::File(format!(
            "nepodporovaný formát zálohy {} v{}.",
            safe_text(&manifest.format),
            manifest.format_version
        )));
    }
    Ok(manifest)
}
/// Opens the SQL file of one database (decrypting when needed) and passes a reader.
fn with_entry<T>(
    path: &Path,
    manifest: &Manifest,
    entry: &ManifestDatabase,
    encryption: &Option<Secret>,
    f: impl FnOnce(&mut dyn Read) -> Result<T>,
) -> Result<T> {
    let file = fs::File::open(path).map_err(|e| file_error("otevření zálohy", e))?;
    let mut archive = ZipArchive::new(file).map_err(|e| file_error("čtení archivu", e))?;
    let mut reader = if manifest.encrypted {
        let password = encryption
            .as_ref()
            .filter(|p| !p.0.is_empty())
            .ok_or_else(|| Error::Validation("Záloha je šifrovaná; zadejte heslo.".into()))?;
        archive
            .by_name_decrypt(&entry.file, password.0.as_bytes())
            .map_err(|e| match e {
                zip::result::ZipError::InvalidPassword => {
                    Error::Validation("Nesprávné heslo zálohy.".into())
                }
                e => file_error("čtení archivu", e),
            })?
    } else {
        archive
            .by_name(&entry.file)
            .map_err(|e| file_error("čtení archivu", e))?
    };
    f(&mut reader)
}
/// Recomputes the checksum of one database file and parses it into statements.
fn verify_entry(
    path: &Path,
    manifest: &Manifest,
    entry: &ManifestDatabase,
    encryption: &Option<Secret>,
    r: &Reporter,
) -> Result<u64> {
    r.phase(format!("Ověřuji {}", safe_text(&entry.name)));
    r.total(entry.bytes.max(1));
    with_entry(path, manifest, entry, encryption, |reader| {
        let mut hashing = HashingReader {
            inner: reader,
            hash: Sha256::new(),
            bytes: 0,
        };
        let mut statements = Statements::new(BufReader::new(&mut hashing));
        let mut count = 0u64;
        let mut last = 0u64;
        while let Some(statement) = statements.next_statement()? {
            drop(statement);
            count += 1;
            if count % 200 == 0 {
                r.check()?;
                let consumed = statements.consumed;
                r.advance(consumed - last);
                last = consumed;
            }
        }
        drop(statements);
        if hashing.bytes != entry.bytes || hex(&hashing.hash.finalize()) != entry.sha256 {
            return Err(Error::File(format!(
                "kontrolní součet souboru {} nesouhlasí — záloha je poškozená.",
                safe_text(&entry.file)
            )));
        }
        Ok(count)
    })
}
struct HashingReader<'a> {
    inner: &'a mut dyn Read,
    hash: Sha256,
    bytes: u64,
}
impl Read for HashingReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}
fn verify(path: &Path, encryption: &Option<Secret>, r: &Reporter) -> Result<String> {
    let manifest = read_manifest(path)?;
    let mut statements = 0;
    for entry in &manifest.databases {
        statements += verify_entry(path, &manifest, entry, encryption, r)?;
    }
    Ok(format!(
        "Záloha je v pořádku: {} databází, {} příkazů, kontrolní součty souhlasí (vytvořena {}, server {}).",
        manifest.databases.len(),
        statements,
        manifest.created_utc,
        safe_text(&manifest.server_version)
    ))
}
async fn restore(
    conn: &mut MySqlConnection,
    request: &RestoreRequest,
    entry: &ManifestDatabase,
    r: &Reporter,
) -> Result<String> {
    identifier(&request.target)?;
    if Database::system_name(&request.target) {
        return Err(Error::Validation(
            "Do systémové databáze Harbor neobnovuje.".into(),
        ));
    }
    let exists: Option<String> = sqlx::query_scalar(
        "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME=?",
    )
    .bind(&request.target)
    .fetch_optional(&mut *conn)
    .await?;
    if exists.is_some() && !request.replace {
        return Err(Error::Validation(format!(
            "Databáze {} už existuje; zvolte jiný název, nebo výslovně nahrazení.",
            safe_text(&request.target)
        )));
    }
    let target = quote_ident(&request.target);
    if exists.is_some() {
        r.phase(format!("Mažu původní {}", safe_text(&request.target)));
        sqlx::raw_sql(&format!("DROP DATABASE {target}"))
            .execute(&mut *conn)
            .await?;
    }
    sqlx::raw_sql(&create_database_line(
        &request.target,
        &entry.charset,
        &entry.collation,
    ))
    .execute(&mut *conn)
    .await?;
    sqlx::raw_sql(&use_line(&request.target))
        .execute(&mut *conn)
        .await?;
    let original_create = create_database_line(&entry.name, &entry.charset, &entry.collation);
    let original_use = use_line(&entry.name);
    r.phase(format!("Obnovuji {}", safe_text(&request.target)));
    r.total(entry.bytes.max(1));
    let manifest = read_manifest(&request.path)?;
    // Statements are read synchronously (zip) in a helper thread and executed
    // asynchronously on `conn`.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<(String, u64)>>(16);
    let path = request.path.clone();
    let encryption = request.encryption.clone();
    let entry_owned = entry.clone();
    let reader = std::thread::spawn(move || {
        let result = with_entry(&path, &manifest, &entry_owned, &encryption, |reader| {
            let mut statements = Statements::new(BufReader::new(reader));
            while let Some(statement) = statements.next_statement()? {
                if tx
                    .blocking_send(Ok((statement, statements.consumed)))
                    .is_err()
                {
                    break;
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            let _ = tx.blocking_send(Err(error));
        }
    });
    let mut executed = 0u64;
    let mut deferred: Vec<String> = Vec::new();
    let mut last = 0u64;
    let mut failure = None;
    while let Some(item) = rx.recv().await {
        let (statement, consumed) = match item {
            Ok(item) => item,
            Err(error) => {
                failure = Some(error);
                break;
            }
        };
        if let Err(error) = r.check() {
            failure = Some(error);
            break;
        }
        r.advance(consumed - last);
        last = consumed;
        let trimmed = statement.trim();
        if trimmed == original_create || trimmed == original_use {
            continue;
        }
        executed += 1;
        if let Err(error) = sqlx::raw_sql(trimmed).execute(&mut *conn).await {
            // A view may reference a view that comes later alphabetically.
            if is_view(trimmed) && db_error(&error) == Some(1146) {
                deferred.push(trimmed.to_string());
                continue;
            }
            failure = Some(statement_error(executed, trimmed, error));
            break;
        }
    }
    drop(rx);
    let _ = reader.join();
    if let Some(error) = failure {
        return Err(match error {
            Error::Cancelled => Error::Job(format!(
                "Obnova zrušena; databáze {} zůstala částečně obnovená.",
                safe_text(&request.target)
            )),
            error => Error::Job(format!(
                "{} Databáze {} zůstala částečně obnovená.",
                message(error),
                safe_text(&request.target)
            )),
        });
    }
    for _ in 0..5 {
        if deferred.is_empty() {
            break;
        }
        let mut still = Vec::new();
        for statement in deferred {
            if let Err(error) = sqlx::raw_sql(&statement).execute(&mut *conn).await {
                if db_error(&error) == Some(1146) {
                    still.push(statement);
                    continue;
                }
                return Err(statement_error(executed, &statement, error));
            }
        }
        deferred = still;
    }
    if let Some(statement) = deferred.first() {
        return Err(Error::File(format!(
            "pohled nelze vytvořit, chybí tabulka, na kterou odkazuje: {}",
            clip(statement, 120)
        )));
    }
    Ok(format!(
        "Obnoveno do databáze {}: {} příkazů, {} tabulek, ≈ {} řádků.",
        safe_text(&request.target),
        executed,
        entry.tables,
        entry.rows
    ))
}
/// Error text without the variant prefix where it would repeat.
fn message(error: Error) -> String {
    match error {
        Error::File(text) => format!("Soubor: {text}"),
        Error::Job(text) => text,
        error => error.to_string(),
    }
}
fn is_view(statement: &str) -> bool {
    let upper = statement
        .chars()
        .take(200)
        .collect::<String>()
        .to_ascii_uppercase();
    upper.starts_with("CREATE") && upper.contains(" VIEW ")
}
fn db_error(error: &sqlx::Error) -> Option<u16> {
    match error {
        sqlx::Error::Database(e) => e
            .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
            .map(|e| e.number()),
        _ => None,
    }
}
fn statement_error(number: u64, statement: &str, error: sqlx::Error) -> Error {
    Error::Job(format!(
        "příkaz č. {number} selhal ({}): {}",
        Error::from(error),
        clip(statement, 120)
    ))
}
fn clip(text: &str, max: usize) -> String {
    let text = safe_text(&text.replace('\n', " "));
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text
    }
}

// ---------------------------------------------------------------- statement splitter

/// Splits an SQL dump into statements like the `mariadb` client: understands
/// `DELIMITER`, quotes with backslash escapes, `--`/`#` and block comments.
pub struct Statements<R: BufRead> {
    reader: R,
    delimiter: String,
    /// Bytes read so far (progress).
    pub consumed: u64,
    current: String,
    meaningful: bool,
    quote: Option<char>,
    block_comment: bool,
    pending: std::collections::VecDeque<String>,
    finished: bool,
}
impl<R: BufRead> Statements<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            delimiter: ";".into(),
            consumed: 0,
            current: String::new(),
            meaningful: false,
            quote: None,
            block_comment: false,
            pending: Default::default(),
            finished: false,
        }
    }
    pub fn next_statement(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(statement) = self.pending.pop_front() {
                return Ok(Some(statement));
            }
            if self.finished {
                return Ok(None);
            }
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| file_error("čtení zálohy", e))?;
            self.consumed += n as u64;
            if n == 0 {
                self.finished = true;
                if self.quote.is_some() {
                    return Err(Error::File(
                        "záloha končí uprostřed textového řetězce — je neúplná.".into(),
                    ));
                }
                if self.meaningful {
                    return Err(Error::File(
                        "záloha končí neukončeným příkazem — je neúplná.".into(),
                    ));
                }
                continue;
            }
            self.feed(&line);
        }
    }
    fn emit(&mut self) {
        let statement = std::mem::take(&mut self.current);
        if self.meaningful {
            self.pending.push_back(statement.trim().to_string());
        }
        self.meaningful = false;
    }
    fn feed(&mut self, line: &str) {
        if self.quote.is_none() && !self.block_comment && !self.meaningful {
            let trimmed = line.trim_start();
            if trimmed
                .get(..10)
                .is_some_and(|word| word.eq_ignore_ascii_case("DELIMITER "))
            {
                let delimiter = trimmed[10..].trim();
                if !delimiter.is_empty() {
                    self.delimiter = delimiter.to_string();
                }
                self.current.clear();
                return;
            }
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < line.len() {
            let rest = &line[i..];
            let c = rest.chars().next().expect("non-empty");
            let width = c.len_utf8();
            if self.block_comment {
                if rest.starts_with("*/") {
                    self.current.push_str("*/");
                    self.block_comment = false;
                    i += 2;
                } else {
                    self.current.push(c);
                    i += width;
                }
                continue;
            }
            if let Some(quote) = self.quote {
                self.current.push(c);
                i += width;
                if c == '\\' && quote != '`' {
                    if let Some(next) = line[i..].chars().next() {
                        self.current.push(next);
                        i += next.len_utf8();
                    }
                } else if c == quote {
                    if line[i..].starts_with(quote) {
                        self.current.push(quote);
                        i += quote.len_utf8();
                    } else {
                        self.quote = None;
                    }
                }
                continue;
            }
            if rest.starts_with(&self.delimiter) {
                i += self.delimiter.len();
                self.emit();
                continue;
            }
            if c == '#'
                || (rest.starts_with("--")
                    && bytes.get(i + 2).is_none_or(|b| b.is_ascii_whitespace()))
            {
                // Comment to the end of the line (kept out of the statement).
                self.current.push('\n');
                break;
            }
            if rest.starts_with("/*") {
                // Executable comments (/*! … */) are statements for the server.
                if rest.starts_with("/*!") || rest.starts_with("/*M!") {
                    self.meaningful = true;
                }
                self.block_comment = true;
                self.current.push_str("/*");
                i += 2;
                continue;
            }
            if matches!(c, '\'' | '"' | '`') {
                self.quote = Some(c);
                self.meaningful = true;
            } else if !c.is_whitespace() {
                self.meaningful = true;
            }
            self.current.push(c);
            i += width;
        }
    }
}

// ---------------------------------------------------------------- backup files

#[derive(Clone, Debug)]
pub struct BackupFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    pub manifest: std::result::Result<Manifest, String>,
}
impl BackupFile {
    pub fn name(&self) -> String {
        file_label(&self.path)
    }
}
/// `$XDG_DATA_HOME/harbor/backups`, or `~/.local/share/harbor/backups`.
pub fn default_backup_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("harbor/backups")
}
/// Backup archives in a directory, newest first.
pub fn list_backups(dir: &Path) -> Vec<BackupFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<BackupFile> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "zip"))
        .map(|e| {
            let metadata = e.metadata().ok();
            BackupFile {
                path: e.path(),
                bytes: metadata.as_ref().map_or(0, |m| m.len()),
                modified: metadata.and_then(|m| m.modified().ok()),
                manifest: read_manifest(&e.path()).map_err(|e| e.to_string()),
            }
        })
        .collect();
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.path.cmp(&a.path)));
    files
}
/// Lowercase ASCII slug of a profile name for file and unit names.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "server".into()
    } else {
        out
    }
}
/// `<profil>-YYYYMMDD-HHMMSS.zip` in UTC.
pub fn backup_file_name(profile: &str, at: SystemTime) -> String {
    let stamp = utc_timestamp(at);
    let compact: String = stamp
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .collect::<String>()
        .replace('T', "-");
    format!("{}-{compact}.zip", slug(profile))
}
/// Deletes the oldest `<slug>-YYYYMMDD-HHMMSS.zip` files beyond `keep`. Other files in
/// the directory are never touched.
pub fn prune_backups(dir: &Path, profile: &str, keep: usize) -> Result<Vec<PathBuf>> {
    let prefix = format!("{}-", slug(profile));
    let mut ours: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| file_error("adresář záloh", e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(&prefix))
                .and_then(|n| n.strip_suffix(".zip"))
                .is_some_and(|stamp| {
                    stamp.len() == 15
                        && stamp.as_bytes()[8] == b'-'
                        && stamp
                            .chars()
                            .enumerate()
                            .all(|(i, c)| i == 8 || c.is_ascii_digit())
                })
        })
        .collect();
    // The timestamp in the name sorts chronologically.
    ours.sort();
    let excess = ours.len().saturating_sub(keep.max(1));
    let removed: Vec<PathBuf> = ours.into_iter().take(excess).collect();
    for path in &removed {
        fs::remove_file(path).map_err(|e| file_error("mazání staré zálohy", e))?;
    }
    Ok(removed)
}
/// "2026-11-01T14:03:00Z" without a date library (civil-from-days).
pub fn utc_timestamp(at: SystemTime) -> String {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn split(sql: &str) -> Vec<String> {
        let mut s = Statements::new(BufReader::new(sql.as_bytes()));
        let mut out = Vec::new();
        while let Some(statement) = s.next_statement().unwrap() {
            out.push(statement);
        }
        out
    }
    #[test]
    fn splitter_handles_quotes_comments_and_delimiters() {
        let sql = "-- komentář; nic\n/*!40101 SET NAMES utf8mb4 */;\n\
            INSERT INTO t VALUES ('a;b','it''s','c\\';d'),(\"x;\");\n\
            /* jen komentář; */\n\
            # další; komentář\n\
            DELIMITER ;;\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END;;\nDELIMITER ;\n\
            SELECT `a;b` FROM t;\n";
        let statements = split(sql);
        assert_eq!(statements.len(), 4, "{statements:#?}");
        assert_eq!(statements[0], "/*!40101 SET NAMES utf8mb4 */");
        assert!(statements[1].ends_with("(\"x;\")"));
        assert_eq!(
            statements[2],
            "CREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END"
        );
        assert_eq!(statements[3], "SELECT `a;b` FROM t");
    }
    #[test]
    fn truncated_dumps_are_detected() {
        let mut s = Statements::new(BufReader::new("INSERT INTO t VALUES ('abc".as_bytes()));
        assert!(s.next_statement().is_err());
        let mut s = Statements::new(BufReader::new("SELECT 1".as_bytes()));
        assert!(s.next_statement().is_err());
    }
    #[test]
    fn names_timestamps_and_retention() {
        let at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_793_541_780);
        assert_eq!(utc_timestamp(at), "2026-11-01T14:03:00Z");
        let leap = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800);
        assert_eq!(utc_timestamp(leap), "2024-02-29T00:00:00Z");
        assert_eq!(
            utc_timestamp(SystemTime::UNIX_EPOCH),
            "1970-01-01T00:00:00Z"
        );
        assert_eq!(slug("Produkce – Eshop!"), "produkce-eshop");
        assert_eq!(backup_file_name("Prod", at), "prod-20261101-140300.zip");
        assert_eq!(archive_name("shop"), "shop");
        assert_ne!(archive_name("a/b"), archive_name("a\\b"));
        let dir = std::env::temp_dir().join(format!("harbor-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "prod-20260101-000000.zip",
            "prod-20260102-000000.zip",
            "prod-20260103-000000.zip",
            "prod-ruční.zip",
            "jiny-20250101-000000.zip",
        ] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let removed = prune_backups(&dir, "Prod", 2).unwrap();
        assert_eq!(removed, vec![dir.join("prod-20260101-000000.zip")]);
        assert!(
            dir.join("prod-ruční.zip").exists() && dir.join("jiny-20250101-000000.zip").exists()
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn binary_and_bit_columns_use_lossless_literals() {
        assert!(value_expression("`b`", "LONGBLOB").contains("HEX(`b`)"));
        assert!(value_expression("`f`", "bit").contains("BIN(`f`)"));
        assert_eq!(
            value_expression("`t`", "varchar"),
            "QUOTE(CONVERT(`t` USING utf8mb4))"
        );
    }
}
