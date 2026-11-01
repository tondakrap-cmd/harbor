//! Access to the machine running MariaDB: locally or over SSH. Used for the systemd
//! service, logs, disk usage and Harbor's own configuration file.
//!
//! Every command is a fixed program with arguments; values from the user or the server
//! are validated and, over SSH, quoted for the remote shell. SSH runs in batch mode
//! (keys / agent only) so it can never hang on a password prompt.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum HostAccess {
    #[default]
    None,
    Local,
    Ssh {
        /// `user@host` or `host`.
        target: String,
        port: u16,
    },
}
/// How administrative commands get root rights.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Elevation {
    /// Run as is (Harbor runs as root, or the user has the rights).
    #[default]
    None,
    /// `sudo -n`: needs a NOPASSWD rule for the listed commands.
    Sudo,
    /// `pkexec`: asks for the password in a desktop dialog (local only).
    Pkexec,
}
impl Elevation {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "žádné (root nebo vlastní práva)",
            Self::Sudo => "sudo bez hesla",
            Self::Pkexec => "pkexec (heslo v okně, jen lokálně)",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HostSettings {
    pub access: HostAccess,
    pub elevation: Elevation,
    /// systemd unit of the server.
    pub service: String,
    /// Directory for Harbor's option file; empty = detect.
    pub config_dir: String,
}
impl Default for HostSettings {
    fn default() -> Self {
        Self {
            access: HostAccess::None,
            elevation: Elevation::None,
            service: "mariadb".into(),
            config_dir: String::new(),
        }
    }
}
impl HostSettings {
    pub fn enabled(&self) -> bool {
        self.access != HostAccess::None
    }
    pub fn validate(&self) -> Result<()> {
        if let HostAccess::Ssh { target, port } = &self.access {
            let valid = !target.is_empty()
                && !target.starts_with('-')
                && target.len() <= 255
                && target
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._@-:[]".contains(c));
            if !valid || *port == 0 {
                return Err(Error::Validation(
                    "SSH cíl zadejte jako uživatel@hostitel (bez mezer a voleb) a platný port."
                        .into(),
                ));
            }
            if self.elevation == Elevation::Pkexec {
                return Err(Error::Validation(
                    "pkexec funguje jen lokálně; přes SSH použijte sudo.".into(),
                ));
            }
        }
        if self.enabled() {
            unit(&self.service)?;
            if !self.config_dir.is_empty() {
                path(&self.config_dir)?;
            }
        }
        Ok(())
    }
    pub fn label(&self) -> String {
        match &self.access {
            HostAccess::None => "bez přístupu k hostiteli".into(),
            HostAccess::Local => format!("lokálně • {}", self.elevation.label()),
            HostAccess::Ssh { target, port } => {
                format!("SSH {target}:{port} • {}", self.elevation.label())
            }
        }
    }
}
fn unit(name: &str) -> Result<&str> {
    if name.is_empty()
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._-".contains(c))
    {
        return Err(Error::Validation(
            "Název služby smí obsahovat jen písmena, číslice a @._-".into(),
        ));
    }
    Ok(name)
}
fn path(value: &str) -> Result<&str> {
    if !value.starts_with('/')
        || value.contains("/../")
        || value.ends_with("/..")
        || value.chars().any(char::is_control)
    {
        return Err(Error::Validation(format!(
            "Neplatná cesta {}.",
            safe_text(value)
        )));
    }
    Ok(value)
}
/// POSIX shell quoting for the remote side of SSH.
fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-=:,@+".contains(c))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}
#[derive(Debug)]
struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}
/// One command runner for local and SSH execution.
pub struct Host {
    settings: HostSettings,
}
impl Host {
    pub fn new(settings: HostSettings) -> Result<Self> {
        settings.validate()?;
        if !settings.enabled() {
            return Err(Error::Validation(
                "Profil nemá nastavený přístup k hostiteli.".into(),
            ));
        }
        Ok(Self { settings })
    }
    /// Runs `argv`; `root` adds the configured elevation.
    fn run(&self, argv: &[&str], root: bool, stdin: Option<&[u8]>) -> Result<Output> {
        let mut full: Vec<&str> = Vec::new();
        if root {
            match self.settings.elevation {
                Elevation::Sudo => full.extend(["sudo", "-n"]),
                Elevation::Pkexec => full.push("pkexec"),
                Elevation::None => (),
            }
        }
        full.extend_from_slice(argv);
        let mut command = match &self.settings.access {
            HostAccess::Local => {
                let mut command = Command::new(full[0]);
                command.args(&full[1..]);
                command
            }
            HostAccess::Ssh { target, port } => {
                let mut command = Command::new("ssh");
                command.args([
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=8",
                    "-o",
                    "ServerAliveInterval=10",
                    "-p",
                    &port.to_string(),
                ]);
                if let Ok(identity) = std::env::var("HARBOR_SSH_IDENTITY") {
                    command.args(["-i", &identity, "-o", "IdentitiesOnly=yes"]);
                }
                if let Ok(known) = std::env::var("HARBOR_SSH_KNOWN_HOSTS") {
                    command.args(["-o", &format!("UserKnownHostsFile={known}")]);
                }
                command.arg("--").arg(target).arg(
                    full.iter()
                        .map(|a| shell_quote(a))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                command
            }
            HostAccess::None => unreachable!("checked in new"),
        };
        command
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("LC_ALL", "C.UTF-8");
        let mut child = command
            .spawn()
            .map_err(|e| Error::Unsupported(format!("nelze spustit {}: {e}", full[0])))?;
        if let Some(data) = stdin {
            let mut pipe = child.stdin.take().expect("piped");
            pipe.write_all(data)
                .map_err(|e| Error::File(format!("předání dat příkazu: {e}")))?;
        }
        // Read both pipes while the command runs: large output (mariadbd --help) would
        // otherwise fill the pipe and block the command forever.
        let reader = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buffer);
                }
                buffer
            })
        };
        let stdout = reader(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        );
        let stderr = reader(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        );
        // Hard limit so a hung SSH connection cannot block the host worker forever.
        let deadline = Instant::now() + Duration::from_secs(120);
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| Error::File(e.to_string()))? {
                break status;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let result = Output {
            status: status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned(),
        };
        if matches!(self.settings.access, HostAccess::Ssh { .. }) && result.status == 255 {
            return Err(
                Error::Network.with_context(&format!("SSH: {}", first_line(&result.stderr)))
            );
        }
        Ok(result)
    }
    /// Like `run`, but a non-zero exit is an error with the command's message.
    fn check(&self, argv: &[&str], root: bool, stdin: Option<&[u8]>) -> Result<String> {
        let output = self.run(argv, root, stdin)?;
        if output.status == 0 {
            return Ok(output.stdout);
        }
        let message = first_line(&output.stderr);
        if message.contains("a password is required") || message.contains("sudo:") {
            return Err(Error::Permission.with_context(&format!(
                "sudo bez hesla není pro „{}“ povoleno ({message})",
                argv[0]
            )));
        }
        if message.contains("Not authorized") || message.contains("Interactive authentication") {
            return Err(Error::Permission.with_context(&format!(
                "{} vyžaduje oprávnění správce; nastavte sudo nebo pkexec v profilu",
                argv[0]
            )));
        }
        Err(Error::Unsupported(format!(
            "{} skončil kódem {}: {}",
            argv[0],
            output.status,
            if message.is_empty() {
                first_line(&output.stdout)
            } else {
                message
            }
        )))
    }
    pub fn status(&self, datadir: &str) -> Result<ServiceStatus> {
        let service = unit(&self.settings.service)?;
        let text = self.check(
            &[
                "systemctl",
                "show",
                service,
                "--no-pager",
                "--property=LoadState,ActiveState,SubState,MainPID,UnitFileState,ActiveEnterTimestamp,Description",
            ],
            false,
            None,
        )?;
        let value = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix('=')))
                .unwrap_or("")
                .to_string()
        };
        let mut status = ServiceStatus {
            unit: service.into(),
            description: value("Description"),
            load: value("LoadState"),
            active: value("ActiveState"),
            sub: value("SubState"),
            pid: value("MainPID").parse().unwrap_or(0),
            enabled: value("UnitFileState"),
            since: value("ActiveEnterTimestamp"),
            disk: None,
        };
        if status.load == "not-found" {
            return Err(Error::Validation(format!(
                "Služba {service} na hostiteli neexistuje; opravte název v profilu (např. mariadb nebo mysql)."
            )));
        }
        if !datadir.is_empty() && path(datadir).is_ok() {
            if let Ok(text) = self.check(&["df", "-P", "-k", datadir], false, None) {
                status.disk = parse_df(&text);
            }
        }
        Ok(status)
    }
    pub fn control(&self, action: ServiceAction) -> Result<String> {
        let service = unit(&self.settings.service)?;
        self.check(&["systemctl", action.verb(), service], true, None)?;
        Ok(format!("Služba {service}: {} provedeno.", action.label()))
    }
    /// The last `lines` lines of the service journal or of a log file.
    pub fn logs(&self, source: &LogSource, lines: u32) -> Result<Vec<String>> {
        let count = lines.clamp(10, 5000).to_string();
        let text = match source {
            LogSource::Journal => {
                let service = unit(&self.settings.service)?;
                self.check(
                    &[
                        "journalctl",
                        "-u",
                        service,
                        "-n",
                        &count,
                        "--no-pager",
                        "-o",
                        "short-iso",
                    ],
                    true,
                    None,
                )?
            }
            LogSource::File(file) => {
                self.check(&["tail", "-n", &count, path(file)?], true, None)?
            }
        };
        Ok(text.lines().map(|l| l.to_string()).collect())
    }
    /// Directory for Harbor's option file: configured, or the first that exists.
    fn config_dir(&self) -> Result<String> {
        if !self.settings.config_dir.is_empty() {
            return Ok(path(&self.settings.config_dir)?.to_string());
        }
        for dir in [
            "/etc/my.cnf.d",
            "/etc/mysql/mariadb.conf.d",
            "/etc/mysql/conf.d",
        ] {
            if self.run(&["test", "-d", dir], false, None)?.status == 0 {
                return Ok(dir.into());
            }
        }
        Err(Error::Validation(
            "Adresář konfigurace MariaDB nebyl nalezen; zadejte ho v profilu.".into(),
        ))
    }
    pub fn read_config(&self) -> Result<HarborConfig> {
        let dir = self.config_dir()?;
        let file = format!("{dir}/{HARBOR_CONFIG_FILE}");
        let exists = self.run(&["test", "-f", &file], false, None)?.status == 0;
        let raw = if exists {
            self.check(&["cat", &file], true, None)?
        } else {
            String::new()
        };
        Ok(HarborConfig {
            entries: parse_config(&raw),
            path: file,
            exists,
            raw,
        })
    }
    /// Validates the new option file with the server binary (dry run), keeps a backup
    /// of the previous one and replaces it.
    pub fn write_config(
        &self,
        entries: &[(String, String)],
        profile: &str,
    ) -> Result<HarborConfig> {
        let current = self.read_config()?;
        let text = render_config(entries, profile)?;
        if let Some(binary) = self.server_binary()? {
            let temporary = self
                .check(&["mktemp", "/tmp/harbor-cnf.XXXXXX"], true, None)?
                .trim()
                .to_string();
            path(&temporary)?;
            let result = self
                .check(&["tee", &temporary], true, Some(text.as_bytes()))
                .and_then(|_| {
                    let output = self.run(
                        &[
                            &binary,
                            &format!("--defaults-file={temporary}"),
                            "--help",
                            "--verbose",
                        ],
                        true,
                        None,
                    )?;
                    if output.status == 0 {
                        Ok(())
                    } else {
                        let errors: Vec<&str> = output
                            .stderr
                            .lines()
                            .chain(output.stdout.lines())
                            .filter(|l| l.contains("[ERROR]") || l.contains("unknown"))
                            .take(3)
                            .collect();
                        Err(Error::Validation(format!(
                            "Server konfiguraci odmítl: {}",
                            if errors.is_empty() {
                                format!("kód {}", output.status)
                            } else {
                                errors.join(" | ")
                            }
                        )))
                    }
                });
            let _ = self.run(&["rm", "-f", &temporary], true, None);
            result?;
        }
        if current.exists {
            let backup = format!("{}.harbor-{}.bak", current.path, compact_stamp());
            self.check(&["cp", "-p", &current.path, &backup], true, None)?;
        }
        let staged = format!("{}.harbor-new", current.path);
        self.check(&["tee", &staged], true, Some(text.as_bytes()))?;
        self.check(&["mv", "-f", &staged, &current.path], true, None)?;
        self.read_config()
    }
    /// The last 30 MB of the slow query log, aggregated by statement shape.
    pub fn slow_log(&self, file: &str) -> Result<Vec<StatementStat>> {
        let text = self.check(&["tail", "-c", "30000000", path(file)?], true, None)?;
        Ok(aggregate_slow_log(&text))
    }
    fn server_binary(&self) -> Result<Option<String>> {
        for candidate in [
            "/usr/sbin/mariadbd",
            "/usr/libexec/mariadbd",
            "/usr/sbin/mysqld",
            "/usr/libexec/mysqld",
        ] {
            if self.run(&["test", "-x", candidate], false, None)?.status == 0 {
                return Ok(Some(candidate.into()));
            }
        }
        Ok(None)
    }
}
impl Error {
    fn with_context(self, context: &str) -> Error {
        match self {
            Error::Network => Error::Unsupported(format!(
                "{context}. Ověřte klíč a známé hostitele (ssh -p … cíl)."
            )),
            Error::Permission => Error::Unsupported(format!("Oprávnění: {context}.")),
            other => other,
        }
    }
}
fn first_line(text: &str) -> String {
    safe_text(
        text.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim(),
    )
}
fn compact_stamp() -> String {
    utc_timestamp(std::time::SystemTime::now())
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect()
}
pub const HARBOR_CONFIG_FILE: &str = "zz-harbor.cnf";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}
impl ServiceAction {
    fn verb(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "spuštění",
            Self::Stop => "zastavení",
            Self::Restart => "restart",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogSource {
    Journal,
    File(String),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Disk {
    pub mount: String,
    pub total_kib: u64,
    pub used_kib: u64,
    pub available_kib: u64,
}
impl Disk {
    pub fn used_percent(&self) -> u64 {
        self.used_kib * 100 / self.total_kib.max(1)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceStatus {
    pub unit: String,
    pub description: String,
    pub load: String,
    /// active, inactive, failed, activating…
    pub active: String,
    pub sub: String,
    pub pid: u64,
    /// enabled / disabled
    pub enabled: String,
    pub since: String,
    pub disk: Option<Disk>,
}
impl ServiceStatus {
    pub fn running(&self) -> bool {
        self.active == "active"
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HarborConfig {
    pub path: String,
    pub exists: bool,
    pub entries: Vec<(String, String)>,
    pub raw: String,
}
fn parse_df(text: &str) -> Option<Disk> {
    let line = text.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    Some(Disk {
        total_kib: fields[1].parse().ok()?,
        used_kib: fields[2].parse().ok()?,
        available_kib: fields[3].parse().ok()?,
        mount: fields[5..].join(" "),
    })
}
/// `name = value` lines of Harbor's own file (comments and groups skipped).
pub fn parse_config(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty() && !l.starts_with('#') && !l.starts_with(';') && !l.starts_with('[')
        })
        .map(|l| {
            let (name, value) = l.split_once('=').unwrap_or((l, ""));
            (name.trim().replace('-', "_"), value.trim().to_string())
        })
        .collect()
}
/// Option names are server variable names; values are restricted to characters that
/// need no quoting in an option file.
pub fn config_entry(name: &str, value: &str) -> Result<(String, String)> {
    let name = name.trim().to_ascii_lowercase().replace('-', "_");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::Validation("Neplatný název volby.".into()));
    }
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_alphanumeric() || " _.,:/+-=@".contains(c))
    {
        return Err(Error::Validation(format!(
            "Hodnota volby {name} smí obsahovat jen písmena, číslice, mezery a _.,:/+-=@ (bez uvozovek a #)."
        )));
    }
    Ok((name, value.to_string()))
}
pub fn render_config(entries: &[(String, String)], profile: &str) -> Result<String> {
    let mut text = format!(
        "# Spravuje Harbor (profil {}), změněno {}.\n\
         # Ruční úpravy tohoto souboru Harbor přepíše; jiné soubory konfigurace nemění.\n\
         # Předchozí verze: {HARBOR_CONFIG_FILE}.harbor-<čas>.bak ve stejném adresáři.\n\
         [mariadb]\n",
        profile.replace('\n', " "),
        utc_timestamp(std::time::SystemTime::now())
    );
    for (name, value) in entries {
        let (name, value) = config_entry(name, value)?;
        text.push_str(&format!("{name} = {value}\n"));
    }
    Ok(text)
}

// ---------------------------------------------------------------- background worker

pub enum HostRequest {
    Status {
        datadir: String,
    },
    Control(ServiceAction),
    Logs {
        source: LogSource,
        lines: u32,
    },
    ReadConfig,
    WriteConfig {
        entries: Vec<(String, String)>,
        profile: String,
    },
    /// Reads the end of the slow query log file and aggregates it.
    SlowLog {
        path: String,
    },
}
pub enum HostResponse {
    Status(ServiceStatus),
    Controlled(String),
    Logs(Vec<String>),
    Config(HarborConfig),
    ConfigWritten(HarborConfig),
    SlowLog(Vec<StatementStat>),
}
/// Runs host commands one at a time in a thread, so slow SSH never blocks the UI or
/// database monitoring.
pub struct HostWorker {
    tx: mpsc::Sender<(u64, HostRequest)>,
    rx: mpsc::Receiver<(u64, Result<HostResponse>)>,
    generation: u64,
    pub busy: bool,
}
impl HostWorker {
    pub fn new(settings: HostSettings, demo: bool) -> Self {
        let (tx, requests) = mpsc::channel::<(u64, HostRequest)>();
        let (results, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut fake = demo.then(DemoHost::default);
            let host = if demo { None } else { Host::new(settings).ok() };
            while let Ok((id, request)) = requests.recv() {
                let result = match (&host, &mut fake) {
                    (_, Some(fake)) => fake.handle(request),
                    (Some(host), None) => handle(host, request),
                    (None, None) => Err(Error::Validation(
                        "Profil nemá nastavený přístup k hostiteli (Připojení → Přístup k serveru).".into(),
                    )),
                };
                if results.send((id, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            tx,
            rx,
            generation: 0,
            busy: false,
        }
    }
    pub fn send(&mut self, request: HostRequest) -> bool {
        if self.busy {
            return false;
        }
        self.generation += 1;
        self.busy = self.tx.send((self.generation, request)).is_ok();
        self.busy
    }
    pub fn poll(&mut self) -> Option<Result<HostResponse>> {
        match self.rx.try_recv() {
            Ok((id, result)) if id == self.generation => {
                self.busy = false;
                Some(result)
            }
            Ok(_) => None,
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                if self.busy {
                    self.busy = false;
                    Some(Err(Error::Worker))
                } else {
                    None
                }
            }
        }
    }
}
fn handle(host: &Host, request: HostRequest) -> Result<HostResponse> {
    Ok(match request {
        HostRequest::Status { datadir } => HostResponse::Status(host.status(&datadir)?),
        HostRequest::Control(action) => HostResponse::Controlled(host.control(action)?),
        HostRequest::Logs { source, lines } => HostResponse::Logs(host.logs(&source, lines)?),
        HostRequest::ReadConfig => HostResponse::Config(host.read_config()?),
        HostRequest::WriteConfig { entries, profile } => {
            HostResponse::ConfigWritten(host.write_config(&entries, &profile)?)
        }
        HostRequest::SlowLog { path } => HostResponse::SlowLog(host.slow_log(&path)?),
    })
}
/// In-memory host for the demo mode.
struct DemoHost {
    running: bool,
    config: Vec<(String, String)>,
}
impl Default for DemoHost {
    fn default() -> Self {
        Self {
            running: true,
            config: vec![("innodb_buffer_pool_size".into(), "8G".into())],
        }
    }
}
impl DemoHost {
    fn handle(&mut self, request: HostRequest) -> Result<HostResponse> {
        let config = |entries: &Vec<(String, String)>| HarborConfig {
            path: format!("/etc/my.cnf.d/{HARBOR_CONFIG_FILE} (demo)"),
            exists: true,
            raw: render_config(entries, "demo").unwrap_or_default(),
            entries: entries.clone(),
        };
        Ok(match request {
            HostRequest::Status { .. } => HostResponse::Status(ServiceStatus {
                unit: "mariadb".into(),
                description: "MariaDB database server (demo)".into(),
                load: "loaded".into(),
                active: if self.running { "active" } else { "inactive" }.into(),
                sub: if self.running { "running" } else { "dead" }.into(),
                pid: if self.running { 4242 } else { 0 },
                enabled: "enabled".into(),
                since: "Sun 2026-11-01 08:00:00 CET".into(),
                disk: Some(Disk {
                    mount: "/var/lib/mysql".into(),
                    total_kib: 100 << 20,
                    used_kib: 87 << 20,
                    available_kib: 13 << 20,
                }),
            }),
            HostRequest::Control(action) => {
                self.running = action != ServiceAction::Stop;
                HostResponse::Controlled(format!("DEMO: {} služby mariadb.", action.label()))
            }
            HostRequest::Logs { .. } => HostResponse::Logs(vec![
                "2026-11-01T08:00:00+0100 demo mariadbd[4242]: [Note] Starting MariaDB 11.4 (demo)".into(),
                "2026-11-01T08:00:01+0100 demo mariadbd[4242]: [Warning] Aborted connection 1842 to db: 'commerce' user: 'app' (Got timeout reading communication packets)".into(),
                "2026-11-01T09:12:44+0100 demo mariadbd[4242]: [ERROR] Incorrect definition of table mysql.column_stats (demo)".into(),
                "2026-11-01T09:13:00+0100 demo mariadbd[4242]: [Note] InnoDB: Buffer pool(s) load completed".into(),
            ]),
            HostRequest::ReadConfig => HostResponse::Config(config(&self.config)),
            HostRequest::WriteConfig { entries, .. } => {
                render_config(&entries, "demo")?;
                self.config = entries;
                HostResponse::ConfigWritten(config(&self.config))
            }
            HostRequest::SlowLog { .. } => HostResponse::SlowLog(Vec::new()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_targets_units_and_paths_are_validated() {
        let ssh = |target: &str| HostSettings {
            access: HostAccess::Ssh {
                target: target.into(),
                port: 22,
            },
            elevation: Elevation::Sudo,
            ..Default::default()
        };
        assert!(ssh("admin@db1.example.cz").validate().is_ok());
        for bad in ["-oProxyCommand=x", "a b", "a;b", ""] {
            assert!(ssh(bad).validate().is_err(), "{bad}");
        }
        let mut pk = ssh("db1");
        pk.elevation = Elevation::Pkexec;
        assert!(pk.validate().is_err());
        assert!(unit("mariadb@1.service").is_ok());
        assert!(unit("maria;db").is_err() && unit("-x").is_err());
        assert!(path("/var/log/mysql/error.log").is_ok());
        assert!(path("relative").is_err() && path("/a/../etc").is_err());
    }
    #[test]
    fn remote_arguments_are_shell_quoted() {
        assert_eq!(shell_quote("systemctl"), "systemctl");
        assert_eq!(
            shell_quote("--defaults-file=/tmp/x"),
            "--defaults-file=/tmp/x"
        );
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's; rm -rf /"), "'it'\\''s; rm -rf /'");
        assert_eq!(shell_quote(""), "''");
    }
    #[test]
    fn option_file_is_rendered_and_parsed() {
        let entries = vec![
            ("max-connections".to_string(), "300".to_string()),
            (
                "sql_mode".to_string(),
                "STRICT_TRANS_TABLES,NO_ZERO_DATE".to_string(),
            ),
        ];
        let text = render_config(&entries, "Prod").unwrap();
        assert!(text.contains(
            "[mariadb]\nmax_connections = 300\nsql_mode = STRICT_TRANS_TABLES,NO_ZERO_DATE\n"
        ));
        assert_eq!(
            parse_config(&text),
            vec![
                ("max_connections".to_string(), "300".to_string()),
                (
                    "sql_mode".to_string(),
                    "STRICT_TRANS_TABLES,NO_ZERO_DATE".to_string()
                )
            ]
        );
        for bad in ["1 # komentář", "\"x\"", "a\nb", ""] {
            assert!(config_entry("x", bad).is_err(), "{bad:?}");
        }
        assert!(config_entry("bad name", "1").is_err());
        assert_eq!(
            parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 1000 870 130 87% /var/lib/mysql\n"),
            Some(Disk { mount: "/var/lib/mysql".into(), total_kib: 1000, used_kib: 870, available_kib: 130 })
        );
    }
    #[test]
    fn demo_host_simulates_service_and_config() {
        let mut worker = HostWorker::new(HostSettings::default(), true);
        let wait = |worker: &mut HostWorker| loop {
            if let Some(result) = worker.poll() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(worker.send(HostRequest::Control(ServiceAction::Stop)));
        assert!(
            !worker.send(HostRequest::ReadConfig),
            "one request at a time"
        );
        assert!(matches!(wait(&mut worker), Ok(HostResponse::Controlled(_))));
        worker.send(HostRequest::Status {
            datadir: String::new(),
        });
        let Ok(HostResponse::Status(status)) = wait(&mut worker) else {
            panic!()
        };
        assert!(!status.running());
        worker.send(HostRequest::WriteConfig {
            entries: vec![("max_connections".into(), "500".into())],
            profile: "demo".into(),
        });
        let Ok(HostResponse::ConfigWritten(config)) = wait(&mut worker) else {
            panic!()
        };
        assert_eq!(
            config.entries,
            vec![("max_connections".to_string(), "500".to_string())]
        );
        // Without host access every request reports why.
        let mut none = HostWorker::new(HostSettings::default(), false);
        none.send(HostRequest::ReadConfig);
        assert!(wait(&mut none).is_err());
    }
}
