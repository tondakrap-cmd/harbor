use crossterm::event::{KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M};
use harbor_core::*;
use harbor_themes::{rgb, terminal, ThemeId};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Sparkline, Wrap,
    },
    Frame,
};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

const PAGES: [&str; 14] = [
    "Server",
    "Databáze",
    "Účty",
    "Spojení",
    "Proměnné",
    "Doktor",
    "Služba",
    "Zálohy",
    "Profily",
    "Nastavení",
    "Výkon",
    "Replikace",
    "Konzole",
    "Porovnání",
];
const SERVER: usize = 0;
const DATABASES: usize = 1;
const ACCOUNTS: usize = 2;
const PROCESSES: usize = 3;
const VARIABLES: usize = 4;
const HEALTH: usize = 5;
const SERVICE: usize = 6;
const BACKUPS: usize = 7;
const PROFILES: usize = 8;
const SETTINGS: usize = 9;
const PERFORMANCE: usize = 10;
const REPLICATION: usize = 11;
const CONSOLE: usize = 12;
const COMPARE: usize = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Page(usize),
    Help,
    Refresh,
    NewProfile,
    Demo,
    Disconnect,
    CreateDatabase,
    DropDatabase,
    TableDetail,
    Maintenance,
    CreateAccount,
    CreateRole,
    ChangePassword,
    EditPrivileges,
    Template,
    AccountProperties,
    RoleMembership,
    DefaultRole,
    DropAccount,
    KillQuery,
    SetVariable,
    RunHealth,
    ToggleLocks,
    KillConnection,
    PerformanceReload,
    EnableSlowLog,
    ResetStats,
    ExplainSample,
    ReplicationReload,
    ReplicaStart,
    ReplicaStop,
    PurgeBinlogs,
    CompareRun,
    CompareExport,
    ConsoleRun,
    ConsoleCancel,
    ConsoleExport,
    HostRefresh,
    ServiceStart,
    ServiceRestart,
    ServiceStop,
    LogsSource,
    LogsLevel,
    ConfigRemove,
    VariableToConfig,
    BackupStart,
    BackupVerify,
    BackupRestore,
    BackupDelete,
    JobCancel,
    ScheduleCreate,
    ScheduleRemove,
    Search,
    ToggleSystem,
    ToggleLong,
    ToggleKeyVariables,
    ToggleChangedVariables,
    NextTheme,
    ResumeDraft,
    DeleteProfile,
    Quit,
}
const MENU: &[(&str, Action)] = &[
    ("Server (1)", Action::Page(SERVER)),
    ("Databáze (2)", Action::Page(DATABASES)),
    ("Uživatelé (3)", Action::Page(ACCOUNTS)),
    ("Spojení (4)", Action::Page(PROCESSES)),
    ("Proměnné (5)", Action::Page(VARIABLES)),
    ("Doktor (6)", Action::Page(HEALTH)),
    ("Služba a logy (7)", Action::Page(SERVICE)),
    ("Zálohy (8)", Action::Page(BACKUPS)),
    ("Profily (9, F3)", Action::Page(PROFILES)),
    ("Nastavení (0)", Action::Page(SETTINGS)),
    ("Výkon a dotazy ([ ])", Action::Page(PERFORMANCE)),
    ("Replikace ([ ])", Action::Page(REPLICATION)),
    ("SQL konzole ([ ])", Action::Page(CONSOLE)),
    ("Porovnání serverů ([ ])", Action::Page(COMPARE)),
    ("Nápověda", Action::Help),
    ("Obnovit", Action::Refresh),
    ("Nový profil", Action::NewProfile),
    ("Otevřít DEMO", Action::Demo),
    ("Odpojit / zrušit připojování", Action::Disconnect),
    ("Databáze: vytvořit", Action::CreateDatabase),
    ("Databáze: smazat vybranou…", Action::DropDatabase),
    ("Tabulka: struktura a DDL", Action::TableDetail),
    (
        "Tabulka: údržba (ANALYZE/CHECK/OPTIMIZE/REPAIR)",
        Action::Maintenance,
    ),
    ("Účet: vytvořit", Action::CreateAccount),
    ("Role: vytvořit", Action::CreateRole),
    ("Účet: změnit heslo", Action::ChangePassword),
    ("Účet: upravit oprávnění…", Action::EditPrivileges),
    ("Účet: šablona oprávnění…", Action::Template),
    ("Účet: zámek, TLS a limity…", Action::AccountProperties),
    ("Účet: přidělit / odebrat roli…", Action::RoleMembership),
    ("Účet: výchozí role…", Action::DefaultRole),
    ("Účet / role: smazat…", Action::DropAccount),
    ("Spojení: ukončit vybraný dotaz", Action::KillQuery),
    ("Spojení: ukončit celé spojení…", Action::KillConnection),
    ("Spojení: transakce a zámky ano/ne", Action::ToggleLocks),
    ("Výkon: načíst znovu", Action::PerformanceReload),
    ("Výkon: zapnout log pomalých dotazů…", Action::EnableSlowLog),
    ("Výkon: vynulovat statistiky…", Action::ResetStats),
    ("Výkon: EXPLAIN vzorku v konzoli", Action::ExplainSample),
    ("Replikace: načíst znovu", Action::ReplicationReload),
    ("Replikace: spustit vybrané spojení", Action::ReplicaStart),
    ("Replikace: zastavit vybrané spojení…", Action::ReplicaStop),
    ("Binlogy: smazat starší než vybraný…", Action::PurgeBinlogs),
    ("Porovnání: spustit…", Action::CompareRun),
    ("Porovnání: uložit do CSV…", Action::CompareExport),
    ("Konzole: spustit dotaz…", Action::ConsoleRun),
    ("Konzole: zrušit běžící dotaz", Action::ConsoleCancel),
    ("Konzole: uložit výsledek do CSV…", Action::ConsoleExport),
    ("Proměnná: změnit hodnotu (SET GLOBAL)", Action::SetVariable),
    ("Doktor: spustit kontrolu", Action::RunHealth),
    (
        "Služba: obnovit stav, konfiguraci a log",
        Action::HostRefresh,
    ),
    ("Služba: spustit", Action::ServiceStart),
    ("Služba: restartovat…", Action::ServiceRestart),
    ("Služba: zastavit…", Action::ServiceStop),
    ("Log: journal / error log", Action::LogsSource),
    ("Log: vše / varování a chyby / chyby", Action::LogsLevel),
    ("Konfigurace: odebrat volbu Harboru…", Action::ConfigRemove),
    ("Proměnná: uložit do konfigurace…", Action::VariableToConfig),
    ("Záloha: vytvořit…", Action::BackupStart),
    ("Záloha: ověřit vybranou", Action::BackupVerify),
    ("Záloha: obnovit vybranou…", Action::BackupRestore),
    ("Záloha: smazat vybraný soubor…", Action::BackupDelete),
    ("Úloha: zrušit běžící", Action::JobCancel),
    ("Plán záloh: vytvořit…", Action::ScheduleCreate),
    ("Plán záloh: odebrat…", Action::ScheduleRemove),
    ("Hledat", Action::Search),
    ("Systémové databáze ano/ne", Action::ToggleSystem),
    ("Jen dlouhé dotazy ano/ne", Action::ToggleLong),
    ("Jen klíčové proměnné ano/ne", Action::ToggleKeyVariables),
    (
        "Jen změněné proměnné ano/ne",
        Action::ToggleChangedVariables,
    ),
    ("Další motiv (zachová formulář)", Action::NextTheme),
    ("Rozpracovaný formulář", Action::ResumeDraft),
    ("Odstranit vybraný uložený profil", Action::DeleteProfile),
    ("Konec", Action::Quit),
];
/// Actions of a screen, offered first in the menu (F10) on that screen.
fn page_actions(page: usize) -> &'static [Action] {
    match page {
        DATABASES => &[
            Action::CreateDatabase,
            Action::TableDetail,
            Action::Maintenance,
            Action::DropDatabase,
            Action::Search,
            Action::ToggleSystem,
        ],
        ACCOUNTS => &[
            Action::EditPrivileges,
            Action::Template,
            Action::ChangePassword,
            Action::AccountProperties,
            Action::RoleMembership,
            Action::DefaultRole,
            Action::CreateAccount,
            Action::CreateRole,
            Action::DropAccount,
            Action::Search,
        ],
        PROCESSES => &[
            Action::ToggleLocks,
            Action::KillQuery,
            Action::KillConnection,
            Action::Search,
            Action::ToggleLong,
        ],
        PERFORMANCE => &[
            Action::PerformanceReload,
            Action::ExplainSample,
            Action::EnableSlowLog,
            Action::ResetStats,
            Action::Search,
        ],
        REPLICATION => &[
            Action::ReplicationReload,
            Action::ReplicaStart,
            Action::ReplicaStop,
            Action::PurgeBinlogs,
        ],
        COMPARE => &[Action::CompareRun, Action::CompareExport, Action::Search],
        CONSOLE => &[
            Action::ConsoleRun,
            Action::ConsoleCancel,
            Action::ConsoleExport,
        ],
        VARIABLES => &[
            Action::SetVariable,
            Action::VariableToConfig,
            Action::Search,
            Action::ToggleKeyVariables,
            Action::ToggleChangedVariables,
        ],
        HEALTH => &[Action::RunHealth, Action::Search],
        SERVICE => &[
            Action::HostRefresh,
            Action::ServiceStart,
            Action::ServiceRestart,
            Action::ServiceStop,
            Action::LogsSource,
            Action::LogsLevel,
            Action::ConfigRemove,
            Action::Search,
        ],
        BACKUPS => &[
            Action::BackupStart,
            Action::BackupVerify,
            Action::BackupRestore,
            Action::BackupDelete,
            Action::JobCancel,
            Action::ScheduleCreate,
            Action::ScheduleRemove,
        ],
        PROFILES => &[Action::NewProfile, Action::DeleteProfile, Action::Demo],
        _ => &[Action::Refresh, Action::Demo, Action::Disconnect],
    }
}
/// Menu rows: this screen's actions, a separator (None), then everything else.
fn menu_items(page: usize) -> Vec<(&'static str, Option<Action>)> {
    let first = page_actions(page);
    let label = |action: Action| {
        MENU.iter()
            .find(|(_, a)| *a == action)
            .map(|(l, _)| *l)
            .unwrap_or("")
    };
    let mut items: Vec<(&'static str, Option<Action>)> =
        first.iter().map(|a| (label(*a), Some(*a))).collect();
    items.push(("────────── všechny akce ──────────", None));
    items.extend(
        MENU.iter()
            .filter(|(_, a)| !first.contains(a))
            .map(|(l, a)| (*l, Some(*a))),
    );
    items
}
/// Values are wiped on drop; dialogs are moved (suspended/restored), never cloned, so a
/// typed password exists in one buffer only.
struct Field {
    label: String,
    value: Zeroizing<String>,
    secret: bool,
}
impl Field {
    fn new(label: &str, value: &str) -> Self {
        Self {
            label: label.into(),
            value: Zeroizing::new(value.into()),
            secret: false,
        }
    }
    fn password() -> Self {
        Self {
            label: "Heslo (pouze v paměti)".into(),
            value: Zeroizing::new(String::new()),
            secret: true,
        }
    }
    fn secret(&self) -> Secret {
        Secret(self.value.clone())
    }
    fn hidden(label: &str) -> Self {
        Self {
            label: label.into(),
            value: Zeroizing::new(String::new()),
            secret: true,
        }
    }
    /// Secret or None when left empty.
    fn optional_secret(&self) -> Option<Secret> {
        (!self.value.is_empty()).then(|| self.secret())
    }
}
/// Selected row identity; the index follows it when a refresh reorders the list.
#[derive(Clone, PartialEq, Debug)]
enum RowKey {
    Database(String),
    Account(Account),
    Process(u64),
    Variable(String),
    Finding(String),
    Backup(std::path::PathBuf),
    Log(usize),
    Profile(String),
}
enum Outcome {
    Keep,
    Close,
    Suspend,
}
#[derive(Clone)]
enum Kind {
    /// Id of the saved profile being edited; None = new profile.
    Profile(Option<String>),
    Database,
    Account,
    Password(Account),
    Role,
    RoleMembership(Account),
    DefaultRole(Account),
    Template(Account),
    Properties(Account),
    /// Maintenance of a table in this database.
    Maintenance(String),
    Variable(Variable),
    Backup,
    Verify(std::path::PathBuf),
    Restore(std::path::PathBuf),
    Schedule,
    Unschedule,
    /// Value of this option in Harbor's option file.
    ConfigValue(String),
    Console,
    ConsoleExport,
    Compare,
    CompareExport,
    ConfigRemove,
    /// Search for the list on this page.
    Search(usize),
}
impl Kind {
    fn account(&self) -> Option<&Account> {
        match self {
            Self::Password(a)
            | Self::RoleMembership(a)
            | Self::DefaultRole(a)
            | Self::Template(a)
            | Self::Properties(a) => Some(a),
            _ => None,
        }
    }
    /// Bound to objects of the connected server (dropped on a server switch).
    fn server_bound(&self) -> bool {
        self.account().is_some()
            || matches!(
                self,
                Self::Maintenance(_)
                    | Self::Variable(_)
                    | Self::Backup
                    | Self::Restore(_)
                    | Self::ConfigValue(_)
                    | Self::ConfigRemove
            )
    }
}
struct Dialog {
    kind: Kind,
    fields: Vec<Field>,
    focus: usize,
}
/// Checkbox list of an account's privileges at one scope.
struct PrivilegePicker {
    account: Account,
    /// Scope picked from the existing grants (keeps wildcard patterns verbatim).
    fixed: Option<GrantScope>,
    scope_text: String,
    loaded_for: Option<GrantScope>,
    wanted: BTreeSet<String>,
    /// 0 = scope line, 1..=n privileges, then the two buttons.
    cursor: usize,
}
impl PrivilegePicker {
    fn scope(&self) -> Result<GrantScope> {
        match &self.fixed {
            Some(scope) => Ok(scope.clone()),
            None => GrantScope::parse(&self.scope_text),
        }
    }
    fn options(&self) -> Vec<&'static str> {
        match self.scope() {
            Ok(scope) => scope
                .privileges()
                .iter()
                .copied()
                .chain([GRANT_OPTION])
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}
fn scope_text(scope: &GrantScope) -> String {
    match scope {
        GrantScope::Global => "*.*".into(),
        GrantScope::Database(db) => format!("{db}.*"),
        GrantScope::DatabasePattern(p) => format!("{p}.*"),
        GrantScope::Table(db, t) => format!("{db}.{t}"),
    }
}
// At most one confirmation exists at a time; boxing would only add indirection.
#[allow(clippy::large_enum_variant)]
enum Pending {
    Server(Change),
    DeleteProfile {
        id: String,
    },
    Restore(RestoreRequest),
    DeleteBackup(std::path::PathBuf),
    Schedule(schedule::Schedule),
    Unschedule(String),
    /// Action and the service unit (typed to confirm a stop).
    Service(ServiceAction, String),
    /// The complete new content of Harbor's option file.
    WriteConfig(Vec<(String, String)>),
    /// A console statement that writes: (database, SQL).
    ConsoleWrite(String, String),
}
struct Confirmation {
    action: Pending,
    yes: bool,
    /// Name typed to unlock a destructive change.
    typed: String,
}
impl Confirmation {
    fn new(action: Pending) -> Self {
        Self {
            action,
            yes: false,
            typed: String::new(),
        }
    }
    fn required_name(&self) -> Option<&str> {
        match &self.action {
            Pending::Server(change) => change.destructive_name(),
            Pending::Restore(r) if r.replace => Some(&r.target),
            Pending::Service(ServiceAction::Stop, unit) => Some(unit),
            _ => None,
        }
    }
    /// Server changes and restores return to their suspended form when cancelled.
    fn returns_to_form(&self) -> bool {
        !matches!(
            self.action,
            Pending::DeleteProfile { .. }
                | Pending::DeleteBackup(_)
                | Pending::Unschedule(_)
                | Pending::Service(..)
        )
    }
}
pub struct App {
    session: Session,
    settings: Settings,
    store: Option<ConfigStore>,
    theme: ThemeId,
    colors: String,
    ascii: bool,
    page: usize,
    selected: usize,
    selected_key: Option<RowKey>,
    horizontal: usize,
    detail_scroll: u16,
    detail_focus: bool,
    /// Table under the cursor in the database detail (detail focus on Databáze).
    table_cursor: usize,
    /// Table structure overlay is open.
    table_overlay: bool,
    overlay_scroll: u16,
    menu: Option<usize>,
    help: bool,
    dialog: Option<Dialog>,
    suspended: Option<Dialog>,
    picker: Option<PrivilegePicker>,
    confirm: Option<Confirmation>,
    /// One search per page, like the desktop.
    filters: [String; 14],
    system: bool,
    long: bool,
    key_only: bool,
    changed_only: bool,
    detail: String,
    status: String,
    status_at: std::time::Instant,
    /// Worker generation the selection and account-bound drafts belong to.
    scope_generation: u64,
    loaded_variables: Option<u64>,
    loaded_health: Option<u64>,
    backups: Vec<BackupFile>,
    backups_loaded: bool,
    jobs_finished_seen: usize,
    /// Installed timers; read with the backup list (asks systemctl), not per frame.
    schedules: Vec<schedule::InstalledSchedule>,
    /// Spojení shows open transactions and locks instead of the process list.
    locks_view: bool,
    locks_requested: Option<std::time::Instant>,
    loaded_performance: Option<u64>,
    loaded_replication: Option<u64>,
    log_from_file: bool,
    /// 0 = all, 1 = warnings and errors, 2 = errors.
    log_level: u8,
    pub quit: bool,
}
#[derive(Clone, Copy)]
struct Palette {
    base: Style,
    menu: Style,
    selected: Style,
    border: Color,
    accent: Color,
}
impl App {
    pub fn new(args: &[String]) -> Self {
        let store = ConfigStore::standard().ok();
        let loaded = store.as_ref().map(ConfigStore::load).transpose();
        let status = loaded
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let settings = loaded.ok().flatten().unwrap_or_default();
        let colors = args
            .iter()
            .find_map(|a| a.strip_prefix("--colors="))
            .unwrap_or(&settings.terminal_colors)
            .to_string();
        let ascii = args.iter().any(|a| a == "--ascii") || settings.ascii;
        let mut app = Self {
            theme: ThemeId::parse(&settings.tui_theme).unwrap_or(ThemeId::TurboClassic),
            colors,
            ascii,
            session: Session::default(),
            settings,
            store,
            page: PROFILES,
            selected: 0,
            selected_key: None,
            horizontal: 0,
            detail_scroll: 0,
            detail_focus: false,
            table_cursor: 0,
            table_overlay: false,
            overlay_scroll: 0,
            menu: None,
            help: false,
            dialog: None,
            suspended: None,
            picker: None,
            confirm: None,
            filters: Default::default(),
            system: false,
            long: false,
            key_only: true,
            changed_only: false,
            detail: String::new(),
            status,
            status_at: std::time::Instant::now(),
            scope_generation: 0,
            loaded_variables: None,
            loaded_health: None,
            backups: Vec::new(),
            backups_loaded: false,
            jobs_finished_seen: 0,
            schedules: Vec::new(),
            locks_view: false,
            locks_requested: None,
            loaded_performance: None,
            loaded_replication: None,
            log_from_file: false,
            log_level: 0,
            quit: false,
        };
        if args.iter().any(|a| a == "--demo") {
            app.session.connect(
                Profile {
                    name: "Ukázkový server".into(),
                    ..Default::default()
                },
                Secret::default(),
                true,
            );
            app.page = SERVER;
        }
        app.scope_generation = app.session.worker.generation();
        app
    }
    pub fn tick(&mut self) {
        self.session.tick(self.settings.refresh_seconds);
        self.sync_server_scope();
        self.reconcile_selection();
        self.autoload();
        self.host_autoload();
    }
    /// The variables and health pages load their data the first time they are shown
    /// for a server; afterwards only on request.
    fn autoload(&mut self) {
        let finished = self.session.jobs.iter().filter(|j| j.finished()).count();
        if self.page == BACKUPS && (!self.backups_loaded || finished != self.jobs_finished_seen) {
            self.jobs_finished_seen = finished;
            self.reload_backups();
        }
        if self.session.snapshot.is_none() {
            return;
        }
        let generation = self.session.worker.generation();
        if self.page == VARIABLES
            && self.session.variables.is_none()
            && self.loaded_variables != Some(generation)
        {
            self.loaded_variables = Some(generation);
            self.session.request(Operation::Variables);
        }
        if self.page == HEALTH
            && self.session.health.is_none()
            && self.loaded_health != Some(generation)
        {
            self.loaded_health = Some(generation);
            self.session.request(Operation::Health);
        }
        if self.page == PERFORMANCE
            && self.session.performance.is_none()
            && self.loaded_performance != Some(generation)
        {
            self.loaded_performance = Some(generation);
            self.session.request(Operation::Performance);
        }
        if self.page == REPLICATION
            && self.session.replication.is_none()
            && self.loaded_replication != Some(generation)
        {
            self.loaded_replication = Some(generation);
            self.session.request(Operation::Replication);
        }
        if self.page == PROCESSES
            && self.locks_view
            && self
                .locks_requested
                .is_none_or(|at| at.elapsed().as_secs() >= self.settings.refresh_seconds.max(2))
        {
            self.locks_requested = Some(std::time::Instant::now());
            self.session.request(Operation::Locks);
        }
    }
    fn reload_backups(&mut self) {
        self.backups = list_backups(&self.settings.backup_dir());
        self.schedules = schedule::list();
        self.backups_loaded = true;
    }
    fn selected_backup(&mut self) -> Option<BackupFile> {
        let file = (self.page == BACKUPS)
            .then(|| self.backups.get(self.selected).cloned())
            .flatten();
        if file.is_none() {
            self.say("Zálohu vyberte na obrazovce Zálohy (7).");
        }
        file
    }
    /// Loads the service page's data from the host, one command at a time.
    fn host_autoload(&mut self) {
        if self.session.host.is_none()
            || self.session.host_busy()
            || self.session.host_error.is_some()
        {
            return;
        }
        let snapshot = self.session.snapshot.clone();
        let datadir = snapshot
            .as_ref()
            .map(|s| s.datadir.clone())
            .unwrap_or_default();
        if self.page == SERVICE {
            if self.session.host_status.is_none() {
                self.session.host_request(HostRequest::Status { datadir });
            } else if self.session.host_config.is_none() {
                self.session.host_request(HostRequest::ReadConfig);
            } else if self.session.host_logs.is_none() {
                let file = snapshot
                    .map(|s| s.log_error.clone())
                    .filter(|f| f.starts_with('/') && self.log_from_file);
                self.session.host_request(HostRequest::Logs {
                    source: file.map(LogSource::File).unwrap_or(LogSource::Journal),
                    lines: 500,
                });
            }
        } else if self.page == VARIABLES && self.session.host_config.is_none() {
            self.session.host_request(HostRequest::ReadConfig);
        } else if self.page == PERFORMANCE && self.session.host_slow_log.is_none() {
            if let Some(Some(StatsSource::SlowLogFile(path))) =
                self.session.performance.as_ref().map(|p| p.source.clone())
            {
                self.session.host_request(HostRequest::SlowLog { path });
            }
        }
    }
    fn log_lines(&self) -> Vec<&String> {
        self.session
            .host_logs
            .as_ref()
            .map(|lines| {
                lines
                    .iter()
                    .filter(|l| {
                        let level = match self.log_level {
                            1 => l.contains("ERROR") || l.contains("[Warning]"),
                            2 => l.contains("ERROR"),
                            _ => true,
                        };
                        level && matches_filter(&self.filters[SERVICE], &[l])
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn service_text(&self) -> String {
        let mut lines = Vec::new();
        if let Some(error) = &self.session.host_error {
            lines.push(format!("! {}", safe_text(error)));
        }
        if let Some(message) = &self.session.host_message {
            lines.push(safe_text(message));
        }
        match &self.session.host_status {
            Some(st) => {
                lines.push(format!(
                    "{} {} ({}) • PID {} • od {} • po startu: {}",
                    safe_text(&st.unit),
                    if st.running() { "BĚŽÍ" } else { "NEBĚŽÍ" },
                    safe_text(&st.sub),
                    st.pid,
                    safe_text(&st.since),
                    safe_text(&st.enabled)
                ));
                if let Some(disk) = &st.disk {
                    lines.push(format!(
                        "Disk {}: {} % obsazeno, volno {}{}",
                        safe_text(&disk.mount),
                        disk.used_percent(),
                        human_bytes(disk.available_kib * 1024),
                        if disk.used_percent() >= 85 {
                            " • DISK TÉMĚŘ PLNÝ"
                        } else {
                            ""
                        }
                    ));
                }
            }
            None if self.session.host.is_some() => lines.push("Zjišťuji stav služby…".into()),
            None => lines.push(
                "Profil nemá přístup k serveru: F3 → profil → Přístup LOCAL nebo SSH.".into(),
            ),
        }
        if let Some(config) = &self.session.host_config {
            lines.push(format!("Konfigurace Harboru: {}", safe_text(&config.path)));
            if config.entries.is_empty() {
                lines.push(
                    "  (prázdná; hodnoty ukládá Proměnné → F10 → uložit do konfigurace)".into(),
                );
            }
            for (name, value) in &config.entries {
                lines.push(format!("  {} = {}", safe_text(name), safe_text(value)));
            }
        }
        lines.push(format!(
            "Log: {} • {}",
            if self.log_from_file {
                "error log"
            } else {
                "journal"
            },
            ["vše", "varování a chyby", "jen chyby"][self.log_level as usize % 3]
        ));
        lines.join("\n")
    }
    /// Harbor's option file with `name = value` set, checked against the variable.
    fn config_change(&self, name: &str, value: &str) -> Result<Vec<(String, String)>> {
        let config = self.session.host_config.as_ref().ok_or_else(|| {
            Error::Validation("Konfigurace serveru se ještě načítá; zkuste to za chvíli.".into())
        })?;
        if let Some(variable) = self
            .session
            .variables
            .as_ref()
            .and_then(|list| list.iter().find(|v| v.name == name))
        {
            let mut check = variable.clone();
            check.read_only = false;
            check.literal(value)?;
        }
        let entry = host::config_entry(name, value)?;
        let mut entries: Vec<(String, String)> = config
            .entries
            .iter()
            .filter(|(n, _)| *n != entry.0)
            .cloned()
            .collect();
        entries.push(entry);
        Ok(entries)
    }
    /// Statement statistics shown on Výkon (database source or the slow log file).
    fn statements(&self) -> Vec<StatementStat> {
        match self
            .session
            .performance
            .as_ref()
            .and_then(|p| p.source.clone())
        {
            Some(StatsSource::SlowLogFile(_)) => {
                self.session.host_slow_log.clone().unwrap_or_default()
            }
            _ => self
                .session
                .performance
                .as_ref()
                .map(|p| p.statements.clone())
                .unwrap_or_default(),
        }
        .into_iter()
        .filter(|s| matches_filter(&self.filters[PERFORMANCE], &[&s.query, &s.schema]))
        .collect()
    }
    fn transactions(&self) -> Vec<Transaction> {
        self.session
            .locks
            .as_ref()
            .map(|r| r.transactions.clone())
            .unwrap_or_default()
    }
    /// Rows of Replikace: replication channels first, then binary logs.
    fn replication_rows(&self) -> Vec<(String, String)> {
        let Some(r) = &self.session.replication else {
            return Vec::new();
        };
        let mut rows: Vec<(String, String)> = r
            .channels
            .iter()
            .map(|c| {
                (
                    format!("channel:{}", c.name),
                    format!(
                        "{} Replikace {} z {} • IO {} SQL {} • zpoždění {}",
                        if c.healthy() { "●" } else { "✗" },
                        safe_text(&c.label()),
                        safe_text(&c.master),
                        safe_text(&c.io_running),
                        safe_text(&c.sql_running),
                        c.seconds_behind
                            .map(|s| format!("{s} s"))
                            .unwrap_or_else(|| "—".into())
                    ),
                )
            })
            .collect();
        rows.extend(r.binlogs.iter().map(|b| {
            (
                format!("binlog:{}", b.name),
                format!(
                    "  Binlog {:<28} {:>12}",
                    safe_text(&b.name),
                    human_bytes(b.bytes)
                ),
            )
        }));
        rows
    }
    fn replication_text(&self) -> String {
        let Some(r) = &self.session.replication else {
            return "Načítám…".into();
        };
        let mut lines = vec![format!(
            "server_id {} • read_only {} • binlog {} {} • GTID binlog {} • GTID repliky {}",
            r.server_id,
            if r.read_only { "ano" } else { "ne" },
            if r.log_bin { "ZAP" } else { "VYP" },
            safe_text(&r.binlog_format),
            safe_text(&r.gtid_binlog_pos),
            safe_text(&r.gtid_slave_pos)
        )];
        if r.log_bin {
            lines.push(format!(
                "Binární logy: {} souborů, {} • automatické mazání {}",
                r.binlogs.len(),
                human_bytes(r.binlog_bytes()),
                if r.expire_seconds > 0.0 {
                    format!("po {:.1} dnech", r.expire_seconds / 86_400.0)
                } else {
                    "VYPNUTÉ".into()
                }
            ));
        }
        if !r.replicas.is_empty() {
            lines.push(format!(
                "Připojené repliky: {}",
                safe_text(&r.replicas.join(", "))
            ));
        }
        if let Some(g) = &r.galera {
            lines.push(format!(
                "Galera {}: uzel {} {} • cluster {} ({} uzlů) • flow control {:.1} %",
                safe_text(&g.cluster_name),
                safe_text(&g.node_name),
                safe_text(&g.local_state),
                safe_text(&g.cluster_status),
                g.cluster_size,
                g.flow_control_paused * 100.0
            ));
        }
        let rows = self.replication_rows();
        if let Some((key, _)) = rows.get(self.selected) {
            if let Some(name) = key.strip_prefix("channel:") {
                if let Some(c) = r.channels.iter().find(|c| c.name == name) {
                    lines.push(format!("Stav: {}", safe_text(&c.state)));
                    for error in [&c.last_io_error, &c.last_sql_error] {
                        if !error.is_empty() {
                            lines.push(format!("! {}", safe_text(error)));
                        }
                    }
                    lines.push("F10: spustit / zastavit replikaci".into());
                }
            } else {
                lines.push("F10: smazat starší binární logy než vybraný".into());
            }
        }
        lines.extend(r.notes.iter().map(|n| safe_text(n)));
        lines.join("\n")
    }
    fn differences(&self) -> Vec<Difference> {
        self.session
            .compare
            .as_ref()
            .and_then(|c| c.result())
            .and_then(|r| r.ok())
            .map(|r| {
                r.differences
                    .into_iter()
                    .filter(|d| {
                        matches_filter(&self.filters[COMPARE], &[&d.object, &d.left, &d.right])
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn compare_text(&self) -> String {
        let Some(job) = &self.session.compare else {
            return "Porovná proměnné, účty s oprávněními a schémata (tabulky, sloupce, indexy)."
                .into();
        };
        match job.result() {
            None => format!("{} ({} s)", job.phase(), job.started.elapsed().as_secs()),
            Some(Err(error)) => format!("! {}", safe_text(&error)),
            Some(Ok(result)) => {
                let mut lines = vec![format!(
                    "Vlevo {} • vpravo {} • {} rozdílů • {} vždy odlišných proměnných vynecháno",
                    safe_text(&result.left),
                    safe_text(&result.right),
                    result.differences.len(),
                    result.ignored
                )];
                if let Some(d) = self.differences().get(self.selected) {
                    lines.push(format!(
                        "{} • {} • {}",
                        d.area.label(),
                        d.kind.label(),
                        safe_text(&d.object)
                    ));
                    lines.push(format!(
                        "  vlevo:  {}",
                        if d.left.is_empty() {
                            "— neexistuje —".into()
                        } else {
                            safe_text(&d.left)
                        }
                    ));
                    lines.push(format!(
                        "  vpravo: {}",
                        if d.right.is_empty() {
                            "— neexistuje —".into()
                        } else {
                            safe_text(&d.right)
                        }
                    ));
                }
                lines.join("\n")
            }
        }
    }
    fn console_rows(&self) -> Vec<String> {
        let Some(job) = &self.session.console else {
            return Vec::new();
        };
        match job.result() {
            None => vec![format!(
                "Běží {} s… (F10 → zrušit)",
                job.started.elapsed().as_secs()
            )],
            Some(Err(error)) => vec![format!("! {}", safe_text(&error))],
            Some(Ok(result)) => {
                if let Some(n) = result.affected {
                    return vec![format!(
                        "Provedeno: {n} řádků změněno • {} ms",
                        result.elapsed_ms
                    )];
                }
                let widths: Vec<usize> = (0..result.columns.len())
                    .map(|c| {
                        result
                            .rows
                            .iter()
                            .map(|r| r[c].as_deref().unwrap_or("NULL").chars().count())
                            .chain([result.columns[c].chars().count()])
                            .max()
                            .unwrap_or(4)
                            .min(30)
                    })
                    .collect();
                let format_row = |values: Vec<&str>| {
                    values
                        .iter()
                        .zip(&widths)
                        .map(|(v, w)| {
                            let v: String = safe_text(v).chars().take(*w).collect();
                            format!("{v:<w$}", w = *w)
                        })
                        .collect::<Vec<_>>()
                        .join(" │ ")
                };
                let mut rows = vec![format_row(
                    result.columns.iter().map(String::as_str).collect(),
                )];
                rows.extend(result.rows.iter().map(|r| {
                    format_row(r.iter().map(|v| v.as_deref().unwrap_or("NULL")).collect())
                }));
                rows
            }
        }
    }
    fn console_text(&self) -> String {
        let Some(job) = &self.session.console else {
            return "F10 → Konzole: spustit dotaz… (jeden příkaz; čtení v transakci READ ONLY, zápis po potvrzení)".into();
        };
        let mut lines = vec![format!("SQL: {}", safe_text(&job.statement))];
        if let Some(Ok(result)) = job.result() {
            if result.affected.is_none() {
                lines.push(format!(
                    "{} řádků{} • {} ms",
                    result.rows.len(),
                    if result.truncated {
                        " (prvních 1000)"
                    } else {
                        ""
                    },
                    result.elapsed_ms
                ));
                if let Some(row) = self
                    .selected
                    .checked_sub(1)
                    .and_then(|i| result.rows.get(i))
                {
                    for (column, value) in result.columns.iter().zip(row) {
                        lines.push(format!(
                            "{}: {}",
                            safe_text(column),
                            value
                                .as_deref()
                                .map(safe_text)
                                .unwrap_or_else(|| "NULL".into())
                        ));
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn performance_hint(&self) -> String {
        match self.session.performance.as_ref().and_then(|p| p.source.clone()) {
            Some(StatsSource::Off) => "Server nesbírá statistiky dotazů. F10 → Výkon: zapnout log pomalých dotazů (platí do restartu), nebo trvale performance_schema = ON v konfiguraci.".into(),
            Some(StatsSource::SlowLogFile(file)) if self.session.host.is_none() => format!(
                "Slow log je soubor {} na serveru: nastavte v profilu přístup k serveru, nebo log_output = TABLE.",
                safe_text(&file)
            ),
            Some(StatsSource::SlowLogFile(_)) => self
                .session
                .host_error
                .clone()
                .unwrap_or_else(|| "Čtu slow log ze serveru…".into()),
            _ => String::new(),
        }
    }
    fn locks_text(&self) -> String {
        let Some(report) = &self.session.locks else {
            return String::new();
        };
        let mut lines: Vec<String> = report
            .notes
            .iter()
            .map(|n| format!("! {}", safe_text(n)))
            .collect();
        if !report.waits.is_empty() {
            lines.push(format!(
                "{} dotazů čeká na zámek • kořenové blokující spojení: {} (označené !)",
                report.waits.len(),
                report
                    .root_blockers
                    .iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for w in &report.metadata_waits {
            lines.push(format!(
                "Čeká na zámek metadat: #{} {} s {}",
                w.thread,
                w.seconds,
                safe_text(&w.query)
            ));
        }
        if let Some(t) = self.transactions().get(self.selected) {
            lines.push(format!(
                "#{} {}@{} db {} • {} • {} s • zamčeno {} změněno {} • {}",
                t.thread,
                safe_text(&t.user),
                safe_text(&t.host),
                safe_text(&t.database),
                if t.idle() {
                    "nečinná relace s otevřenou transakcí"
                } else {
                    &t.state
                },
                t.seconds,
                t.rows_locked,
                t.rows_modified,
                safe_text(&t.isolation)
            ));
            for w in report
                .waits
                .iter()
                .filter(|w| w.blocking_thread == t.thread)
            {
                lines.push(format!(
                    "  blokuje #{} ({} s, {} {}): {}",
                    w.waiting_thread,
                    w.waiting_seconds,
                    safe_text(&w.lock_table),
                    safe_text(&w.lock_mode),
                    safe_text(&w.waiting_query)
                ));
            }
            lines.push("F10: ukončit dotaz nebo celé spojení".into());
        }
        lines.join("\n")
    }
    fn filter(&self) -> &str {
        &self.filters[self.page]
    }
    fn row_keys(&self) -> Vec<RowKey> {
        match self.page {
            DATABASES => self
                .databases()
                .iter()
                .map(|d| RowKey::Database(d.name.clone()))
                .collect(),
            ACCOUNTS => self
                .accounts()
                .iter()
                .map(|a| RowKey::Account((*a).clone()))
                .collect(),
            PROCESSES if self.locks_view => self
                .transactions()
                .iter()
                .map(|t| RowKey::Process(t.thread))
                .collect(),
            PROCESSES => self
                .processes()
                .iter()
                .map(|p| RowKey::Process(p.id))
                .collect(),
            PERFORMANCE => self
                .statements()
                .into_iter()
                .map(|s| RowKey::Variable(s.query))
                .collect(),
            REPLICATION => self
                .replication_rows()
                .into_iter()
                .map(|(k, _)| RowKey::Variable(k))
                .collect(),
            CONSOLE => (0..self.console_rows().len()).map(RowKey::Log).collect(),
            COMPARE => self
                .differences()
                .into_iter()
                .map(|d| RowKey::Variable(d.object))
                .collect(),
            VARIABLES => self
                .variables()
                .iter()
                .map(|v| RowKey::Variable(v.name.clone()))
                .collect(),
            HEALTH => self
                .findings()
                .iter()
                .map(|f| RowKey::Finding(f.title.clone()))
                .collect(),
            BACKUPS => self
                .backups
                .iter()
                .map(|b| RowKey::Backup(b.path.clone()))
                .collect(),
            SERVICE => (0..self.log_lines().len()).map(RowKey::Log).collect(),
            PROFILES => self
                .settings
                .profiles
                .iter()
                .map(|p| RowKey::Profile(p.id.clone()))
                .collect(),
            _ => Vec::new(),
        }
    }
    fn remember_selection(&mut self) {
        self.selected_key = self.row_keys().get(self.selected).cloned();
    }
    /// After a refresh the list can reorder (processes sort by time): keep the cursor
    /// on the same row instead of the same index, so actions hit what the user chose.
    fn reconcile_selection(&mut self) {
        let keys = self.row_keys();
        if keys.is_empty() {
            return;
        }
        if let Some(index) = self
            .selected_key
            .as_ref()
            .and_then(|key| keys.iter().position(|k| k == key))
        {
            self.selected = index;
            return;
        }
        if self.page != PROFILES {
            self.selected = self.selected.min(keys.len() - 1);
        }
        self.selected_key = keys.get(self.selected).cloned();
    }
    /// Selection, details and account-bound drafts belong to one server; after a
    /// connect/demo/disconnect they would silently target the new server.
    fn sync_server_scope(&mut self) {
        let generation = self.session.worker.generation();
        let switched = generation != self.scope_generation;
        if switched {
            self.scope_generation = generation;
            self.selected = 0;
            self.selected_key = None;
            self.horizontal = 0;
            self.detail.clear();
            self.detail_scroll = 0;
            self.table_cursor = 0;
            self.table_overlay = false;
            if matches!(
                self.confirm.as_ref().map(|c| &c.action),
                Some(Pending::Server(_))
            ) {
                self.confirm = None;
            }
        }
        let accounts = self.session.snapshot.as_ref().map(|s| s.accounts.clone());
        let missing = |a: &Account| accounts.as_ref().is_some_and(|list| !list.contains(a));
        let stale =
            |kind: &Kind| (switched && kind.server_bound()) || kind.account().is_some_and(missing);
        let mut dropped = false;
        for slot in [&mut self.dialog, &mut self.suspended] {
            if slot.as_ref().is_some_and(|d| stale(&d.kind)) {
                *slot = None;
                dropped = true;
            }
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|p| switched || missing(&p.account))
        {
            self.picker = None;
            dropped = true;
        }
        if let Some(Pending::Server(change)) = self.confirm.as_ref().map(|c| &c.action) {
            let account = match change {
                Change::ChangePassword { account, .. }
                | Change::Privileges { account, .. }
                | Change::RoleMembership { account, .. }
                | Change::DefaultRole { account, .. }
                | Change::AlterAccount { account, .. }
                | Change::DropAccount { account } => Some(account),
                _ => None,
            };
            if account.is_some_and(missing) {
                self.confirm = None;
                dropped = true;
            }
        }
        if dropped {
            self.say(
                "Rozpracovaná změna byla zahozena: patří jinému serveru nebo cíl už neexistuje.",
            );
        }
    }
    fn say(&mut self, message: impl Into<String>) {
        self.status = message.into();
        self.status_at = std::time::Instant::now();
    }
    /// The newest of: own status, session error, connection test result. A stale error
    /// or test result must not hide newer messages such as form validation errors.
    fn status_line(&self) -> &str {
        let session = &self.session;
        [
            Some((self.status_at, self.status.as_str())),
            session.error.as_deref().map(|e| (session.error_at, e)),
            session
                .test_result
                .as_deref()
                .map(|t| (session.test_result_at, t)),
        ]
        .into_iter()
        .flatten()
        .filter(|(_, text)| !text.is_empty())
        .max_by_key(|(at, _)| *at)
        .map(|(_, text)| text)
        .unwrap_or("")
    }
    fn try_save<T>(&mut self, change: impl FnOnce(&mut Settings) -> Result<T>) -> Result<T> {
        let (settings, value) = self
            .store
            .as_ref()
            .ok_or_else(|| Error::Config("Úložiště není dostupné.".into()))?
            .try_update(change)?;
        self.settings = settings;
        Ok(value)
    }
    fn save(&mut self, change: impl FnOnce(&mut Settings)) {
        match self
            .store
            .as_ref()
            .ok_or_else(|| Error::Config("Úložiště není dostupné.".into()))
            .and_then(|s| s.update(change))
        {
            Ok(settings) => self.settings = settings,
            Err(e) => self.say(e.to_string()),
        }
    }
    fn next_theme(&mut self) {
        let i = ThemeId::TERMINAL
            .iter()
            .position(|t| *t == self.theme)
            .unwrap_or(0);
        self.theme = ThemeId::TERMINAL[(i + 1) % ThemeId::TERMINAL.len()];
        let key = self.theme.key();
        self.save(|s| s.tui_theme = key.into());
    }
    fn palette(&self) -> Palette {
        let mode = if self.colors == "auto" {
            if std::env::var_os("NO_COLOR").is_some() {
                "mono"
            } else if std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit") {
                "truecolor"
            } else {
                "16"
            }
        } else {
            &self.colors
        };
        if mode == "mono" {
            return Palette {
                base: Style::default(),
                menu: Style::default().add_modifier(Modifier::REVERSED),
                selected: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                border: Color::Reset,
                accent: Color::Reset,
            };
        }
        let colors = terminal(self.theme, mode == "16");
        let c = |role: &str| -> Color {
            let value = &colors[role];
            if value.starts_with('#') {
                let [r, g, b] = rgb(value);
                Color::Rgb(r, g, b)
            } else {
                match value.as_str() {
                    "White" => Color::White,
                    "Black" => Color::Black,
                    "DarkGrey" => Color::DarkGray,
                    "Grey" => Color::Gray,
                    "DarkBlue" => Color::Blue,
                    "DarkCyan" => Color::Cyan,
                    "DarkGreen" => Color::Green,
                    "DarkRed" => Color::Red,
                    "Cyan" => Color::LightCyan,
                    "Green" => Color::LightGreen,
                    "Yellow" => Color::Yellow,
                    "Red" => Color::LightRed,
                    _ => Color::White,
                }
            }
        };
        Palette {
            base: Style::default().fg(c("foreground")).bg(c("background")),
            menu: Style::default()
                .fg(c("menu_foreground"))
                .bg(c("menu_background")),
            selected: Style::default()
                .fg(c("selected_foreground"))
                .bg(c("selected_background"))
                .add_modifier(Modifier::BOLD),
            border: c("border"),
            accent: c("accent"),
        }
    }
    fn block<'a>(&self, title: impl Into<Line<'a>>, p: Palette) -> Block<'a> {
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(p.border))
            .style(p.base);
        if self.ascii {
            block.border_set(ratatui::symbols::border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            })
        } else {
            block
        }
    }
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let p = self.palette();
        frame.render_widget(Block::default().style(p.base), area);
        if area.width < 80 || area.height < 24 {
            frame.render_widget(Paragraph::new("Harbor potřebuje alespoň 80 × 24 znaků. Zvětšete terminál. Ctrl+C ukončí aplikaci bezpečně.").wrap(Wrap { trim: false }).style(p.base), area);
            return;
        }
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(area);
        frame.render_widget(
            Paragraph::new(page_bar(self.page, rows[0].width as usize)).style(p.menu),
            rows[0],
        );
        let profile = self
            .session
            .profile
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("bez spojení");
        frame.render_widget(
            Paragraph::new(format!(
                " Harbor / {} • {} • {}\n {}",
                safe_text(profile),
                if self.session.worker.busy {
                    "PRACUJI…"
                } else {
                    &self.session.status
                },
                self.theme.label(),
                self.session
                    .profile
                    .as_ref()
                    .map(
                        |p| if self.session.snapshot.as_ref().is_some_and(|s| s.demo) {
                            "DEMO • žádné síťové spojení"
                        } else {
                            p.security_label()
                        }
                    )
                    .unwrap_or("Připojení vyberte přes F3")
            ))
            .style(p.base),
            rows[1],
        );
        match self.page {
            SERVER => self.overview(frame, rows[2], p),
            SETTINGS => self.draw_settings(frame, rows[2], p),
            _ => self.draw_list(frame, rows[2], p),
        }
        let status = self.status_line();
        frame.render_widget(
            Paragraph::new(safe_text(status))
                .wrap(Wrap { trim: false })
                .style(p.base),
            rows[3],
        );
        frame.render_widget(
            Paragraph::new(
                " F1 Pomoc  F3 Profily  F5 Obnovit  F10/F9/Alt+M Menu  1–9,0 Obrazovky  Esc",
            )
            .style(p.menu),
            rows[4],
        );
        if self.table_overlay {
            self.draw_table_overlay(frame, p);
        }
        if let Some(dialog) = &self.dialog {
            self.draw_dialog(frame, dialog, p);
        }
        if let Some(picker) = &self.picker {
            self.draw_picker(frame, picker, p);
        }
        if self.help {
            let area = centered(area, 76, 22);
            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new("F10, F9 nebo Alt+M: všechny akce v nabídce\n1–9 a 0: přímo na obrazovku (Server … Nastavení)\nSlužba (7): stav, restart, log, konfigurace; vyžaduje přístup k serveru v profilu\nZálohy: F10 → Záloha: vytvořit / ověřit / obnovit; průběh vidíte v detailu\nŠipky / PageUp / PageDown: výběr a posuv\nEnter: otevřít detail, upravit profil nebo nastavení\nTab: přepnout na detail (Databáze: výběr tabulky, Enter = struktura)\nVlevo/vpravo: vodorovný posuv seznamu\nEsc: zavřít; formulář zůstane rozpracovaný\nF3: profily; F5: obnovit (na Doktorovi znovu zkontrolovat)\n\nOprávnění: mezerník zaškrtne, Enter na Zkontrolovat ukáže SQL.\nZápisy vyžadují potvrzení s náhledem SQL; výchozí je Zrušit.\nMazání vyžaduje opsat název objektu.\nSET GLOBAL platí do restartu; trvale: Proměnná → uložit do konfigurace.\nDEMO je simulace bez skutečného serveru.\nTCP standardně vyžaduje ověřenou identitu TLS.\n\nEsc zavře nápovědu.").block(self.block(" Nápověda ", p)).style(p.base).wrap(Wrap { trim: false }), area);
        }
        if let Some(index) = self.menu {
            let area = centered(area, 64, 26);
            frame.render_widget(Clear, area);
            let items: Vec<_> = menu_items(self.page)
                .into_iter()
                .map(|(label, _)| ListItem::new(label))
                .collect();
            let mut state = ListState::default().with_selected(Some(index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(self.block(
                        format!(" Nabídka • {} nahoře • Enter / Esc ", PAGES[self.page]),
                        p,
                    ))
                    .highlight_style(p.selected)
                    .highlight_symbol("> "),
                area,
                &mut state,
            );
        }
        if let Some(confirm) = &self.confirm {
            self.draw_confirm(frame, confirm, profile, p);
        }
    }
    fn draw_confirm(&self, frame: &mut Frame, confirm: &Confirmation, profile: &str, p: Palette) {
        let mut lines: Vec<String> = Vec::new();
        match &confirm.action {
            Pending::Server(change) => {
                lines.push(format!("Server: {}", safe_text(profile)));
                lines.push(safe_text(&change.summary()));
                if let Change::KillQuery { target } = change {
                    lines.push(format!("#{} {}", target.id, safe_text(&target.query)));
                }
                match preview(change) {
                    Ok(sql) => {
                        lines.push("Provede se:".into());
                        lines.extend(sql.lines().map(|l| format!("  {}", safe_text(l))));
                    }
                    Err(error) => lines.push(format!("NELZE PROVÉST: {error}")),
                }
                lines.push(
                    if self.session.snapshot.as_ref().is_some_and(|s| s.demo) {
                        "DEMO • pouze paměť"
                    } else {
                        "Změna na připojeném serveru"
                    }
                    .into(),
                );
            }
            Pending::DeleteProfile { id } => {
                lines.push(format!(
                    "Odstranit uložený profil „{}“?",
                    self.settings
                        .profile(id)
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "(už neexistuje)".into())
                ));
                lines.push("Změní se jen konfigurace Harboru, server ne.".into());
            }
            Pending::Restore(r) => {
                lines.push(format!("Server: {}", safe_text(profile)));
                lines.push(safe_text(&JobSpec::Restore(r.clone()).title()));
                lines.push(
                    "Nejdřív se ověří kontrolní součet, pak se databáze vytvoří a naplní.".into(),
                );
                lines.push(if r.replace {
                    "Stávající databáze se nejdřív SMAŽE.".into()
                } else {
                    "Cílová databáze nesmí existovat.".into()
                });
            }
            Pending::DeleteBackup(path) => {
                lines.push(format!(
                    "Smazat soubor zálohy {}?",
                    safe_text(&path.display().to_string())
                ));
            }
            Pending::Schedule(plan) => {
                lines.push(format!(
                    "Naplánovat zálohu profilu {} ({}), uchovávat {}:",
                    safe_text(&plan.profile_name),
                    safe_text(&plan.calendar),
                    plan.keep
                ));
                match schedule::current_binary()
                    .and_then(|binary| schedule::render_units(plan, &binary))
                {
                    Ok((service, timer)) => {
                        lines.extend(
                            service
                                .lines()
                                .chain(timer.lines())
                                .filter(|l| {
                                    l.starts_with("ExecStart") || l.starts_with("OnCalendar")
                                })
                                .map(|l| format!("  {}", safe_text(l))),
                        );
                        lines.push(format!("Soubory: {}", schedule::unit_dir().display()));
                    }
                    Err(error) => lines.push(format!("NELZE: {error}")),
                }
            }
            Pending::Unschedule(name) => {
                lines.push(format!(
                    "Odebrat plán {}? Časovač se zastaví, zálohy zůstanou.",
                    safe_text(name)
                ));
            }
            Pending::Service(action, unit) => {
                lines.push(format!(
                    "Server: {} • služba {}",
                    safe_text(profile),
                    safe_text(unit)
                ));
                lines.push(match action {
                    ServiceAction::Start => "Služba se spustí.".to_string(),
                    _ => format!(
                        "{}: všech {} spojení se přeruší, běžící transakce se vrátí zpět.",
                        if *action == ServiceAction::Stop {
                            "ZASTAVENÍ"
                        } else {
                            "Restart"
                        },
                        self.session.snapshot.as_ref().map_or(0, |s| s.connections)
                    ),
                });
            }
            Pending::ConsoleWrite(database, sql) => {
                lines.push(format!(
                    "Server: {} • databáze {}",
                    safe_text(profile),
                    if database.is_empty() {
                        "—".to_string()
                    } else {
                        safe_text(database)
                    }
                ));
                lines.push("Zápisový příkaz se provede tak, jak je napsaný:".into());
                lines.push(format!("  {}", safe_text(sql)));
            }
            Pending::WriteConfig(entries) => {
                lines.push(
                    "Nový obsah souboru Harboru (ověří se binárkou serveru nanečisto):".into(),
                );
                match host::render_config(entries, "…") {
                    Ok(text) => lines.extend(
                        text.lines()
                            .filter(|l| !l.starts_with('#'))
                            .map(|l| format!("  {}", safe_text(l))),
                    ),
                    Err(error) => lines.push(format!("NELZE: {error}")),
                }
                lines.push("Projeví se po restartu; předchozí soubor zůstane jako .bak.".into());
            }
        }
        if let Some(name) = confirm.required_name() {
            lines.push(String::new());
            lines.push(format!(
                "NEVRATNÉ. Pro potvrzení napište název: {}",
                safe_text(name)
            ));
            lines.push(format!("> {}_", safe_text(&confirm.typed)));
        }
        lines.push(String::new());
        lines.push(
            if confirm.yes {
                "  Zrušit       [ PROVÉST ZMĚNU ]"
            } else {
                "[ ZRUŠIT ]      Provést změnu"
            }
            .into(),
        );
        lines.push("Tab / šipky změní volbu, Enter potvrdí, Esc zruší".into());
        let area = centered(frame.area(), 78, (lines.len() + 4) as u16);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines.join("\n"))
                .wrap(Wrap { trim: false })
                .block(self.block(" Potvrzení změny ", p))
                .style(p.base),
            area,
        );
    }
    fn overview(&self, frame: &mut Frame, area: Rect, p: Palette) {
        let Some(s) = &self.session.snapshot else {
            frame.render_widget(Paragraph::new("Nejprve vyberte profil (F3) nebo otevřete DEMO v nabídce (F10). Žádné automatické spojení se serverem.").wrap(Wrap { trim: false }).block(self.block(" Přehled serveru ", p)), area);
            return;
        };
        let rows = Layout::vertical([
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Min(1),
        ])
        .split(area);
        let rate = self
            .session
            .qps
            .map(|q| format!("{q:.1}"))
            .unwrap_or_else(|| "čekám na druhý vzorek".into());
        let text = format!("MariaDB {} | Uptime {} dní {} h\nRelace {}/{} | {} databází | {}\nDotazy klientů/s (bez Harboru): {} | Dlouhé dotazy: {}\nOS: Nezjištěno | Časové pásmo: {}", safe_text(&s.version), s.uptime/86400, s.uptime/3600%24, s.connections, s.max_connections, s.databases.len(), bytes_label(s.bytes()), rate, s.long_queries(), safe_text(&s.timezone));
        frame.render_widget(
            Paragraph::new(text)
                .block(self.block(" Přehled serveru ", p))
                .style(p.base),
            rows[0],
        );
        let data: Vec<u64> = self
            .session
            .history
            .iter()
            .map(|q| q.round() as u64)
            .collect();
        frame.render_widget(
            Sparkline::default()
                .data(&data)
                .block(self.block(" Dotazy klientů/s • bez dotazů Harboru • od připojení ", p))
                .style(Style::default().fg(p.accent)),
            rows[1],
        );
        frame.render_widget(
            Paragraph::new(
                s.notices
                    .iter()
                    .map(|n| safe_text(n))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .wrap(Wrap { trim: false })
            .block(self.block(" Informace / omezení ", p))
            .style(p.base),
            rows[2],
        );
    }
    fn databases(&self) -> Vec<&Database> {
        self.session
            .snapshot
            .as_ref()
            .map(|s| {
                s.databases
                    .iter()
                    .filter(|d| {
                        (self.system || !d.system())
                            && matches_filter(self.filter(), &[&d.name, &d.charset, &d.collation])
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn accounts(&self) -> Vec<&Account> {
        self.session
            .snapshot
            .as_ref()
            .map(|s| {
                s.accounts
                    .iter()
                    .filter(|a| matches_filter(self.filter(), &[&a.user, &a.host, &a.plugin]))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn processes(&self) -> Vec<&Process> {
        self.session
            .snapshot
            .as_ref()
            .map(|s| {
                s.processes
                    .iter()
                    .filter(|p| {
                        (!self.long || p.long_query())
                            && matches_filter(
                                self.filter(),
                                &[&p.user, &p.host, &p.database, &p.command, &p.query],
                            )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn variables(&self) -> Vec<&Variable> {
        self.session
            .variables
            .as_ref()
            .map(|list| {
                list.iter()
                    .filter(|v| {
                        (!self.key_only || v.key_description().is_some())
                            && (!self.changed_only || v.changed())
                            && matches_filter(
                                &self.filters[VARIABLES],
                                &[&v.name, &v.value, &v.comment],
                            )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    fn findings(&self) -> Vec<&Finding> {
        self.session
            .health
            .as_ref()
            .map(|(_, list)| {
                list.iter()
                    .filter(|f| matches_filter(&self.filters[HEALTH], &[&f.title, &f.detail]))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn list_rows(&self) -> Vec<String> {
        match self.page {
            DATABASES => self
                .databases()
                .iter()
                .map(|d| {
                    format!(
                        "{:<22} {:>5} tab. {:>14}  {}  {}",
                        safe_text(&d.name),
                        d.tables,
                        bytes_label(d.bytes),
                        safe_text(&d.charset),
                        safe_text(&d.collation)
                    )
                })
                .collect(),
            ACCOUNTS => self
                .accounts()
                .iter()
                .map(|a| {
                    format!(
                        "{:<24} {:<24} {}",
                        safe_text(&a.user),
                        if a.is_role() {
                            "(role)".into()
                        } else {
                            safe_text(&a.host)
                        },
                        safe_text(&a.plugin)
                    )
                })
                .collect(),
            PROCESSES if self.locks_view => {
                let roots = self
                    .session
                    .locks
                    .as_ref()
                    .map(|r| r.root_blockers.clone())
                    .unwrap_or_default();
                self.transactions()
                    .iter()
                    .map(|t| {
                        format!(
                            "{}#{:<7} {:<16} {:<10} {:>5}s zamčeno {:>6} {}",
                            if roots.contains(&t.thread) { "!" } else { " " },
                            t.thread,
                            safe_text(&t.user),
                            if t.idle() {
                                "nečinná".to_string()
                            } else {
                                safe_text(&t.state)
                            },
                            t.seconds,
                            t.rows_locked,
                            safe_text(&t.query)
                        )
                    })
                    .collect()
            }
            PERFORMANCE => self
                .statements()
                .iter()
                .map(|s| {
                    format!(
                        "{:>8.1} s {:>7}× {:>7.0} ms  {}",
                        s.total_seconds,
                        s.count,
                        s.average_ms(),
                        safe_text(&s.query)
                    )
                })
                .collect(),
            REPLICATION => self
                .replication_rows()
                .into_iter()
                .map(|(_, r)| r)
                .collect(),
            CONSOLE => self.console_rows(),
            COMPARE => self
                .differences()
                .iter()
                .map(|d| {
                    format!(
                        "{:<9} {:<10} {}",
                        d.area.label(),
                        d.kind.label(),
                        safe_text(&d.object)
                    )
                })
                .collect(),
            PROCESSES => self
                .processes()
                .iter()
                .map(|p| {
                    format!(
                        "#{:<7} {:<20} {:<18} {:>5}s {:<8} {}",
                        p.id,
                        safe_text(&p.user),
                        safe_text(&p.database),
                        p.seconds,
                        safe_text(&p.command),
                        safe_text(&p.query)
                    )
                })
                .collect(),
            VARIABLES => self
                .variables()
                .iter()
                .map(|v| {
                    format!(
                        "{:<34} {:<26} {}",
                        safe_text(&v.name),
                        safe_text(&v.value),
                        if v.runtime_only() {
                            "ZMĚNĚNO ZA BĚHU"
                        } else if v.changed() {
                            "nastaveno"
                        } else {
                            ""
                        }
                    )
                })
                .collect(),
            HEALTH => self
                .findings()
                .iter()
                .map(|f| format!("[{:<8}] {}", f.severity.label(), safe_text(&f.title)))
                .collect(),
            BACKUPS => self
                .backups
                .iter()
                .map(|b| match &b.manifest {
                    Ok(m) => format!(
                        "{:<34} {} {:>10}  {}{}",
                        safe_text(&b.name()),
                        m.created_utc,
                        human_bytes(b.bytes),
                        safe_text(
                            &m.databases
                                .iter()
                                .map(|d| d.name.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        if m.encrypted { " [šifr.]" } else { "" }
                    ),
                    Err(_) => format!(
                        "{:<34} nečitelná / není záloha Harboru",
                        safe_text(&b.name())
                    ),
                })
                .collect(),
            SERVICE => self.log_lines().iter().map(|l| safe_text(l)).collect(),
            PROFILES => self
                .settings
                .profiles
                .iter()
                .map(|p| {
                    format!(
                        "{} • {} • {}",
                        safe_text(&p.name),
                        safe_text(&p.user),
                        p.security_label()
                    )
                })
                .chain(std::iter::once("+ Nový profil (Enter)".into()))
                .collect(),
            _ => Vec::new(),
        }
    }
    fn empty_hint(&self) -> Option<&'static str> {
        match self.page {
            VARIABLES if self.session.variables.is_none() => Some("Načítám proměnné…"),
            HEALTH if self.session.health.is_none() => Some("Probíhá kontrola…"),
            HEALTH if self.findings().is_empty() => Some("Bez nálezů."),
            PROCESSES if self.locks_view && self.session.locks.is_none() => {
                Some("Načítám transakce…")
            }
            PROCESSES if self.locks_view && self.transactions().is_empty() => {
                Some("Žádná otevřená transakce InnoDB.")
            }
            PERFORMANCE if self.session.performance.is_none() => Some("Načítám statistiky…"),
            PERFORMANCE if self.statements().is_empty() => {
                Some("Žádné statistiky dotazů (viz detail).")
            }
            REPLICATION if self.session.replication.is_none() => Some("Načítám…"),
            CONSOLE if self.session.console.is_none() => {
                Some("Enter nebo F10 → Konzole: spustit dotaz…")
            }
            COMPARE if self.session.compare.is_none() => {
                Some("Enter nebo F10 → Porovnání: spustit… (připojený server vs. uložený profil)")
            }
            COMPARE if self.differences().is_empty() => {
                Some("Žádné rozdíly (nebo porovnání běží).")
            }
            SERVICE if self.session.host.is_none() => {
                Some("Profil nemá přístup k serveru (F3 → profil → Přístup k serveru).")
            }
            SERVICE if self.session.host_logs.is_none() => Some("Načítám log…"),
            BACKUPS if self.backups.is_empty() => Some("Žádné zálohy. F10 → Záloha: vytvořit…"),
            _ => None,
        }
    }
    /// Text of the detail pane for the current page.
    fn detail_text(&self) -> String {
        match self.page {
            ACCOUNTS => match &self.session.account {
                Some(detail) => account_text(detail),
                None if self.session.grants_for.is_some() => "Načítám…".into(),
                None => "Enter: načíst oprávnění vybraného účtu.".into(),
            },
            VARIABLES => match self.variables().get(self.selected).copied() {
                Some(v) => variable_text(v),
                None => String::new(),
            },
            BACKUPS => self.backups_text(),
            SERVICE => self.service_text(),
            REPLICATION => self.replication_text(),
            CONSOLE => self.console_text(),
            COMPARE => self.compare_text(),
            PERFORMANCE => match self.statements().get(self.selected) {
                Some(s) => format!(
                    "{}\nschéma {} • {}× • celkem {:.2} s • průměr {:.1} ms • max {:.2} s • řádky prohledané/vrácené {}/{}{}",
                    safe_text(&s.query),
                    safe_text(&s.schema),
                    s.count,
                    s.total_seconds,
                    s.average_ms(),
                    s.max_seconds,
                    s.rows_examined,
                    s.rows_sent,
                    s.sample
                        .as_ref()
                        .map(|x| format!("\nNejpomalejší spuštění: {}", safe_text(x)))
                        .unwrap_or_default()
                ),
                None => self.performance_hint(),
            },
            PROCESSES if self.locks_view => self.locks_text(),
            HEALTH => match self.findings().get(self.selected) {
                Some(f) => format!(
                    "{} • {}\n{}\nDoporučení: {}",
                    f.severity.label(),
                    safe_text(&f.title),
                    safe_text(&f.detail),
                    safe_text(&f.recommendation)
                ),
                None => String::new(),
            },
            _ => self.detail.clone(),
        }
    }
    fn backups_text(&self) -> String {
        let mut lines = Vec::new();
        for job in self.session.jobs.iter().rev().take(4) {
            let progress = job.progress();
            lines.push(match &progress.result {
                None => format!(
                    "▶ {} • {} • {} %{}",
                    safe_text(&job.title),
                    safe_text(&progress.phase),
                    (progress.fraction() * 100.0) as u32,
                    if job.cancel_requested() {
                        " • ruší se"
                    } else {
                        ""
                    }
                ),
                Some(Ok(summary)) => format!("✓ {}", safe_text(summary)),
                Some(Err(error)) => format!("✗ {}: {}", safe_text(&job.title), safe_text(error)),
            });
        }
        if let Some(file) = self.backups.get(self.selected) {
            match &file.manifest {
                Ok(m) => {
                    lines.push(format!(
                        "{} • server {} • profil {} • {}{}",
                        safe_text(&file.name()),
                        safe_text(&m.server_version),
                        safe_text(&m.profile),
                        if m.consistent {
                            "konzistentní snímek"
                        } else {
                            "netransakční tabulky mimo snímek"
                        },
                        if m.encrypted { " • šifrováno" } else { "" }
                    ));
                    for db in &m.databases {
                        lines.push(format!(
                            "  {} • {} tabulek • {} řádků • SQL {}",
                            safe_text(&db.name),
                            db.tables,
                            db.rows,
                            human_bytes(db.bytes)
                        ));
                    }
                    lines.extend(m.warnings.iter().map(|w| format!("  ! {}", safe_text(w))));
                }
                Err(error) => {
                    lines.push(format!("{}: {}", safe_text(&file.name()), safe_text(error)))
                }
            }
        }
        lines.push(format!("Adresář: {}", self.settings.backup_dir().display()));
        for installed in &self.schedules {
            lines.push(format!(
                "Plán {} • {} • {}",
                safe_text(&installed.name),
                safe_text(&installed.calendar),
                match installed.active {
                    Some(true) => "aktivní",
                    Some(false) => "neaktivní",
                    None => "stav neznámý",
                }
            ));
        }
        lines.join("\n")
    }
    fn draw_list(&self, frame: &mut Frame, area: Rect, p: Palette) {
        let rows = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(if area.height > 23 { 11 } else { 7 }),
        ])
        .split(area);
        let mut rows_text = self.list_rows();
        if rows_text.is_empty() {
            if let Some(hint) = self.empty_hint() {
                rows_text.push(hint.into());
            }
        }
        let list: Vec<_> = rows_text
            .iter()
            .map(|r| ListItem::new(r.chars().skip(self.horizontal).collect::<String>()))
            .collect();
        let mut state = ListState::default().with_selected(Some(self.selected));
        let extra = match self.page {
            VARIABLES => format!(
                " • klíčové: {} • změněné: {}",
                if self.key_only { "ano" } else { "ne" },
                if self.changed_only { "ano" } else { "ne" }
            ),
            _ => String::new(),
        };
        frame.render_stateful_widget(
            List::new(list)
                .block(self.block(
                    format!(
                        " {} • filtr: {}{extra} • Enter detail ",
                        PAGES[self.page],
                        safe_text(self.filter())
                    ),
                    p,
                ))
                .highlight_style(p.selected)
                .highlight_symbol("> "),
            rows[0],
            &mut state,
        );
        let focus = if self.detail_focus {
            "> Detail • šipky • Tab zpět"
        } else {
            "Detail • Tab aktivuje • F10 akce"
        };
        if self.page == DATABASES {
            self.draw_database_detail(frame, rows[1], p, focus);
            return;
        }
        frame.render_widget(
            Paragraph::new(self.detail_text())
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0))
                .block(self.block(
                    format!(
                        " {}{} ",
                        match self.page {
                            ACCOUNTS => self
                                .session
                                .grants_for
                                .as_ref()
                                .map(|a| format!("Oprávnění {} • ", safe_text(&a.label())))
                                .unwrap_or_default(),
                            _ => String::new(),
                        },
                        focus
                    ),
                    p,
                ))
                .style(p.base),
            rows[1],
        );
    }
    fn draw_database_detail(&self, frame: &mut Frame, area: Rect, p: Palette, focus: &str) {
        let title = format!(
            " {}{} ",
            self.session
                .tables_for
                .as_deref()
                .map(|db| format!("Databáze {} • ", safe_text(db)))
                .unwrap_or_default(),
            if self.detail_focus {
                "> Tabulky • šipky • Enter struktura • Tab zpět"
            } else {
                focus
            }
        );
        let block = self.block(title, p);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let Some(detail) = &self.session.database else {
            frame.render_widget(
                Paragraph::new(if self.session.tables_for.is_some() {
                    "Načítám…"
                } else {
                    "Enter: načíst tabulky a velikosti vybrané databáze."
                })
                .style(p.base),
                inner,
            );
            return;
        };
        let mut summary = vec![format!(
            "{} tab. • {} pohl. • {} proc. • {} trig. • data {} • indexy {} • nevyužito {}",
            detail.base_tables().count(),
            detail.views,
            detail.routines,
            detail.triggers,
            human_bytes(detail.data_bytes()),
            human_bytes(detail.index_bytes()),
            human_bytes(detail.free_bytes())
        )];
        summary.extend(
            detail
                .warnings()
                .iter()
                .map(|w| format!("! {}", safe_text(w))),
        );
        let parts = Layout::vertical([
            Constraint::Length(summary.len().min(3) as u16),
            Constraint::Min(1),
        ])
        .split(inner);
        frame.render_widget(Paragraph::new(summary.join("\n")).style(p.base), parts[0]);
        let items: Vec<_> = detail
            .tables
            .iter()
            .map(|t| {
                ListItem::new(if t.view() {
                    format!("{:<28} pohled", safe_text(&t.name))
                } else {
                    format!(
                        "{:<28} {:<7} ≈{:>10} ř. {:>10} {}",
                        safe_text(&t.name),
                        safe_text(&t.engine),
                        t.rows,
                        human_bytes(t.bytes()),
                        t.warnings().join(", ")
                    )
                })
            })
            .collect();
        let mut state = ListState::default().with_selected(
            self.detail_focus
                .then_some(self.table_cursor.min(detail.tables.len().saturating_sub(1))),
        );
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(p.selected)
                .highlight_symbol("> ")
                .style(p.base),
            parts[1],
            &mut state,
        );
    }
    fn draw_table_overlay(&self, frame: &mut Frame, p: Palette) {
        let area = centered(
            frame.area(),
            frame.area().width.saturating_sub(4),
            frame.area().height.saturating_sub(4),
        );
        frame.render_widget(Clear, area);
        let (title, text) = match (&self.session.table, &self.session.table_for) {
            (Some(detail), _) => (
                format!(
                    " {}.{} ",
                    safe_text(&detail.database),
                    safe_text(&detail.table)
                ),
                table_text(detail),
            ),
            (None, Some((db, t))) => (
                format!(" {}.{} ", safe_text(db), safe_text(t)),
                "Načítám…".into(),
            ),
            _ => (" Tabulka ".into(), "Vyberte tabulku.".into()),
        };
        frame.render_widget(
            Paragraph::new(text)
                .scroll((self.overlay_scroll, 0))
                .block(self.block(
                    format!("{title}• šipky posouvají • F10 údržba • Esc zavře "),
                    p,
                ))
                .style(p.base),
            area,
        );
    }
    fn draw_settings(&self, frame: &mut Frame, area: Rect, p: Palette) {
        let items = [
            format!("Motiv: {} (Enter přepíná)", self.theme.label()),
            format!("Barvy: {} (auto / truecolor / 16 / mono)", self.colors),
            format!("Rámečky ASCII: {}", self.ascii),
            format!("Obnovování: {} sekund", self.settings.refresh_seconds),
        ];
        let mut state = ListState::default().with_selected(Some(self.selected.min(3)));
        frame.render_stateful_widget(
            List::new(items.map(ListItem::new))
                .block(self.block(" Nastavení • Enter změní / šipky vyberou ", p))
                .highlight_style(p.selected)
                .highlight_symbol("> "),
            area,
            &mut state,
        );
    }
    fn draw_dialog(&self, frame: &mut Frame, d: &Dialog, p: Palette) {
        let area = centered(frame.area(), 76, (d.fields.len() * 2 + 6) as u16);
        frame.render_widget(Clear, area);
        let block = self.block(" Formulář • Tab / Shift+Tab • Esc ponechá rozpracované ", p);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        // Long forms (profile) scroll so the focused field and the buttons stay visible.
        let capacity = ((inner.height as usize).saturating_sub(3) / 2).max(1);
        let start = if d.focus < d.fields.len() {
            d.focus.saturating_sub(capacity - 1)
        } else {
            d.fields.len().saturating_sub(capacity)
        };
        let end = (start + capacity).min(d.fields.len());
        let mut lines = Vec::new();
        if start > 0 {
            lines.push(Line::from(format!("  ↑ další pole ({start})")));
        }
        for (i, field) in d.fields.iter().enumerate().take(end).skip(start) {
            lines.push(Line::from(format!(
                "{}{}",
                if i == d.focus { "> " } else { "  " },
                field.label
            )));
            let text = if field.secret {
                "*".repeat(field.value.chars().count())
            } else {
                safe_text(&field.value)
            };
            lines.push(Line::styled(
                format!("  {text}"),
                if i == d.focus { p.selected } else { p.base },
            ));
        }
        if end < d.fields.len() {
            lines.push(Line::from(format!(
                "  ↓ další pole ({})",
                d.fields.len() - end
            )));
        } else {
            lines.push(Line::from(""));
        }
        let buttons = if matches!(d.kind, Kind::Profile(_)) {
            vec!["Zrušit", "Uložit", "Test", "Připojit"]
        } else {
            vec!["Zrušit", "Zkontrolovat"]
        };
        lines.push(Line::from(
            buttons
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if d.focus == d.fields.len() + i {
                        format!("[ {b} ]")
                    } else {
                        format!("  {b}  ")
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        ));
        frame.render_widget(Paragraph::new(lines).style(p.base), inner);
    }
    fn draw_picker(&self, frame: &mut Frame, picker: &PrivilegePicker, p: Palette) {
        let area = centered(frame.area(), 76, frame.area().height.saturating_sub(2));
        frame.render_widget(Clear, area);
        let block = self.block(
            format!(
                " Oprávnění {} • mezerník přepíná • Esc zavře ",
                safe_text(&picker.account.label())
            ),
            p,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let mut items = vec![ListItem::new(match (&picker.fixed, picker.scope()) {
            (Some(scope), _) => format!(
                "Rozsah: {}  (←/→ jiný existující, psaním nový)",
                safe_text(&scope.label())
            ),
            (None, Ok(_)) => format!(
                "Rozsah: {}_  (*.*, db.*, db.tabulka)",
                safe_text(&picker.scope_text)
            ),
            (None, Err(error)) => format!("Rozsah: {}_  ! {error}", safe_text(&picker.scope_text)),
        })];
        for privilege in picker.options() {
            items.push(ListItem::new(format!(
                "[{}] {privilege}",
                if picker.wanted.contains(privilege) {
                    "x"
                } else {
                    " "
                }
            )));
        }
        items.push(ListItem::new("  Zrušit"));
        items.push(ListItem::new("  Zkontrolovat změnu…"));
        let mut state = ListState::default().with_selected(Some(picker.cursor));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(p.selected)
                .highlight_symbol("> ")
                .style(p.base),
            inner,
            &mut state,
        );
    }
    pub fn key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.handle_key(key);
        self.remember_selection();
    }
    fn handle_key(&mut self, key: KeyEvent) {
        if key.code == K::Char('c') && key.modifiers.contains(M::CONTROL) {
            self.quit = true;
            return;
        }
        if self.confirm.is_some() {
            self.confirm_key(key);
            return;
        }
        if is_menu_key(&key) {
            self.menu = if self.menu.is_some() { None } else { Some(0) };
            return;
        }
        if let Some(index) = self.menu {
            let items = menu_items(self.page);
            match key.code {
                K::Esc => self.menu = None,
                K::Up => self.menu = Some(index.saturating_sub(1)),
                K::Down => self.menu = Some((index + 1).min(items.len() - 1)),
                K::PageUp => self.menu = Some(index.saturating_sub(8)),
                K::PageDown => self.menu = Some((index + 8).min(items.len() - 1)),
                K::Enter => {
                    if let Some(action) = items[index].1 {
                        self.menu = None;
                        self.run(action);
                    }
                }
                _ => (),
            }
            return;
        }
        if self.help {
            if matches!(key.code, K::Esc | K::F(1) | K::Enter) {
                self.help = false;
            }
            return;
        }
        if key.code == K::F(1) {
            self.help = true;
            return;
        }
        if self.dialog.is_some() {
            self.dialog_key(key);
            return;
        }
        if self.picker.is_some() {
            self.picker_key(key);
            return;
        }
        if self.table_overlay {
            match key.code {
                K::Esc | K::Enter => self.table_overlay = false,
                K::Down => self.overlay_scroll = self.overlay_scroll.saturating_add(1),
                K::Up => self.overlay_scroll = self.overlay_scroll.saturating_sub(1),
                K::PageDown => self.overlay_scroll = self.overlay_scroll.saturating_add(10),
                K::PageUp => self.overlay_scroll = self.overlay_scroll.saturating_sub(10),
                _ => (),
            }
            return;
        }
        match key.code {
            K::Char(c @ '1'..='9') if key.modifiers.is_empty() => {
                self.run(Action::Page(c as usize - '1' as usize));
            }
            K::Char('0') if key.modifiers.is_empty() => self.run(Action::Page(SETTINGS)),
            K::Char(']') => self.run(Action::Page((self.page + 1) % PAGES.len())),
            K::Char('[') => self.run(Action::Page((self.page + PAGES.len() - 1) % PAGES.len())),
            K::F(3) => self.run(Action::Page(PROFILES)),
            K::F(5) => {
                self.session.request(Operation::Refresh);
                if self.page == HEALTH {
                    self.session.request(Operation::Health);
                }
            }
            K::Tab | K::BackTab => {
                self.detail_focus = !self.detail_focus;
            }
            K::Down if self.detail_focus && self.page == DATABASES => {
                let count = self.session.tables().len();
                self.table_cursor = (self.table_cursor + 1).min(count.saturating_sub(1));
            }
            K::Up if self.detail_focus && self.page == DATABASES => {
                self.table_cursor = self.table_cursor.saturating_sub(1)
            }
            K::PageDown if self.detail_focus && self.page == DATABASES => {
                let count = self.session.tables().len();
                self.table_cursor = (self.table_cursor + 8).min(count.saturating_sub(1));
            }
            K::PageUp if self.detail_focus && self.page == DATABASES => {
                self.table_cursor = self.table_cursor.saturating_sub(8)
            }
            K::Enter if self.detail_focus && self.page == DATABASES => {
                self.run(Action::TableDetail)
            }
            K::Down if self.detail_focus => {
                self.detail_scroll = self.detail_scroll.saturating_add(1)
            }
            K::Up if self.detail_focus => self.detail_scroll = self.detail_scroll.saturating_sub(1),
            K::PageDown if self.detail_focus => {
                self.detail_scroll = self.detail_scroll.saturating_add(8)
            }
            K::PageUp if self.detail_focus => {
                self.detail_scroll = self.detail_scroll.saturating_sub(8)
            }
            K::Up => self.selected = self.selected.saturating_sub(1),
            K::Down => self.selected = (self.selected + 1).min(self.row_count().saturating_sub(1)),
            K::PageUp => self.selected = self.selected.saturating_sub(8),
            K::PageDown => {
                self.selected = (self.selected + 8).min(self.row_count().saturating_sub(1))
            }
            K::Right => self.horizontal += 8,
            K::Left => self.horizontal = self.horizontal.saturating_sub(8),
            K::Enter => self.enter(),
            K::Esc => {
                self.page = SERVER;
                self.detail.clear();
            }
            _ => (),
        }
    }
    fn row_count(&self) -> usize {
        if self.page == SETTINGS {
            4
        } else {
            self.list_rows().len()
        }
    }
    fn enter(&mut self) {
        self.detail_scroll = 0;
        match self.page {
            DATABASES => {
                if let Some(db) = self.databases().get(self.selected) {
                    let name = db.name.clone();
                    if self.session.tables_for.as_ref() != Some(&name) {
                        self.table_cursor = 0;
                    }
                    self.session.request(Operation::Tables(name));
                }
            }
            ACCOUNTS => {
                if let Some(account) = self.accounts().get(self.selected) {
                    let account = (*account).clone();
                    self.session.request(Operation::Grants(account));
                }
            }
            CONSOLE => self.run(Action::ConsoleRun),
            COMPARE if self.session.compare.is_none() => self.run(Action::CompareRun),
            COMPARE => self.detail_focus = true,
            PERFORMANCE | REPLICATION => self.detail_focus = true,
            PROCESSES if self.locks_view => self.detail_focus = true,
            PROCESSES => {
                if let Some(process) = self.processes().get(self.selected) {
                    self.detail = format!(
                        "#{} {}@{}\n{}",
                        process.id,
                        safe_text(&process.user),
                        safe_text(&process.host),
                        safe_text(&process.query)
                    );
                }
            }
            VARIABLES | HEALTH => self.detail_focus = true,
            PROFILES => self.profile_dialog(self.settings.profiles.get(self.selected).cloned()),
            SETTINGS => match self.selected.min(3) {
                0 => self.next_theme(),
                1 => {
                    let choices = ["auto", "truecolor", "16", "mono"];
                    let index = choices.iter().position(|c| *c == self.colors).unwrap_or(0);
                    self.colors = choices[(index + 1) % 4].into();
                    let colors = self.colors.clone();
                    self.save(|s| s.terminal_colors = colors);
                }
                2 => {
                    self.ascii = !self.ascii;
                    let ascii = self.ascii;
                    self.save(|s| s.ascii = ascii);
                }
                _ => {
                    let refresh = match self.settings.refresh_seconds {
                        1 => 5,
                        5 => 10,
                        10 => 30,
                        _ => 1,
                    };
                    self.save(|s| s.refresh_seconds = refresh);
                }
            },
            _ => (),
        }
    }
    /// Account loaded in the detail pane, if it is the selected one on Uživatelé.
    fn selected_account(&mut self) -> Option<Account> {
        if self.page != ACCOUNTS {
            self.say("Nejprve vyberte účet na obrazovce Uživatelé (3).");
            return None;
        }
        let account = self.accounts().get(self.selected).map(|a| (*a).clone());
        if account.is_none() {
            self.say("Nejprve vyberte účet na obrazovce Uživatelé (3).");
        }
        account
    }
    /// Detail of `account` if it is loaded (Enter on the account).
    fn loaded_detail(&mut self, account: &Account) -> Option<AccountDetail> {
        let detail = self
            .session
            .account
            .clone()
            .filter(|d| d.account.as_ref() == Some(account));
        if detail.is_none() {
            self.session.request(Operation::Grants(account.clone()));
            self.say("Načítám detail účtu; akci zopakujte, až se zobrazí.");
        }
        detail
    }
    fn selected_table(&mut self) -> Option<(String, TableInfo)> {
        let found = self
            .session
            .database
            .as_ref()
            .filter(|_| self.page == DATABASES)
            .and_then(|d| {
                d.tables
                    .get(self.table_cursor)
                    .map(|t| (d.name.clone(), t.clone()))
            });
        if found.is_none() {
            self.say("Vyberte databázi (Enter), pak Tab a tabulku šipkami.");
        }
        found
    }
    fn run(&mut self, action: Action) {
        if let Action::Page(page) = action {
            self.page = page;
            self.selected = 0;
            self.detail_focus = false;
            self.detail_scroll = 0;
            self.table_overlay = false;
            self.autoload();
            return;
        }
        if action == Action::NextTheme {
            self.next_theme();
            return;
        }
        if let Some(d) = self.dialog.take() {
            self.suspended = Some(d);
        }
        let dialog = |kind: Kind, fields: Vec<Field>| {
            Some(Dialog {
                kind,
                fields,
                focus: 0,
            })
        };
        match action {
            Action::Help => self.help = true,
            Action::Refresh => {
                self.session.request(Operation::Refresh);
            }
            Action::NewProfile => self.profile_dialog(None),
            Action::Demo => {
                self.session.connect(
                    Profile {
                        name: "Ukázkový server".into(),
                        ..Default::default()
                    },
                    Secret::default(),
                    true,
                );
                self.page = SERVER;
            }
            Action::Disconnect => self.session.disconnect(),
            Action::CreateDatabase => {
                self.dialog = dialog(
                    Kind::Database,
                    vec![
                        Field::new("Název databáze", ""),
                        Field::new("Kolace (ověří server)", "utf8mb4_unicode_ci"),
                    ],
                )
            }
            Action::DropDatabase => {
                let name = (self.page == DATABASES)
                    .then(|| self.databases().get(self.selected).map(|d| d.name.clone()))
                    .flatten();
                match name {
                    Some(name) if !Database::system_name(&name) => {
                        self.confirm =
                            Some(Confirmation::new(Pending::Server(Change::DropDatabase {
                                name,
                            })))
                    }
                    Some(_) => self.say("Systémovou databázi Harbor nesmaže."),
                    None => self.say("Databázi ke smazání vyberte na obrazovce Databáze (2)."),
                }
            }
            Action::TableDetail => {
                if let Some((database, table)) = self.selected_table() {
                    self.session.request(Operation::Table {
                        database,
                        table: table.name,
                    });
                    self.overlay_scroll = 0;
                    self.table_overlay = true;
                }
            }
            Action::Maintenance => {
                let target = if self.table_overlay {
                    self.session.table_for.clone()
                } else {
                    self.selected_table().map(|(db, t)| (db, t.name))
                };
                if let Some((database, table)) = target {
                    self.table_overlay = false;
                    self.dialog = dialog(
                        Kind::Maintenance(database),
                        vec![
                            Field::new("Tabulka", &table),
                            Field::new("Akce: ANALYZE / CHECK / OPTIMIZE / REPAIR", "ANALYZE"),
                        ],
                    );
                }
            }
            Action::CreateAccount => {
                self.dialog = dialog(
                    Kind::Account,
                    vec![
                        Field::new("Uživatel", ""),
                        Field::new("Hostitel účtu (% = odkudkoli)", "localhost"),
                        Field::password(),
                    ],
                )
            }
            Action::CreateRole => {
                self.dialog = dialog(Kind::Role, vec![Field::new("Název role", "")])
            }
            Action::ChangePassword => {
                if let Some(account) = self.selected_account() {
                    if account.is_role() {
                        self.say("Role nemá heslo.");
                    } else {
                        self.dialog = dialog(Kind::Password(account), vec![Field::password()]);
                    }
                }
            }
            Action::EditPrivileges => {
                if let Some(account) = self.selected_account() {
                    if let Some(detail) = self.loaded_detail(&account) {
                        let fixed = detail.privileges.first().map(|s| s.scope.clone());
                        self.picker = Some(PrivilegePicker {
                            account,
                            scope_text: if fixed.is_none() {
                                "*.*".into()
                            } else {
                                String::new()
                            },
                            fixed,
                            loaded_for: None,
                            wanted: BTreeSet::new(),
                            cursor: 0,
                        });
                        self.load_picker();
                    }
                }
            }
            Action::Template => {
                if let Some(account) = self.selected_account() {
                    self.dialog = dialog(
                        Kind::Template(account),
                        vec![
                            Field::new("Šablona: READONLY / APP / BACKUP / MONITOR", "READONLY"),
                            Field::new(
                                "Databáze (pro READONLY a APP)",
                                self.session.tables_for.as_deref().unwrap_or(""),
                            ),
                        ],
                    );
                }
            }
            Action::AccountProperties => {
                if let Some(account) = self.selected_account() {
                    if account.is_role() {
                        self.say("Role nemá vlastnosti přihlášení.");
                    } else if let Some(detail) = self.loaded_detail(&account) {
                        match detail.properties {
                            Some(props) => {
                                let yes = |b: bool| if b { "ANO" } else { "NE" };
                                self.dialog = dialog(
                                    Kind::Properties(account),
                                    vec![
                                        Field::new("Zamčen: ANO / NE", yes(props.locked)),
                                        Field::new("Nechat heslo vypršet: ANO / NE", "NE"),
                                        Field::new(
                                            "TLS: NONE / SSL / X509 (KEEP = neměnit)",
                                            match props.tls {
                                                TlsRequirement::None => "NONE",
                                                TlsRequirement::Ssl => "SSL",
                                                TlsRequirement::X509 => "X509",
                                                TlsRequirement::Specified => "KEEP",
                                            },
                                        ),
                                        Field::new(
                                            "Dotazy za hodinu (0 = bez limitu)",
                                            &props.limits.queries_per_hour.to_string(),
                                        ),
                                        Field::new(
                                            "Změny za hodinu",
                                            &props.limits.updates_per_hour.to_string(),
                                        ),
                                        Field::new(
                                            "Připojení za hodinu",
                                            &props.limits.connections_per_hour.to_string(),
                                        ),
                                        Field::new(
                                            "Souběžná spojení",
                                            &props.limits.user_connections.to_string(),
                                        ),
                                    ],
                                );
                            }
                            None => self.say(
                                "Vlastnosti účtu nejsou dostupné (chybí právo čtení mysql.*).",
                            ),
                        }
                    }
                }
            }
            Action::RoleMembership => {
                if let Some(account) = self.selected_account() {
                    self.dialog = dialog(
                        Kind::RoleMembership(account),
                        vec![
                            Field::new("Role", ""),
                            Field::new("Akce: GRANT / REVOKE", "GRANT"),
                        ],
                    );
                }
            }
            Action::DefaultRole => {
                if let Some(account) = self.selected_account() {
                    if account.is_role() {
                        self.say("Výchozí roli lze nastavit jen účtu.");
                    } else {
                        let current = self
                            .session
                            .account
                            .as_ref()
                            .filter(|d| d.account.as_ref() == Some(&account))
                            .and_then(|d| d.default_role.clone())
                            .unwrap_or_default();
                        self.dialog = dialog(
                            Kind::DefaultRole(account),
                            vec![Field::new("Výchozí role (prázdné = žádná)", &current)],
                        );
                    }
                }
            }
            Action::DropAccount => {
                if let Some(account) = self.selected_account() {
                    self.confirm = Some(Confirmation::new(Pending::Server(Change::DropAccount {
                        account,
                    })));
                }
            }
            Action::KillQuery => {
                if self.page != PROCESSES {
                    self.say("Nejprve vyberte běžící dotaz na obrazovce Spojení (4).");
                    return;
                }
                let target = self.processes().get(self.selected).map(|p| (*p).clone());
                if let Some(target) = target.filter(|p| p.running_query()) {
                    self.confirm = Some(Confirmation::new(Pending::Server(Change::KillQuery {
                        target,
                    })));
                } else {
                    self.say("Vyberte běžící dotaz na obrazovce Spojení (4).");
                }
            }
            Action::SetVariable => {
                let variable = (self.page == VARIABLES)
                    .then(|| self.variables().get(self.selected).map(|v| (*v).clone()))
                    .flatten();
                match variable {
                    Some(v) if v.settable() => {
                        let value = v.value.clone();
                        self.dialog = dialog(
                            Kind::Variable(v),
                            vec![Field::new(
                                "Nová hodnota (velikosti lze s příponou K/M/G)",
                                &value,
                            )],
                        );
                    }
                    Some(v) => self.say(format!(
                        "{} nelze měnit za běhu; patří do konfiguračního souboru.",
                        v.name
                    )),
                    None => self.say("Proměnnou vyberte na obrazovce Proměnné (5)."),
                }
            }
            Action::RunHealth => {
                self.session.request(Operation::Health);
                self.page = HEALTH;
            }
            Action::ToggleLocks => {
                self.locks_view = !self.locks_view;
                self.locks_requested = None;
                self.selected = 0;
                self.page = PROCESSES;
            }
            Action::KillConnection => {
                let thread = match self.page {
                    PROCESSES if self.locks_view => {
                        self.transactions().get(self.selected).map(|t| t.thread)
                    }
                    PROCESSES => self.processes().get(self.selected).map(|p| p.id),
                    _ => None,
                };
                let target = thread.and_then(|id| {
                    self.session
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.processes.iter().find(|p| p.id == id).cloned())
                });
                match target {
                    Some(target) => {
                        self.confirm =
                            Some(Confirmation::new(Pending::Server(Change::KillConnection {
                                target,
                            })))
                    }
                    None => self.say("Spojení vyberte na obrazovce Spojení (4)."),
                }
            }
            Action::PerformanceReload => {
                self.session.request(Operation::Performance);
                self.session.host_slow_log = None;
                self.page = PERFORMANCE;
            }
            Action::EnableSlowLog => {
                match self
                    .session
                    .variables
                    .as_ref()
                    .and_then(|v| v.iter().find(|v| v.name == "slow_query_log"))
                    .cloned()
                {
                    Some(variable) => {
                        self.confirm = Some(Confirmation::new(Pending::Server(Change::SetGlobal {
                            variable,
                            value: "ON".into(),
                        })))
                    }
                    None => {
                        self.session.request(Operation::Variables);
                        self.say("Načítám proměnné serveru; akci zopakujte.");
                    }
                }
            }
            Action::ResetStats => {
                if self
                    .session
                    .performance
                    .as_ref()
                    .is_some_and(|p| p.performance_schema)
                {
                    self.confirm = Some(Confirmation::new(Pending::Server(
                        Change::ResetStatementStats,
                    )));
                } else {
                    self.say("Vynulovat lze jen statistiky performance_schema.");
                }
            }
            Action::ExplainSample => {
                let stat = (self.page == PERFORMANCE)
                    .then(|| self.statements().get(self.selected).cloned())
                    .flatten();
                match stat.and_then(|s| s.sample.map(|sample| (s.schema, sample))) {
                    Some((schema, sample)) => {
                        self.page = CONSOLE;
                        self.dialog = dialog(
                            Kind::Console,
                            vec![
                                Field::new("Databáze (prázdné = žádná)", &schema),
                                Field::new("SQL (jeden příkaz)", &format!("EXPLAIN {sample}")),
                            ],
                        );
                    }
                    None => {
                        self.say("Vyberte dotaz se vzorkem (jen ze slow logu) na obrazovce Výkon.")
                    }
                }
            }
            Action::ReplicationReload => {
                self.session.request(Operation::Replication);
                self.page = REPLICATION;
            }
            Action::ReplicaStart | Action::ReplicaStop => {
                let key = (self.page == REPLICATION)
                    .then(|| {
                        self.replication_rows()
                            .get(self.selected)
                            .map(|(k, _)| k.clone())
                    })
                    .flatten();
                match key.as_deref().and_then(|k| k.strip_prefix("channel:")) {
                    Some(name) => {
                        self.confirm =
                            Some(Confirmation::new(Pending::Server(Change::ReplicaControl {
                                connection: name.to_string(),
                                start: action == Action::ReplicaStart,
                            })))
                    }
                    None => self.say("Vyberte replikační spojení na obrazovce Replikace."),
                }
            }
            Action::PurgeBinlogs => {
                let key = (self.page == REPLICATION)
                    .then(|| {
                        self.replication_rows()
                            .get(self.selected)
                            .map(|(k, _)| k.clone())
                    })
                    .flatten();
                match key.as_deref().and_then(|k| k.strip_prefix("binlog:")) {
                    Some(name) => {
                        self.confirm =
                            Some(Confirmation::new(Pending::Server(Change::PurgeBinlogs {
                                to: name.to_string(),
                            })))
                    }
                    None => self.say("Vyberte binární log na obrazovce Replikace."),
                }
            }
            Action::CompareRun => {
                let current = self
                    .session
                    .profile
                    .as_ref()
                    .map(|p| p.id.clone())
                    .unwrap_or_default();
                let first = self
                    .settings
                    .profiles
                    .iter()
                    .find(|p| p.id != current)
                    .map(|p| p.name.clone());
                match first {
                    Some(name) if self.session.snapshot.is_some() => {
                        self.page = COMPARE;
                        self.dialog = dialog(
                            Kind::Compare,
                            vec![
                                Field::new("Porovnat s profilem (název)", &name),
                                Field::hidden("Heslo (prázdné = uložené v úložišti)"),
                                Field::new("Oblasti: VŠE nebo PROMĚNNÉ,ÚČTY,SCHÉMA", "VŠE"),
                            ],
                        );
                    }
                    Some(_) => self.say("Nejprve se připojte k serveru, který chcete porovnat."),
                    None => self.say("Pro porovnání uložte alespoň jeden další profil (F3)."),
                }
            }
            Action::CompareExport => {
                if self
                    .session
                    .compare
                    .as_ref()
                    .and_then(|c| c.result())
                    .is_some_and(|r| r.is_ok())
                {
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                    let stamp: String = utc_timestamp(std::time::SystemTime::now())
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect();
                    self.dialog = dialog(
                        Kind::CompareExport,
                        vec![Field::new(
                            "Soubor CSV",
                            &format!("{home}/harbor-porovnani-{stamp}.csv"),
                        )],
                    );
                } else {
                    self.say("Nejdřív dokončete porovnání.");
                }
            }
            Action::ConsoleRun => {
                let (database, sql) = self
                    .session
                    .console
                    .as_ref()
                    .map(|c| (String::new(), c.statement.clone()))
                    .unwrap_or_default();
                let database = if database.is_empty() {
                    self.session.tables_for.clone().unwrap_or_default()
                } else {
                    database
                };
                self.page = CONSOLE;
                self.dialog = dialog(
                    Kind::Console,
                    vec![
                        Field::new("Databáze (prázdné = žádná)", &database),
                        Field::new("SQL (jeden příkaz)", &sql),
                    ],
                );
            }
            Action::ConsoleCancel => match &self.session.console {
                Some(job) if job.running() => {
                    job.cancel();
                    self.say("Ruším dotaz…");
                }
                _ => self.say("Žádný dotaz neběží."),
            },
            Action::ConsoleExport => {
                if self
                    .session
                    .console
                    .as_ref()
                    .and_then(|c| c.result())
                    .is_some_and(|r| r.is_ok())
                {
                    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                    let stamp: String = utc_timestamp(std::time::SystemTime::now())
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect();
                    self.dialog = dialog(
                        Kind::ConsoleExport,
                        vec![Field::new(
                            "Soubor CSV",
                            &format!("{home}/harbor-export-{stamp}.csv"),
                        )],
                    );
                } else {
                    self.say("Nejdřív spusťte dotaz s výsledkem.");
                }
            }
            Action::HostRefresh => {
                self.session.host_status = None;
                self.session.host_config = None;
                self.session.host_logs = None;
                self.session.host_error = None;
                self.page = SERVICE;
            }
            Action::ServiceStart | Action::ServiceRestart | Action::ServiceStop => {
                let Some(unit) = self
                    .session
                    .profile
                    .as_ref()
                    .filter(|p| p.host.enabled() || self.session.host.is_some())
                    .map(|p| p.host.service.clone())
                    .filter(|_| self.session.host.is_some())
                else {
                    self.say("Profil nemá přístup k serveru (F3 → profil → Přístup k serveru).");
                    return;
                };
                let service_action = match action {
                    Action::ServiceStart => ServiceAction::Start,
                    Action::ServiceRestart => ServiceAction::Restart,
                    _ => ServiceAction::Stop,
                };
                self.confirm = Some(Confirmation::new(Pending::Service(service_action, unit)));
            }
            Action::LogsSource => {
                self.log_from_file = !self.log_from_file;
                self.session.host_logs = None;
                self.selected = 0;
            }
            Action::LogsLevel => {
                self.log_level = (self.log_level + 1) % 3;
                self.selected = 0;
            }
            Action::ConfigRemove => match &self.session.host_config {
                Some(config) if !config.entries.is_empty() => {
                    let first = config.entries[0].0.clone();
                    self.dialog = dialog(
                        Kind::ConfigRemove,
                        vec![Field::new("Volba k odebrání z konfigurace Harboru", &first)],
                    );
                }
                Some(_) => self.say("Konfigurace Harboru je prázdná."),
                None => self.say("Konfigurace se načítá na obrazovce Služba (7)."),
            },
            Action::VariableToConfig => {
                if self.session.host.is_none() {
                    self.say("Profil nemá přístup k serveru (F3 → profil → Přístup k serveru).");
                    return;
                }
                let variable = (self.page == VARIABLES)
                    .then(|| self.variables().get(self.selected).map(|v| (*v).clone()))
                    .flatten();
                match variable {
                    Some(v) => {
                        let current = self
                            .session
                            .host_config
                            .as_ref()
                            .and_then(|c| c.entries.iter().find(|(n, _)| *n == v.name))
                            .map(|(_, value)| value.clone())
                            .unwrap_or_else(|| v.value.clone());
                        self.dialog = dialog(
                            Kind::ConfigValue(v.name.clone()),
                            vec![Field::new(
                                &format!("Hodnota {} v konfiguraci (po restartu)", v.name),
                                &current,
                            )],
                        );
                    }
                    None => self.say("Proměnnou vyberte na obrazovce Proměnné (5)."),
                }
            }
            Action::BackupStart => {
                if self.session.snapshot.as_ref().is_none_or(|s| s.demo) {
                    self.say("Zálohovat lze jen připojený skutečný server (ne demo).");
                    return;
                }
                self.dialog = dialog(
                    Kind::Backup,
                    vec![
                        Field::new(
                            "Databáze oddělené čárkou (prázdné = všechny uživatelské)",
                            self.session.tables_for.as_deref().unwrap_or(""),
                        ),
                        Field::new("Adresář", &self.settings.backup_dir().display().to_string()),
                        Field::hidden("Heslo pro šifrování (prázdné = bez šifrování)"),
                        Field::hidden("Heslo znovu"),
                    ],
                );
            }
            Action::BackupVerify => {
                if let Some(file) = self.selected_backup() {
                    if file.manifest.as_ref().is_ok_and(|m| m.encrypted) {
                        self.dialog =
                            dialog(Kind::Verify(file.path), vec![Field::hidden("Heslo zálohy")]);
                    } else {
                        self.session.start_job(JobSpec::Verify {
                            path: file.path,
                            encryption: None,
                        });
                    }
                }
            }
            Action::BackupRestore => {
                if let Some(file) = self.selected_backup() {
                    if self.session.snapshot.as_ref().is_none_or(|s| s.demo) {
                        self.say("Obnovit lze jen do připojeného skutečného serveru.");
                        return;
                    }
                    match &file.manifest {
                        Ok(m) => {
                            let first = m
                                .databases
                                .first()
                                .map(|d| d.name.clone())
                                .unwrap_or_default();
                            self.dialog = dialog(
                                Kind::Restore(file.path.clone()),
                                vec![
                                    Field::new(
                                        &format!(
                                            "Databáze ze zálohy ({})",
                                            m.databases
                                                .iter()
                                                .map(|d| d.name.as_str())
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        ),
                                        &first,
                                    ),
                                    Field::new("Obnovit do databáze (nový název = kopie)", &first),
                                    Field::new("Nahradit existující databázi: ANO / NE", "NE"),
                                    Field::hidden("Heslo zálohy (jen u šifrované)"),
                                ],
                            );
                        }
                        Err(error) => self.say(format!("Zálohu nelze číst: {error}")),
                    }
                }
            }
            Action::BackupDelete => {
                if let Some(file) = self.selected_backup() {
                    self.confirm = Some(Confirmation::new(Pending::DeleteBackup(file.path)));
                }
            }
            Action::JobCancel => match self.session.jobs.iter().rev().find(|j| !j.finished()) {
                Some(job) => {
                    job.cancel();
                    self.say(format!("Ruším: {}", job.title));
                }
                None => self.say("Žádná úloha neběží."),
            },
            Action::ScheduleCreate => {
                let Some(profile) = self.session.profile.clone() else {
                    self.say("Nejprve se připojte k serveru, jehož zálohy chcete plánovat.");
                    return;
                };
                if self.settings.profile(&profile.id).is_none() {
                    self.say("Plán potřebuje uložený profil (F3).");
                    return;
                }
                self.dialog = dialog(
                    Kind::Schedule,
                    vec![
                        Field::new(
                            "Kdy (systemd: daily, *-*-* 02:30, Mon..Fri 22:00)",
                            "*-*-* 02:30",
                        ),
                        Field::new("Uchovávat záloh", "7"),
                        Field::new("Databáze (prázdné = všechny uživatelské)", ""),
                        Field::new("Adresář", &self.settings.backup_dir().display().to_string()),
                        Field::new(
                            "Soubor s heslem serveru (chmod 600; prázdné = bez hesla)",
                            "",
                        ),
                        Field::new("Soubor s heslem šifrování (volitelné)", ""),
                    ],
                );
            }
            Action::ScheduleRemove => {
                let first = self.schedules.first().map(|s| s.name.clone());
                match first {
                    Some(name) => {
                        self.dialog =
                            dialog(Kind::Unschedule, vec![Field::new("Název plánu", &name)])
                    }
                    None => self.say("Žádný plán zálohy není nastaven."),
                }
            }
            Action::Search => {
                self.dialog = dialog(
                    Kind::Search(self.page),
                    vec![Field::new("Hledat", self.filter())],
                )
            }
            Action::ToggleSystem => self.system = !self.system,
            Action::ToggleLong => self.long = !self.long,
            Action::ToggleKeyVariables => {
                self.key_only = !self.key_only;
                self.selected = 0;
            }
            Action::ToggleChangedVariables => {
                self.changed_only = !self.changed_only;
                self.selected = 0;
            }
            Action::ResumeDraft => self.dialog = self.suspended.take(),
            Action::DeleteProfile => {
                if self.page != PROFILES {
                    self.say("Profil k odstranění vyberte na obrazovce Profily.");
                    return;
                }
                if let Some(profile) = self.settings.profiles.get(self.selected) {
                    self.confirm = Some(Confirmation::new(Pending::DeleteProfile {
                        id: profile.id.clone(),
                    }));
                }
            }
            Action::Quit => self.quit = true,
            Action::Page(_) | Action::NextTheme => unreachable!(),
        }
    }
    fn profile_dialog(&mut self, profile: Option<Profile>) {
        let original_id = profile.as_ref().map(|p| p.id.clone());
        let profile = profile.unwrap_or_default();
        let (ssh_target, ssh_port) = match &profile.host.access {
            HostAccess::Ssh { target, port } => (target.clone(), port.to_string()),
            _ => (String::new(), "22".to_string()),
        };
        let (host, port, socket) = match profile.transport.clone() {
            Transport::Tcp { host, port } => (host, port.to_string(), String::new()),
            Transport::Unix { path } => ("localhost".into(), "3306".into(), path),
        };
        self.dialog = Some(Dialog {
            kind: Kind::Profile(original_id),
            fields: vec![
                Field::new("Název profilu", &profile.name),
                Field::new("Hostitel TCP", &host),
                Field::new("Port", &port),
                Field::new("Uživatel", &profile.user),
                Field::password(),
                Field::new("Unix socket (prázdné = TCP)", &socket),
                Field::new("CA soubor (volitelné)", profile.ca.as_deref().unwrap_or("")),
                Field::new(
                    "TLS: VERIFY / INSECURE_TEST_ONLY",
                    if profile.tls == TlsMode::VerifyIdentity {
                        "VERIFY"
                    } else {
                        "INSECURE_TEST_ONLY"
                    },
                ),
                Field::new(
                    "Přístup k serveru: NONE / LOCAL / SSH",
                    match profile.host.access {
                        HostAccess::None => "NONE",
                        HostAccess::Local => "LOCAL",
                        HostAccess::Ssh { .. } => "SSH",
                    },
                ),
                Field::new("SSH cíl (uživatel@hostitel)", &ssh_target),
                Field::new("SSH port", &ssh_port),
                Field::new(
                    "Práva správce: SUDO / PKEXEC / NONE",
                    match profile.host.elevation {
                        Elevation::Sudo => "SUDO",
                        Elevation::Pkexec => "PKEXEC",
                        Elevation::None => "NONE",
                    },
                ),
                Field::new("Služba systemd", &profile.host.service),
                Field::new(
                    "Adresář konfigurace (prázdné = najít)",
                    &profile.host.config_dir,
                ),
                Field::new(
                    "Jen pro čtení (produkce): ANO / NE",
                    if profile.read_only { "ANO" } else { "NE" },
                ),
                Field::new(
                    "Zapamatovat heslo v systémovém úložišti: ANO / NE",
                    if profile.remember_password {
                        "ANO"
                    } else {
                        "NE"
                    },
                ),
            ],
            focus: 0,
        });
    }
    /// Current privileges of the picker's account at `scope` (GRANT OPTION included).
    fn current_privileges(&self, account: &Account, scope: &GrantScope) -> BTreeSet<String> {
        self.session
            .account
            .as_ref()
            .filter(|d| d.account.as_ref() == Some(account))
            .and_then(|d| d.privileges_at(scope))
            .map(|set| {
                let mut all = set.privileges.clone();
                if set.grantable {
                    all.insert(GRANT_OPTION.into());
                }
                all
            })
            .unwrap_or_default()
    }
    /// Initialise the checkboxes whenever the picker's scope changes.
    fn load_picker(&mut self) {
        let Some(picker) = &self.picker else {
            return;
        };
        let Ok(scope) = picker.scope() else {
            return;
        };
        if picker.loaded_for.as_ref() == Some(&scope) {
            return;
        }
        let wanted = self.current_privileges(&picker.account, &scope);
        let picker = self.picker.as_mut().expect("checked above");
        picker.wanted = wanted;
        picker.loaded_for = Some(scope);
    }
    fn picker_key(&mut self, key: KeyEvent) {
        let Some(mut picker) = self.picker.take() else {
            return;
        };
        let options = picker.options();
        let total = options.len() + 3;
        match key.code {
            K::Esc => return,
            K::Down | K::Tab => picker.cursor = (picker.cursor + 1) % total,
            K::Up | K::BackTab => picker.cursor = (picker.cursor + total - 1) % total,
            K::PageDown => picker.cursor = (picker.cursor + 8).min(total - 1),
            K::PageUp => picker.cursor = picker.cursor.saturating_sub(8),
            K::Left | K::Right if picker.cursor == 0 => {
                let scopes: Vec<GrantScope> = self
                    .session
                    .account
                    .as_ref()
                    .filter(|d| d.account.as_ref() == Some(&picker.account))
                    .map(|d| d.privileges.iter().map(|s| s.scope.clone()).collect())
                    .unwrap_or_default();
                if !scopes.is_empty() {
                    let at = picker
                        .fixed
                        .as_ref()
                        .and_then(|f| scopes.iter().position(|s| s == f));
                    let next = match (at, key.code) {
                        (Some(i), K::Right) => (i + 1) % scopes.len(),
                        (Some(i), _) => (i + scopes.len() - 1) % scopes.len(),
                        (None, _) => 0,
                    };
                    picker.fixed = Some(scopes[next].clone());
                }
            }
            K::Char(c) if picker.cursor == 0 && !key.modifiers.intersects(M::CONTROL | M::ALT) => {
                if let Some(fixed) = picker.fixed.take() {
                    picker.scope_text = scope_text(&fixed);
                }
                picker.scope_text.push(c);
            }
            K::Backspace if picker.cursor == 0 => {
                if let Some(fixed) = picker.fixed.take() {
                    picker.scope_text = scope_text(&fixed);
                }
                picker.scope_text.pop();
            }
            K::Char(' ') | K::Enter if (1..=options.len()).contains(&picker.cursor) => {
                let privilege = options[picker.cursor - 1].to_string();
                if !picker.wanted.remove(&privilege) {
                    picker.wanted.insert(privilege);
                }
            }
            K::Enter if picker.cursor == options.len() + 1 => return,
            K::Enter if picker.cursor == options.len() + 2 => match picker.scope() {
                Ok(scope) => {
                    let current = self.current_privileges(&picker.account, &scope);
                    let (grant, revoke) = privilege_diff(&current, &picker.wanted);
                    let change = Change::Privileges {
                        account: picker.account.clone(),
                        scope,
                        grant,
                        revoke,
                    };
                    match preview(&change) {
                        Ok(_) => {
                            self.confirm = Some(Confirmation::new(Pending::Server(change)));
                            self.picker = Some(picker);
                            return;
                        }
                        Err(error) => self.say(error.to_string()),
                    }
                }
                Err(error) => self.say(error.to_string()),
            },
            _ => (),
        }
        self.picker = Some(picker);
        self.load_picker();
        if let Some(picker) = &mut self.picker {
            picker.cursor = picker.cursor.min(picker.options().len() + 2);
        }
    }
    fn dialog_key(&mut self, key: KeyEvent) {
        let Some(mut d) = self.dialog.take() else {
            return;
        };
        let total = d.fields.len()
            + if matches!(d.kind, Kind::Profile(_)) {
                4
            } else {
                2
            };
        match key.code {
            K::Esc => {
                self.suspended = Some(d);
                self.say("Formulář zůstal rozpracovaný. Otevřete ho přes nabídku.");
                return;
            }
            K::Tab | K::Down => d.focus = (d.focus + 1) % total,
            K::BackTab | K::Up => d.focus = (d.focus + total - 1) % total,
            K::Char(c)
                if d.focus < d.fields.len() && !key.modifiers.intersects(M::CONTROL | M::ALT) =>
            {
                if !c.is_control() {
                    d.fields[d.focus].value.push(c);
                }
            }
            K::Backspace if d.focus < d.fields.len() => {
                d.fields[d.focus].value.pop();
            }
            K::Enter if d.focus < d.fields.len() => d.focus += 1,
            K::Enter if d.focus == d.fields.len() => {
                self.suspended = Some(d);
                self.say("Formulář ponechán rozpracovaný.");
                return;
            }
            K::Enter => {
                let button = d.focus - d.fields.len();
                match self.submit(&mut d, button) {
                    Ok(Outcome::Close) => return,
                    Ok(Outcome::Suspend) => {
                        self.suspended = Some(d);
                        return;
                    }
                    Ok(Outcome::Keep) => (),
                    Err(error) => self.say(error.to_string()),
                }
            }
            _ => (),
        }
        self.dialog = Some(d);
    }
    fn submit(&mut self, d: &mut Dialog, button: usize) -> Result<Outcome> {
        let v = |i: usize| d.fields[i].value.trim().to_string();
        let yes_no = |value: String, what: &str| match value.to_ascii_uppercase().as_str() {
            "ANO" | "A" | "YES" => Ok(true),
            "NE" | "N" | "NO" => Ok(false),
            _ => Err(Error::Validation(format!("{what}: zadejte ANO nebo NE."))),
        };
        let number = |value: String, what: &str| {
            value
                .parse::<u64>()
                .map_err(|_| Error::Validation(format!("{what}: zadejte nezáporné celé číslo.")))
        };
        let change = match &d.kind {
            Kind::Search(page) => {
                self.filters[*page] = d.fields[0].value.to_string();
                if *page == self.page {
                    self.selected = 0;
                }
                return Ok(Outcome::Close);
            }
            Kind::Profile(original_id) => {
                let tls = match v(7).as_str() {
                    "VERIFY" => TlsMode::VerifyIdentity,
                    "INSECURE_TEST_ONLY" => TlsMode::InsecureTestOnly,
                    _ => {
                        return Err(Error::Validation(
                            "TLS musí být VERIFY nebo výslovné INSECURE_TEST_ONLY.".into(),
                        ))
                    }
                };
                let profile = Profile {
                    id: original_id.clone().unwrap_or_default(),
                    name: d.fields[0].value.to_string(),
                    user: d.fields[3].value.to_string(),
                    tls,
                    ca: (!v(6).is_empty()).then(|| v(6)),
                    transport: if v(5).is_empty() {
                        Transport::Tcp {
                            host: v(1),
                            port: v(2)
                                .parse()
                                .map_err(|_| Error::Validation("Neplatný port.".into()))?,
                        }
                    } else {
                        Transport::Unix { path: v(5) }
                    },
                    host: HostSettings {
                        access: match v(8).to_ascii_uppercase().as_str() {
                            "NONE" | "" => HostAccess::None,
                            "LOCAL" => HostAccess::Local,
                            "SSH" => HostAccess::Ssh {
                                target: v(9),
                                port: v(10)
                                    .parse()
                                    .map_err(|_| Error::Validation("Neplatný SSH port.".into()))?,
                            },
                            _ => {
                                return Err(Error::Validation(
                                    "Přístup k serveru musí být NONE, LOCAL nebo SSH.".into(),
                                ))
                            }
                        },
                        elevation: match v(11).to_ascii_uppercase().as_str() {
                            "SUDO" => Elevation::Sudo,
                            "PKEXEC" => Elevation::Pkexec,
                            "NONE" | "" => Elevation::None,
                            _ => {
                                return Err(Error::Validation(
                                    "Práva správce musí být SUDO, PKEXEC nebo NONE.".into(),
                                ))
                            }
                        },
                        service: if v(12).is_empty() {
                            "mariadb".into()
                        } else {
                            v(12)
                        },
                        config_dir: v(13),
                    },
                    read_only: yes_no(v(14), "Jen pro čtení")?,
                    remember_password: yes_no(v(15), "Zapamatovat heslo")?,
                    // upsert_profile keeps unknown fields of the stored profile.
                    extra: Default::default(),
                };
                profile.validate()?;
                if button == 1 {
                    // Only a profile that remembered its password asks the keyring.
                    let forget = !profile.remember_password
                        && self
                            .settings
                            .profile(&profile.id)
                            .is_some_and(|stored| stored.remember_password);
                    let id = self.try_save(|s| s.upsert_profile(profile))?;
                    if forget {
                        let _ = secrets::delete_password(&id);
                    }
                    // A second save of the same form updates this profile, not a new one.
                    d.kind = Kind::Profile(Some(id));
                    self.say("Profil uložen. Heslo není součástí konfigurace.");
                    return Ok(Outcome::Keep);
                }
                if button == 2 {
                    self.session.request(Operation::TestConnection {
                        profile,
                        password: d.fields[4].secret(),
                    });
                    return Ok(Outcome::Keep);
                }
                self.session.connect(profile, d.fields[4].secret(), false);
                self.page = SERVER;
                return Ok(Outcome::Close);
            }
            Kind::Database => {
                identifier(&v(0))?;
                Change::CreateDatabase {
                    name: v(0),
                    collation: v(1),
                }
            }
            Kind::Account => Change::CreateAccount {
                user: v(0),
                host: v(1),
                password: d.fields[2].secret(),
            },
            Kind::Password(account) => Change::ChangePassword {
                account: account.clone(),
                password: d.fields[0].secret(),
            },
            Kind::Role => Change::CreateRole { name: v(0) },
            Kind::RoleMembership(account) => Change::RoleMembership {
                role: v(0),
                account: account.clone(),
                grant: match v(1).to_ascii_uppercase().as_str() {
                    "GRANT" => true,
                    "REVOKE" => false,
                    _ => return Err(Error::Validation("Akce musí být GRANT nebo REVOKE.".into())),
                },
            },
            Kind::DefaultRole(account) => Change::DefaultRole {
                account: account.clone(),
                role: (!v(0).is_empty()).then(|| v(0)),
            },
            Kind::Template(account) => {
                let template = Template::parse(&v(0)).ok_or_else(|| {
                    Error::Validation("Šablona musí být READONLY, APP, BACKUP nebo MONITOR.".into())
                })?;
                template.change(account.clone(), &v(1))?
            }
            Kind::Properties(account) => Change::AlterAccount {
                account: account.clone(),
                locked: yes_no(v(0), "Zamčen")?,
                expire_password: yes_no(v(1), "Vypršení hesla")?,
                tls: match v(2).to_ascii_uppercase().as_str() {
                    "NONE" => Some(TlsRequirement::None),
                    "SSL" => Some(TlsRequirement::Ssl),
                    "X509" => Some(TlsRequirement::X509),
                    "KEEP" | "" => None,
                    _ => {
                        return Err(Error::Validation(
                            "TLS musí být NONE, SSL, X509 nebo KEEP.".into(),
                        ))
                    }
                },
                limits: Limits {
                    queries_per_hour: number(v(3), "Dotazy za hodinu")?,
                    updates_per_hour: number(v(4), "Změny za hodinu")?,
                    connections_per_hour: number(v(5), "Připojení za hodinu")?,
                    user_connections: number(v(6), "Souběžná spojení")?,
                },
            },
            Kind::Maintenance(database) => Change::Maintenance {
                database: database.clone(),
                table: v(0),
                action: Maintenance::parse(&v(1)).ok_or_else(|| {
                    Error::Validation("Akce musí být ANALYZE, CHECK, OPTIMIZE nebo REPAIR.".into())
                })?,
            },
            Kind::Variable(variable) => Change::SetGlobal {
                variable: variable.clone(),
                value: v(0),
            },
            Kind::Backup => {
                let profile = self
                    .session
                    .profile
                    .clone()
                    .filter(|_| self.session.snapshot.as_ref().is_some_and(|s| !s.demo))
                    .ok_or_else(|| Error::Validation("Připojte skutečný server.".into()))?;
                let mut databases: Vec<String> = v(0)
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if databases.is_empty() {
                    databases = self
                        .session
                        .snapshot
                        .as_ref()
                        .map(|s| {
                            s.databases
                                .iter()
                                .filter(|d| !d.system())
                                .map(|d| d.name.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                }
                if databases.is_empty() {
                    return Err(Error::Validation(
                        "Server nemá žádnou uživatelskou databázi; zadejte názvy.".into(),
                    ));
                }
                if d.fields[2].value != d.fields[3].value {
                    return Err(Error::Validation(
                        "Hesla pro šifrování se neshodují.".into(),
                    ));
                }
                if v(1).is_empty() {
                    return Err(Error::Validation("Zadejte adresář záloh.".into()));
                }
                let spec = JobSpec::Backup(BackupRequest {
                    path: std::path::PathBuf::from(v(1)).join(backup_file_name(
                        &profile.name,
                        std::time::SystemTime::now(),
                    )),
                    profile: profile.name,
                    databases,
                    encryption: d.fields[2].optional_secret(),
                });
                if !self.session.start_job(spec) {
                    return Err(Error::Validation(
                        "Jiná operace právě běží; zkuste to za chvíli.".into(),
                    ));
                }
                self.page = BACKUPS;
                return Ok(Outcome::Close);
            }
            Kind::Verify(path) => {
                self.session.start_job(JobSpec::Verify {
                    path: path.clone(),
                    encryption: d.fields[0].optional_secret(),
                });
                return Ok(Outcome::Close);
            }
            Kind::Restore(path) => {
                identifier(&v(1))?;
                let request = RestoreRequest {
                    path: path.clone(),
                    encryption: d.fields[3].optional_secret(),
                    database: v(0),
                    target: v(1),
                    replace: yes_no(v(2), "Nahradit")?,
                };
                self.confirm = Some(Confirmation::new(Pending::Restore(request)));
                return Ok(Outcome::Suspend);
            }
            Kind::Schedule => {
                let profile =
                    self.session.profile.clone().ok_or_else(|| {
                        Error::Validation("Nejprve se připojte k serveru.".into())
                    })?;
                let file =
                    |text: String| (!text.is_empty()).then(|| std::path::PathBuf::from(text));
                let plan = schedule::Schedule {
                    profile_id: profile.id,
                    profile_name: profile.name,
                    calendar: v(0),
                    keep: v(1).parse().map_err(|_| {
                        Error::Validation("Uchovávat: zadejte kladné číslo.".into())
                    })?,
                    databases: v(2)
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                    dir: std::path::PathBuf::from(v(3)),
                    password_file: file(v(4)),
                    encryption_file: file(v(5)),
                };
                schedule::render_units(&plan, std::path::Path::new("/x"))?;
                self.confirm = Some(Confirmation::new(Pending::Schedule(plan)));
                return Ok(Outcome::Suspend);
            }
            Kind::ConfigValue(name) => {
                let entries = self.config_change(name, &v(0))?;
                self.confirm = Some(Confirmation::new(Pending::WriteConfig(entries)));
                return Ok(Outcome::Suspend);
            }
            Kind::Console => {
                let database = v(0);
                let sql = d.fields[1].value.to_string();
                match self.session.console(&database, &sql, false)? {
                    ConsoleKind::Write => {
                        self.confirm =
                            Some(Confirmation::new(Pending::ConsoleWrite(database, sql)));
                        return Ok(Outcome::Suspend);
                    }
                    ConsoleKind::Read => {
                        self.selected = 0;
                        return Ok(Outcome::Close);
                    }
                }
            }
            Kind::Compare => {
                let name = v(0);
                let right = self
                    .settings
                    .profiles
                    .iter()
                    .find(|p| p.name.trim().eq_ignore_ascii_case(name.trim()))
                    .cloned()
                    .ok_or_else(|| {
                        Error::Validation(format!("Profil {} neexistuje.", safe_text(&name)))
                    })?;
                let text = v(2).to_uppercase();
                let areas = if text == "VŠE" || text.is_empty() {
                    Areas::default()
                } else {
                    Areas {
                        variables: text.contains("PROMĚNN"),
                        accounts: text.contains("ÚČT") || text.contains("UCT"),
                        schema: text.contains("SCHÉM") || text.contains("SCHEM"),
                    }
                };
                self.session
                    .compare_with(right, d.fields[1].secret(), areas)?;
                self.selected = 0;
                return Ok(Outcome::Close);
            }
            Kind::CompareExport => {
                let result = self
                    .session
                    .compare
                    .as_ref()
                    .and_then(|c| c.result())
                    .and_then(|r| r.ok())
                    .ok_or_else(|| Error::Validation("Žádný výsledek k uložení.".into()))?;
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let path = v(0);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .map_err(|e| Error::File(format!("{path}: {e}")))?;
                file.write_all(result.csv().as_bytes())
                    .map_err(|e| Error::File(format!("{path}: {e}")))?;
                self.say(format!("Uloženo do {path}."));
                return Ok(Outcome::Close);
            }
            Kind::ConsoleExport => {
                let result = self
                    .session
                    .console
                    .as_ref()
                    .and_then(|c| c.result())
                    .and_then(|r| r.ok())
                    .ok_or_else(|| Error::Validation("Žádný výsledek k uložení.".into()))?;
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let path = v(0);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .map_err(|e| Error::File(format!("{path}: {e}")))?;
                file.write_all(result.csv().as_bytes())
                    .map_err(|e| Error::File(format!("{path}: {e}")))?;
                self.say(format!("Uloženo do {path}."));
                return Ok(Outcome::Close);
            }
            Kind::ConfigRemove => {
                let config = self
                    .session
                    .host_config
                    .as_ref()
                    .ok_or_else(|| Error::Validation("Konfigurace se ještě načítá.".into()))?;
                let name = v(0).to_ascii_lowercase().replace('-', "_");
                if !config.entries.iter().any(|(n, _)| *n == name) {
                    return Err(Error::Validation(format!(
                        "Volba {} v konfiguraci Harboru není.",
                        safe_text(&name)
                    )));
                }
                let rest = config
                    .entries
                    .iter()
                    .filter(|(n, _)| *n != name)
                    .cloned()
                    .collect();
                self.confirm = Some(Confirmation::new(Pending::WriteConfig(rest)));
                return Ok(Outcome::Suspend);
            }
            Kind::Unschedule => {
                self.confirm = Some(Confirmation::new(Pending::Unschedule(v(0))));
                return Ok(Outcome::Close);
            }
        };
        // Invalid input is reported in the form, not in an unusable confirmation.
        preview(&change)?;
        self.confirm = Some(Confirmation::new(Pending::Server(change)));
        // The caller moves the dialog to `suspended` (no copy of a typed password).
        Ok(Outcome::Suspend)
    }
    fn confirm_key(&mut self, key: KeyEvent) {
        let Some(mut confirm) = self.confirm.take() else {
            return;
        };
        let from_form = confirm.returns_to_form();
        let restore = |app: &mut Self| {
            if from_form {
                app.dialog = app.suspended.take();
            }
        };
        match key.code {
            K::Esc => {
                restore(self);
                return;
            }
            K::Tab | K::BackTab | K::Left | K::Right => confirm.yes = !confirm.yes,
            K::Char(c)
                if confirm.required_name().is_some()
                    && !key.modifiers.intersects(M::CONTROL | M::ALT) =>
            {
                confirm.typed.push(c);
            }
            K::Backspace => {
                confirm.typed.pop();
            }
            K::Enter if !confirm.yes => {
                restore(self);
                return;
            }
            K::Enter => {
                if confirm
                    .required_name()
                    .is_some_and(|name| name != confirm.typed)
                {
                    self.say("Pro potvrzení přesně opište název objektu.");
                    self.confirm = Some(confirm);
                    return;
                }
                let change = match &confirm.action {
                    Pending::DeleteProfile { id } => {
                        let id = id.clone();
                        match self.try_save(|s| s.remove_profile(&id)) {
                            Ok(profile) => {
                                if profile.remember_password {
                                    let _ = secrets::delete_password(&profile.id);
                                }
                                self.say(format!(
                                    "Profil „{}“ odstraněn.",
                                    safe_text(&profile.name)
                                ));
                                self.selected = 0;
                            }
                            Err(error) => self.say(error.to_string()),
                        }
                        return;
                    }
                    Pending::Restore(request) => {
                        if self.session.start_job(JobSpec::Restore(request.clone())) {
                            self.suspended = None;
                            return;
                        }
                        self.say("Jiná operace právě běží; potvrzení zůstává otevřené.");
                        self.confirm = Some(confirm);
                        return;
                    }
                    Pending::DeleteBackup(path) => {
                        match std::fs::remove_file(path) {
                            Ok(()) => self.say("Soubor zálohy smazán."),
                            Err(error) => self.say(format!("Soubor nelze smazat: {error}")),
                        }
                        self.reload_backups();
                        return;
                    }
                    Pending::Schedule(plan) => {
                        match schedule::current_binary()
                            .and_then(|binary| schedule::install(plan, &binary))
                        {
                            Ok(note) => {
                                self.suspended = None;
                                self.say(note);
                            }
                            Err(error) => self.say(error.to_string()),
                        }
                        self.reload_backups();
                        return;
                    }
                    Pending::Service(action, _) => {
                        if !self.session.host_request(HostRequest::Control(*action)) {
                            self.say("Na serveru právě běží jiný příkaz.");
                        }
                        return;
                    }
                    Pending::ConsoleWrite(database, sql) => {
                        match self.session.console(database, sql, true) {
                            Ok(_) => self.suspended = None,
                            Err(error) => self.say(error.to_string()),
                        }
                        return;
                    }
                    Pending::WriteConfig(entries) => {
                        let profile = self
                            .session
                            .profile
                            .as_ref()
                            .map(|p| p.name.clone())
                            .unwrap_or_default();
                        if self.session.host_request(HostRequest::WriteConfig {
                            entries: entries.clone(),
                            profile,
                        }) {
                            self.suspended = None;
                        } else {
                            self.say("Na serveru právě běží jiný příkaz.");
                        }
                        return;
                    }
                    Pending::Unschedule(name) => {
                        match schedule::remove(name) {
                            Ok(()) => self.say("Plán odebrán."),
                            Err(error) => self.say(error.to_string()),
                        }
                        self.reload_backups();
                        return;
                    }
                    Pending::Server(change) => change.clone(),
                };
                if self.session.request(Operation::Apply(change)) {
                    self.suspended = None;
                    self.picker = None;
                    return;
                }
                self.say("Jiná operace právě běží; potvrzení zůstává otevřené.");
            }
            _ => (),
        }
        self.confirm = Some(confirm);
    }
}
fn account_text(detail: &AccountDetail) -> String {
    let mut lines = Vec::new();
    if let Some(props) = &detail.properties {
        lines.push(format!(
            "{} • {} • heslo {} • TLS {}",
            safe_text(&props.plugin),
            if props.locked { "ZAMČEN" } else { "odemčen" },
            if props.password_expired {
                "VYPRŠELO"
            } else {
                "platné"
            },
            props.tls.label()
        ));
        let l = props.limits;
        lines.push(format!(
            "Limity: dotazy/h {} • změny/h {} • připojení/h {} • souběžně {} (0 = bez limitu)",
            l.queries_per_hour, l.updates_per_hour, l.connections_per_hour, l.user_connections
        ));
    }
    lines.push(format!(
        "Role: {} • výchozí: {}",
        if detail.roles.is_empty() {
            "žádné".into()
        } else {
            safe_text(&detail.roles.join(", "))
        },
        detail
            .default_role
            .as_deref()
            .map(safe_text)
            .unwrap_or_else(|| "žádná".into())
    ));
    for set in &detail.privileges {
        lines.push(format!(
            "{}: {}{}",
            safe_text(&set.scope.label()),
            if set.privileges.is_empty() {
                "USAGE".into()
            } else {
                set.privileges
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            if set.grantable { " + GRANT OPTION" } else { "" }
        ));
    }
    for column in &detail.columns {
        lines.push(format!("Sloupec: {}", safe_text(column)));
    }
    lines.push("— SHOW GRANTS —".into());
    lines.extend(detail.grants.iter().map(|g| safe_text(g)));
    lines.join("\n")
}
fn variable_text(v: &Variable) -> String {
    let mut lines = Vec::new();
    if let Some(description) = v.key_description() {
        lines.push(description.to_string());
    }
    if !v.comment.is_empty() {
        lines.push(safe_text(&v.comment));
    }
    let human = v
        .value
        .parse::<u64>()
        .ok()
        .filter(|n| v.var_type.contains("INT") && *n >= 1_048_576)
        .map(|n| format!(" ({})", human_bytes(n)))
        .unwrap_or_default();
    lines.push(format!("Hodnota: {}{human}", safe_text(&v.value)));
    lines.push(format!(
        "Výchozí: {} • původ: {} • typ: {}",
        v.default
            .as_deref()
            .map(safe_text)
            .unwrap_or_else(|| "—".into()),
        safe_text(&v.origin),
        safe_text(&v.var_type)
    ));
    if v.min.is_some() || v.max.is_some() {
        lines.push(format!(
            "Rozsah: {} až {}",
            v.min.as_deref().unwrap_or("?"),
            v.max.as_deref().unwrap_or("?")
        ));
    }
    if !v.allowed.is_empty() {
        lines.push(format!("Povolené: {}", safe_text(&v.allowed.join(", "))));
    }
    lines.push(if v.settable() {
        "Změna: F10 → Proměnná: změnit hodnotu (platí do restartu serveru)".into()
    } else {
        "Nelze měnit za běhu; patří do konfiguračního souboru.".into()
    });
    lines.join("\n")
}
fn table_text(t: &TableDetail) -> String {
    let mut lines = vec!["SLOUPCE".to_string()];
    for c in &t.columns {
        lines.push(format!(
            "  {:<24} {:<24} {:<4} {:<4} {}{}",
            safe_text(&c.name),
            safe_text(&c.column_type),
            if c.nullable { "NULL" } else { "" },
            safe_text(&c.key),
            safe_text(&c.extra),
            c.default
                .as_deref()
                .map(|d| format!(" default {}", safe_text(d)))
                .unwrap_or_default()
        ));
    }
    if !t.indexes.is_empty() {
        lines.push("INDEXY".into());
        for i in &t.indexes {
            lines.push(format!(
                "  {}{} ({}) {} ≈{}",
                safe_text(&i.name),
                if i.unique { " UNIQUE" } else { "" },
                safe_text(&i.columns.join(", ")),
                safe_text(&i.index_type),
                i.cardinality
            ));
        }
    }
    if !t.foreign_keys.is_empty() {
        lines.push("CIZÍ KLÍČE".into());
        for fk in &t.foreign_keys {
            lines.push(format!(
                "  {} ({}) → {} ({}) ON DELETE {} ON UPDATE {}",
                safe_text(&fk.name),
                safe_text(&fk.columns.join(", ")),
                safe_text(&fk.referenced_table),
                safe_text(&fk.referenced_columns.join(", ")),
                safe_text(&fk.on_delete),
                safe_text(&fk.on_update)
            ));
        }
    }
    lines.push("DEFINICE".into());
    lines.extend(t.create_sql.lines().map(|l| format!("  {}", safe_text(l))));
    lines.join("\n")
}
/// Page names for the top line; scrolls so the current page stays visible.
fn page_bar(page: usize, width: usize) -> String {
    let full: Vec<String> = PAGES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if i == page {
                format!("[{name}]")
            } else {
                name.to_string()
            }
        })
        .collect();
    let mut start = 0;
    loop {
        let shown = full[start..].join(" ");
        let prefix = if start > 0 { "‹ " } else { "" };
        let end = full[..=page].join(" ").chars().count();
        let before = full[..start].join(" ").chars().count();
        if start >= page || end - before + prefix.len() + 2 <= width {
            let text = format!("{prefix}{shown}");
            return if text.chars().count() > width {
                format!(
                    "{}›",
                    text.chars()
                        .take(width.saturating_sub(1))
                        .collect::<String>()
                )
            } else {
                text
            };
        }
        start += 1;
    }
}
/// Ctrl+M is not usable: terminals send it as CR, which crossterm reports as Enter.
/// F9 and Alt+M are alternatives for terminals that capture F10 (e.g. GNOME Terminal).
fn is_menu_key(key: &KeyEvent) -> bool {
    matches!(key.code, K::F(10) | K::F(9))
        || (matches!(key.code, K::Char('m') | K::Char('M')) && key.modifiers.contains(M::ALT))
}
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests;
