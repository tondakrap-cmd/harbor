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

const PAGES: [&str; 8] = [
    "Server",
    "Databáze",
    "Uživatelé",
    "Spojení",
    "Proměnné",
    "Doktor",
    "Profily",
    "Nastavení",
];
const SERVER: usize = 0;
const DATABASES: usize = 1;
const ACCOUNTS: usize = 2;
const PROCESSES: usize = 3;
const VARIABLES: usize = 4;
const HEALTH: usize = 5;
const PROFILES: usize = 6;
const SETTINGS: usize = 7;

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
    ("Profily (7, F3)", Action::Page(PROFILES)),
    ("Nastavení (8)", Action::Page(SETTINGS)),
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
    ("Proměnná: změnit hodnotu (SET GLOBAL)", Action::SetVariable),
    ("Doktor: spustit kontrolu", Action::RunHealth),
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
}
/// Selected row identity; the index follows it when a refresh reorders the list.
#[derive(Clone, PartialEq, Debug)]
enum RowKey {
    Database(String),
    Account(Account),
    Process(u64),
    Variable(String),
    Finding(String),
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
        self.account().is_some() || matches!(self, Self::Maintenance(_) | Self::Variable(_))
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
    DeleteProfile { id: String },
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
            Pending::DeleteProfile { .. } => None,
        }
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
    filters: [String; 8],
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
    }
    /// The variables and health pages load their data the first time they are shown
    /// for a server; afterwards only on request.
    fn autoload(&mut self) {
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
            PROCESSES => self
                .processes()
                .iter()
                .map(|p| RowKey::Process(p.id))
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
            Paragraph::new(
                PAGES
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        if i == self.page {
                            format!("[{name}]")
                        } else {
                            format!(" {name} ")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(""),
            )
            .style(p.menu),
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
                " F1 Pomoc  F3 Profily  F5 Obnovit  F10/F9/Alt+M Menu  1–8 Obrazovky  Esc Zpět",
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
            frame.render_widget(Paragraph::new("F10, F9 nebo Alt+M: všechny akce v nabídce\n1–8: přímo na obrazovku (Server … Nastavení)\nŠipky / PageUp / PageDown: výběr a posuv\nEnter: otevřít detail, upravit profil nebo nastavení\nTab: přepnout na detail (Databáze: výběr tabulky, Enter = struktura)\nVlevo/vpravo: vodorovný posuv seznamu\nEsc: zavřít; formulář zůstane rozpracovaný\nF3: profily; F5: obnovit (na Doktorovi znovu zkontrolovat)\n\nOprávnění: mezerník zaškrtne, Enter na Zkontrolovat ukáže SQL.\nZápisy vyžadují potvrzení s náhledem SQL; výchozí je Zrušit.\nMazání vyžaduje opsat název objektu.\nSET GLOBAL platí jen do restartu serveru.\nDEMO je simulace bez skutečného serveru.\nTCP standardně vyžaduje ověřenou identitu TLS.\n\nEsc zavře nápovědu.").block(self.block(" Nápověda ", p)).style(p.base).wrap(Wrap { trim: false }), area);
        }
        if let Some(index) = self.menu {
            let area = centered(area, 64, 26);
            frame.render_widget(Clear, area);
            let items: Vec<_> = MENU
                .iter()
                .map(|(label, _)| ListItem::new(*label))
                .collect();
            let mut state = ListState::default().with_selected(Some(index));
            frame.render_stateful_widget(
                List::new(items)
                    .block(self.block(" Nabídka • Enter vybere / Esc zavře ", p))
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
        let mut lines = Vec::new();
        for (i, field) in d.fields.iter().enumerate() {
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
        lines.push(Line::from(""));
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
            self.menu = if self.menu.is_some() {
                None
            } else {
                Some(
                    MENU.iter()
                        .position(|(_, a)| *a == Action::Page(self.page))
                        .unwrap_or(0),
                )
            };
            return;
        }
        if let Some(index) = self.menu {
            match key.code {
                K::Esc => self.menu = None,
                K::Up => self.menu = Some(index.saturating_sub(1)),
                K::Down => self.menu = Some((index + 1).min(MENU.len() - 1)),
                K::PageUp => self.menu = Some(index.saturating_sub(8)),
                K::PageDown => self.menu = Some((index + 8).min(MENU.len() - 1)),
                K::Enter => {
                    self.menu = None;
                    self.run(MENU[index].1);
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
            K::Char(c @ '1'..='8') if key.modifiers.is_empty() => {
                self.run(Action::Page(c as usize - '1' as usize));
            }
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
        let (host, port, socket) = match profile.transport {
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
                    // upsert_profile keeps unknown fields of the stored profile.
                    extra: Default::default(),
                };
                profile.validate()?;
                if button == 1 {
                    let id = self.try_save(|s| s.upsert_profile(profile))?;
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
        let profile_delete = matches!(confirm.action, Pending::DeleteProfile { .. });
        let restore = |app: &mut Self| {
            if !profile_delete {
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
