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
use zeroize::Zeroizing;

const PAGES: [&str; 6] = [
    "Server",
    "Databáze",
    "Uživatelé",
    "Spojení",
    "Profily",
    "Nastavení",
];
const MENU: [&str; 23] = [
    "Server",
    "Databáze",
    "Uživatelé",
    "Spojení",
    "Profily",
    "Nastavení",
    "Nápověda",
    "Obnovit",
    "Nový profil",
    "Otevřít DEMO",
    "Odpojit / zrušit připojování",
    "Vytvořit databázi",
    "Vytvořit účet",
    "Změnit heslo vybraného účtu",
    "Oprávnění vybraného účtu",
    "Ukončit vybraný dotaz",
    "Hledat",
    "Systémové databáze ano/ne",
    "Jen dlouhé dotazy ano/ne",
    "Další motiv (zachová formulář)",
    "Rozpracovaný formulář",
    "Odstranit vybraný uložený profil",
    "Konec",
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
    Grant(Account),
    /// Search for the list on this page.
    Search(usize),
}
struct Dialog {
    kind: Kind,
    fields: Vec<Field>,
    focus: usize,
}
enum Pending {
    Server(Change),
    DeleteProfile { id: String },
}
struct Confirmation {
    action: Pending,
    yes: bool,
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
    menu: Option<usize>,
    help: bool,
    dialog: Option<Dialog>,
    suspended: Option<Dialog>,
    confirm: Option<Confirmation>,
    /// One search per page, like the desktop.
    filters: [String; 6],
    system: bool,
    long: bool,
    detail: String,
    status: String,
    status_at: std::time::Instant,
    /// Worker generation the selection and account-bound drafts belong to.
    scope_generation: u64,
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
            page: 4,
            selected: 0,
            selected_key: None,
            horizontal: 0,
            detail_scroll: 0,
            detail_focus: false,
            menu: None,
            help: false,
            dialog: None,
            suspended: None,
            confirm: None,
            filters: Default::default(),
            system: false,
            long: false,
            detail: String::new(),
            status,
            status_at: std::time::Instant::now(),
            scope_generation: 0,
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
            app.page = 0;
        }
        app.scope_generation = app.session.worker.generation();
        app
    }
    pub fn tick(&mut self) {
        self.session.tick(self.settings.refresh_seconds);
        self.sync_server_scope();
        self.reconcile_selection();
    }
    fn filter(&self) -> &str {
        &self.filters[self.page]
    }
    fn row_keys(&self) -> Vec<RowKey> {
        match self.page {
            1 => self
                .databases()
                .iter()
                .map(|d| RowKey::Database(d.name.clone()))
                .collect(),
            2 => self
                .accounts()
                .iter()
                .map(|a| RowKey::Account((*a).clone()))
                .collect(),
            3 => self
                .processes()
                .iter()
                .map(|p| RowKey::Process(p.id))
                .collect(),
            4 => self
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
        if self.page != 4 {
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
            if matches!(
                self.confirm.as_ref().map(|c| &c.action),
                Some(Pending::Server(_))
            ) {
                self.confirm = None;
            }
        }
        let accounts = self.session.snapshot.as_ref().map(|s| &s.accounts);
        let stale = |kind: &Kind| match kind {
            Kind::Password(a) | Kind::Grant(a) => {
                switched || accounts.is_some_and(|list| !list.contains(a))
            }
            _ => false,
        };
        let mut dropped = false;
        for slot in [&mut self.dialog, &mut self.suspended] {
            if slot.as_ref().is_some_and(|d| stale(&d.kind)) {
                *slot = None;
                dropped = true;
            }
        }
        if let Some(Pending::Server(
            Change::ChangePassword { account, .. } | Change::DatabaseGrant { account, .. },
        )) = self.confirm.as_ref().map(|c| &c.action)
        {
            if accounts.is_some_and(|list| !list.contains(account)) {
                self.confirm = None;
                dropped = true;
            }
        }
        if dropped {
            self.say("Rozpracovaná změna účtu byla zahozena: účet patří jinému serveru nebo už neexistuje.");
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
        self.theme = ThemeId::TERMINAL[(i + 1) % 5];
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
                    .join(" "),
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
            0 => self.overview(frame, rows[2], p),
            5 => self.draw_settings(frame, rows[2], p),
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
                " F1 Pomoc  F3 Profily  F5 Obnovit  F10/F9/Alt+M Menu  Esc Zpět  Ctrl+C Konec",
            )
            .style(p.menu),
            rows[4],
        );
        if let Some(dialog) = &self.dialog {
            self.draw_dialog(frame, dialog, p);
        }
        if self.help {
            let area = centered(area, 74, 19);
            frame.render_widget(Clear, area);
            frame.render_widget(Paragraph::new("F10, F9 nebo Alt+M: všechny akce v nabídce\nŠipky / PageUp / PageDown: výběr a posuv\nEnter: otevřít detail, upravit profil nebo nastavení\nTab / Shift+Tab: další/předchozí pole nebo obrazovka\nVlevo/vpravo: vodorovný posuv seznamu\nEsc: zavřít; formulář zůstane rozpracovaný\nF3: profily, Enter: upravit uložený profil\nF5: obnovit data; Ctrl+C: ukončit\n\nZápisy vyžadují potvrzení; výchozí je Zrušit.\nDEMO je simulace bez skutečného serveru.\nTCP standardně vyžaduje ověřenou identitu TLS.\nBarvy 16 / truecolor / mono a ASCII: Nastavení.\nPři chybě zůstávají data označena jako neaktuální.\n\nEsc zavře nápovědu.").block(self.block(" Nápověda ", p)).style(p.base).wrap(Wrap { trim: false }), area);
        }
        if let Some(index) = self.menu {
            let area = centered(area, 64, 24);
            frame.render_widget(Clear, area);
            let items: Vec<_> = MENU.iter().map(|s| ListItem::new(*s)).collect();
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
            let area = centered(area, 74, 12);
            frame.render_widget(Clear, area);
            let body = match &confirm.action {
                Pending::Server(change) => {
                    let query = match change {
                        Change::KillQuery { target } => {
                            format!("\n#{} {}", target.id, safe_text(&target.query))
                        }
                        _ => String::new(),
                    };
                    format!(
                        "Server: {}\n{}{}\n\n{}",
                        safe_text(profile),
                        safe_text(&change.summary()),
                        query,
                        if self.session.snapshot.as_ref().is_some_and(|s| s.demo) {
                            "DEMO • pouze paměť"
                        } else {
                            "Změna na připojeném serveru"
                        }
                    )
                }
                Pending::DeleteProfile { id } => format!(
                    "Odstranit uložený profil „{}“?\n\nZmění se jen konfigurace Harboru, server ne.",
                    self.settings
                        .profile(id)
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "(už neexistuje)".into())
                ),
            };
            let text = format!(
                "{body}\n\n{}\nTab / šipky změní volbu, Enter potvrdí, Esc zruší",
                if confirm.yes {
                    "  Zrušit       [ PROVÉST ZMĚNU ]"
                } else {
                    "[ ZRUŠIT ]      Provést změnu"
                }
            );
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(self.block(" Potvrzení změny ", p))
                    .style(p.base),
                area,
            );
        }
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
    fn list_rows(&self) -> Vec<String> {
        match self.page {
            1 => self
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
            2 => self
                .accounts()
                .iter()
                .map(|a| {
                    format!(
                        "{:<24} {:<24} {}",
                        safe_text(&a.user),
                        safe_text(&a.host),
                        safe_text(&a.plugin)
                    )
                })
                .collect(),
            3 => self
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
            4 => self
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
    fn draw_list(&self, frame: &mut Frame, area: Rect, p: Palette) {
        let rows = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(if area.height > 23 { 9 } else { 6 }),
        ])
        .split(area);
        let list: Vec<_> = self
            .list_rows()
            .iter()
            .map(|r| ListItem::new(r.chars().skip(self.horizontal).collect::<String>()))
            .collect();
        let mut state = ListState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(
            List::new(list)
                .block(self.block(
                    format!(
                        " {} • filtr: {} • Enter detail ",
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
        let details = if self.page == 1 {
            self.session
                .tables
                .iter()
                .map(|t| {
                    format!(
                        "{} • {} • ≈ {} řádků • {}",
                        safe_text(&t.name),
                        safe_text(&t.engine),
                        t.rows,
                        bytes_label(t.bytes)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else if self.page == 2 {
            self.session
                .grants
                .iter()
                .map(|g| safe_text(g))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            self.detail.clone()
        };
        frame.render_widget(
            Paragraph::new(details)
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0))
                .block(self.block(
                    format!(
                        " {}{} ",
                        match self.page {
                            1 => self
                                .session
                                .tables_for
                                .as_deref()
                                .map(|db| format!("Tabulky {} • ", safe_text(db)))
                                .unwrap_or_default(),
                            2 => self
                                .session
                                .grants_for
                                .as_ref()
                                .map(|a| format!("Oprávnění {} • ", safe_text(&a.label())))
                                .unwrap_or_default(),
                            _ => String::new(),
                        },
                        if self.detail_focus {
                            "> Detail • šipky posouvají • Tab zpět"
                        } else {
                            "Detail • Tab aktivuje posuv • F10 akce"
                        }
                    ),
                    p,
                ))
                .style(p.base),
            rows[1],
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
                Some(self.page)
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
                    self.action(index);
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
        match key.code {
            K::F(3) => {
                self.page = 4;
                self.selected = 0;
            }
            K::F(5) => {
                self.session.request(Operation::Refresh);
            }
            K::Tab => {
                self.detail_focus = !self.detail_focus;
            }
            K::BackTab => {
                self.detail_focus = !self.detail_focus;
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
                self.page = 0;
                self.detail.clear();
            }
            _ => (),
        }
    }
    fn row_count(&self) -> usize {
        if self.page == 5 {
            4
        } else {
            self.list_rows().len()
        }
    }
    fn enter(&mut self) {
        match self.page {
            1 => {
                if let Some(db) = self.databases().get(self.selected) {
                    let name = db.name.clone();
                    if self.session.request(Operation::Tables(name)) {
                        self.session.tables.clear();
                    }
                }
            }
            2 => {
                if let Some(account) = self.accounts().get(self.selected) {
                    let account = (*account).clone();
                    if self.session.request(Operation::Grants(account)) {
                        self.session.grants.clear();
                    }
                }
            }
            3 => {
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
            4 => self.profile_dialog(self.settings.profiles.get(self.selected).cloned()),
            5 => match self.selected.min(3) {
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
    fn action(&mut self, index: usize) {
        if index == 19 {
            self.next_theme();
            return;
        }
        if index < 6 {
            self.page = index;
            self.selected = 0;
            return;
        }
        if let Some(d) = self.dialog.take() {
            self.suspended = Some(d);
        }
        match index {
            6 => self.help = true,
            7 => {
                self.session.request(Operation::Refresh);
            }
            8 => self.profile_dialog(None),
            9 => {
                self.session.connect(
                    Profile {
                        name: "Ukázkový server".into(),
                        ..Default::default()
                    },
                    Secret::default(),
                    true,
                );
                self.page = 0;
            }
            10 => self.session.disconnect(),
            11 => {
                self.dialog = Some(Dialog {
                    kind: Kind::Database,
                    fields: vec![
                        Field::new("Název databáze", ""),
                        Field::new("Kolace (ověří server)", "utf8mb4_unicode_ci"),
                    ],
                    focus: 0,
                })
            }
            12 => {
                self.dialog = Some(Dialog {
                    kind: Kind::Account,
                    fields: vec![
                        Field::new("Uživatel", ""),
                        Field::new("Hostitel účtu", "localhost"),
                        Field::password(),
                    ],
                    focus: 0,
                })
            }
            13 | 14 => {
                if self.page != 2 {
                    self.say("Nejprve vyberte účet na obrazovce Uživatelé.");
                    return;
                }
                let account = self.accounts().get(self.selected).map(|a| (*a).clone());
                if let Some(account) = account {
                    self.dialog = Some(if index == 13 {
                        Dialog {
                            kind: Kind::Password(account),
                            fields: vec![Field::password()],
                            focus: 0,
                        }
                    } else {
                        Dialog {
                            kind: Kind::Grant(account),
                            fields: vec![
                                Field::new("Databáze", ""),
                                Field::new(
                                    "Oprávnění: SELECT / INSERT / UPDATE / DELETE",
                                    "SELECT",
                                ),
                                Field::new("Akce: GRANT / REVOKE", "GRANT"),
                            ],
                            focus: 0,
                        }
                    });
                } else {
                    self.say("Nejprve vyberte účet na obrazovce Uživatelé.");
                }
            }
            15 => {
                if self.page != 3 {
                    self.say("Nejprve vyberte běžící dotaz na obrazovce Spojení.");
                    return;
                }
                let target = self.processes().get(self.selected).map(|p| (*p).clone());
                if let Some(target) = target.filter(|p| p.running_query()) {
                    self.confirm = Some(Confirmation {
                        action: Pending::Server(Change::KillQuery { target }),
                        yes: false,
                    });
                } else {
                    self.say("Vyberte běžící dotaz na obrazovce Spojení.");
                }
            }
            16 => {
                self.dialog = Some(Dialog {
                    kind: Kind::Search(self.page),
                    fields: vec![Field::new("Hledat", self.filter())],
                    focus: 0,
                })
            }
            17 => self.system = !self.system,
            18 => self.long = !self.long,
            19 => self.next_theme(),
            20 => self.dialog = self.suspended.take(),
            21 => {
                if self.page != 4 {
                    self.say("Profil k odstranění vyberte na obrazovce Profily.");
                    return;
                }
                if let Some(profile) = self.settings.profiles.get(self.selected) {
                    self.confirm = Some(Confirmation {
                        action: Pending::DeleteProfile {
                            id: profile.id.clone(),
                        },
                        yes: false,
                    });
                }
            }
            22 => self.quit = true,
            _ => (),
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
        let v = |i: usize| d.fields[i].value.to_string();
        let change = match &d.kind {
            Kind::Search(page) => {
                self.filters[*page] = v(0);
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
                    name: v(0),
                    user: v(3),
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
                self.page = 0;
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
            Kind::Grant(account) => {
                let privilege = Privilege::ALL
                    .into_iter()
                    .find(|p| p.sql() == v(1))
                    .ok_or_else(|| Error::Validation("Nepodporované oprávnění.".into()))?;
                let grant = match v(2).as_str() {
                    "GRANT" => true,
                    "REVOKE" => false,
                    _ => return Err(Error::Validation("Akce musí být GRANT nebo REVOKE.".into())),
                };
                Change::DatabaseGrant {
                    account: account.clone(),
                    database: v(0),
                    privilege,
                    grant,
                }
            }
        };
        self.confirm = Some(Confirmation {
            action: Pending::Server(change),
            yes: false,
        });
        // The caller moves the dialog to `suspended` (no copy of a typed password).
        Ok(Outcome::Suspend)
    }
    fn confirm_key(&mut self, key: KeyEvent) {
        let Some(mut confirm) = self.confirm.take() else {
            return;
        };
        let profile_delete = matches!(confirm.action, Pending::DeleteProfile { .. });
        match key.code {
            K::Esc => {
                if !profile_delete {
                    self.dialog = self.suspended.take();
                }
                return;
            }
            K::Tab | K::BackTab | K::Left | K::Right => confirm.yes = !confirm.yes,
            K::Enter if !confirm.yes => {
                if !profile_delete {
                    self.dialog = self.suspended.take();
                }
                return;
            }
            K::Enter => {
                let change = match &confirm.action {
                    Pending::DeleteProfile { id } => {
                        let id = id.clone();
                        match self.try_save(|s| s.remove_profile(&id)) {
                            Ok(profile) => {
                                self.status =
                                    format!("Profil „{}“ odstraněn.", safe_text(&profile.name));
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
                    return;
                }
                self.say("Jiná operace právě běží; potvrzení zůstává otevřené.");
            }
            _ => (),
        }
        self.confirm = Some(confirm);
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
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn all_screens_themes_modes_and_sizes_render_and_keep_drafts() {
        let mut app = App::new(&[]);
        app.store = None;
        app.profile_dialog(None);
        app.dialog.as_mut().unwrap().fields[0].value = Zeroizing::new("Rozpracováno".into());
        for theme in ThemeId::TERMINAL {
            app.theme = theme;
            for colors in ["16", "mono", "truecolor"] {
                app.colors = colors.into();
                for (w, h) in [(80, 24), (120, 35), (60, 20)] {
                    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                    for page in 0..6 {
                        app.page = page;
                        terminal.draw(|frame| app.draw(frame)).unwrap();
                    }
                    app.ascii = true;
                    terminal.draw(|frame| app.draw(frame)).unwrap();
                    app.ascii = false;
                }
            }
        }
        assert_eq!(app.dialog.unwrap().fields[0].value.as_str(), "Rozpracováno");
    }
    #[test]
    fn input_menu_theme_keeps_form_and_confirmation_defaults_to_cancel() {
        let mut app = App::new(&[]);
        app.store = None;
        app.profile_dialog(None);
        app.key(KeyEvent::new(K::F(10), M::NONE));
        app.menu = Some(19);
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.dialog.is_some());
        app.confirm = Some(Confirmation {
            action: Pending::Server(Change::CreateDatabase {
                name: "test".into(),
                collation: "utf8mb4_unicode_ci".into(),
            }),
            yes: false,
        });
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.confirm.is_none());
        assert!(!app.session.worker.busy);
    }
    #[test]
    fn menu_opens_with_f10_f9_alt_m_but_not_enter() {
        let mut app = App::new(&[]);
        app.store = None;
        app.page = 0;
        for key in [
            KeyEvent::new(K::F(10), M::NONE),
            KeyEvent::new(K::F(9), M::NONE),
            KeyEvent::new(K::Char('m'), M::ALT),
        ] {
            app.menu = None;
            app.key(key);
            assert!(app.menu.is_some(), "{key:?}");
        }
        app.menu = None;
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.menu.is_none());
    }
    #[test]
    fn newer_form_message_is_not_hidden_by_an_older_connection_error() {
        let mut app = App::new(&[]);
        app.store = None;
        // A refused TCP connection leaves a persistent session error (no snapshot).
        app.session.connect(
            Profile {
                transport: Transport::Tcp {
                    host: "127.0.0.1".into(),
                    port: 1,
                },
                tls: TlsMode::InsecureTestOnly,
                ..Default::default()
            },
            Secret::default(),
            false,
        );
        let start = std::time::Instant::now();
        while app.session.worker.busy && start.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.tick();
        }
        let error = app.session.error.clone().expect("connection error");
        assert_eq!(app.status_line(), error);
        app.profile_dialog(None);
        let dialog = app.dialog.as_mut().unwrap();
        dialog.fields[2].value = Zeroizing::new("není port".into());
        dialog.focus = dialog.fields.len() + 1; // "Uložit"
        key(&mut app, K::Enter);
        assert_eq!(app.status_line(), "Neplatný vstup: Neplatný port.");
    }
    #[test]
    fn enter_on_a_database_during_refresh_is_queued() {
        let mut app = App::new(&["--demo".to_string()]);
        app.store = None;
        wait_idle(&mut app);
        app.page = 1;
        app.selected = 0;
        assert!(app.session.request(Operation::Refresh));
        assert!(app.session.worker.busy);
        key(&mut app, K::Enter);
        assert!(app.session.has_pending());
        let start = std::time::Instant::now();
        while (app.session.worker.busy || app.session.has_pending())
            && start.elapsed() < std::time::Duration::from_secs(3)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.tick();
        }
        assert!(!app.session.tables.is_empty());
    }
    #[test]
    fn selection_follows_the_row_when_a_refresh_reorders_the_list() {
        let mut app = App::new(&["--demo".to_string()]);
        app.store = None;
        wait_idle(&mut app);
        app.page = 3;
        let target = app.processes()[1].id;
        app.selected = 1;
        app.remember_selection();
        // A new long query appears on top (processes sort by time).
        let mut snapshot = (**app.session.snapshot.as_ref().unwrap()).clone();
        let mut newcomer = snapshot.processes[0].clone();
        newcomer.id = 9999;
        newcomer.seconds = 10_000;
        snapshot.processes.insert(0, newcomer);
        app.session.snapshot = Some(std::sync::Arc::new(snapshot));
        app.reconcile_selection();
        assert_eq!(app.processes()[app.selected].id, target);
        // The row vanished: the cursor stays in range and picks a real row.
        let mut snapshot = (**app.session.snapshot.as_ref().unwrap()).clone();
        snapshot.processes.retain(|p| p.id != target);
        app.session.snapshot = Some(std::sync::Arc::new(snapshot));
        app.reconcile_selection();
        assert!(app.selected < app.processes().len());
        assert_eq!(
            app.selected_key,
            Some(RowKey::Process(app.processes()[app.selected].id))
        );
    }
    #[test]
    fn filters_are_per_page_and_case_insensitive() {
        let mut app = App::new(&["--demo".to_string()]);
        app.store = None;
        wait_idle(&mut app);
        app.page = 3;
        app.action(16);
        app.dialog.as_mut().unwrap().fields[0].value = Zeroizing::new("SELECT DATE".into());
        app.dialog.as_mut().unwrap().focus = 2; // "Zkontrolovat"
        key(&mut app, K::Enter);
        assert_eq!(
            app.processes().len(),
            1,
            "process query matched case-insensitively"
        );
        app.page = 1;
        assert!(
            !app.databases().is_empty(),
            "the database list has its own filter"
        );
        app.page = 2;
        app.filters[2] = "ANALYTICS".into();
        assert_eq!(app.accounts().len(), 1);
    }
    #[test]
    fn cancelled_confirmation_returns_the_same_draft_with_its_password() {
        let mut app = App::new(&["--demo".to_string()]);
        app.store = None;
        wait_idle(&mut app);
        app.action(12); // create account
        let dialog = app.dialog.as_mut().unwrap();
        dialog.fields[0].value = Zeroizing::new("novy".into());
        dialog.fields[2].value = Zeroizing::new("tajne".into());
        dialog.focus = dialog.fields.len() + 1;
        key(&mut app, K::Enter);
        assert!(app.confirm.is_some());
        assert!(app.dialog.is_none(), "dialog moved away, not copied");
        key(&mut app, K::Esc);
        assert_eq!(
            app.dialog.as_ref().unwrap().fields[2].value.as_str(),
            "tajne"
        );
        let dialog = app.dialog.as_mut().unwrap();
        dialog.focus = dialog.fields.len() + 1;
        key(&mut app, K::Enter);
        key(&mut app, K::Tab);
        key(&mut app, K::Enter);
        assert!(app.suspended.is_none() && app.dialog.is_none());
    }
    fn wait_idle(app: &mut App) {
        let start = std::time::Instant::now();
        app.tick();
        while app.session.worker.busy && start.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.tick();
        }
        assert!(app.session.snapshot.is_some());
    }
    fn key(app: &mut App, code: K) {
        app.key(KeyEvent::new(code, M::NONE));
    }
    #[test]
    fn switching_server_drops_account_bound_drafts_and_confirmation() {
        let mut app = App::new(&["--demo".to_string()]);
        app.store = None;
        wait_idle(&mut app);
        app.page = 2;
        app.selected = 1;
        app.action(13); // change password of the selected account
        assert!(matches!(
            app.dialog.as_ref().unwrap().kind,
            Kind::Password(_)
        ));
        key(&mut app, K::Esc); // the draft is kept as suspended
        assert!(app.suspended.is_some());
        app.tick();
        assert!(app.suspended.is_some(), "same server keeps the draft");
        let account = app.accounts()[1].clone();
        app.confirm = Some(Confirmation {
            action: Pending::Server(Change::ChangePassword {
                account,
                password: Secret::new("x".into()),
            }),
            yes: true,
        });
        app.action(9); // open DEMO again = new server session
        app.tick();
        assert!(app.suspended.is_none());
        assert!(app.dialog.is_none());
        assert!(app.confirm.is_none());
        assert_eq!(app.selected, 0);
        assert!(app.status.contains("zahozena"));
    }
    #[test]
    fn profiles_are_keyed_by_id_and_deletion_needs_confirmation() {
        let directory =
            std::env::temp_dir().join(format!("harbor-tui-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let mut app = App::new(&[]);
        app.store = Some(ConfigStore::at(directory.clone()));
        app.settings = Settings::default();
        let save = |app: &mut App, name: &str, existing: Option<Profile>| {
            app.profile_dialog(existing);
            let dialog = app.dialog.as_mut().unwrap();
            dialog.fields[0].value = name.to_string().into();
            dialog.focus = dialog.fields.len() + 1; // "Uložit"
            key(app, K::Enter);
        };
        save(&mut app, "A", None);
        // Saving the same open form again must update, not duplicate.
        key(&mut app, K::Enter);
        assert_eq!(app.settings.profiles.len(), 1);
        save(&mut app, "B", None);
        assert_eq!(app.settings.profiles.len(), 2);
        let a = app.settings.profiles[0].clone();
        save(&mut app, "b", Some(a.clone()));
        assert!(app.status.contains("už existuje"), "{}", app.status);
        assert_eq!(app.settings.profiles.len(), 2);
        assert_eq!(app.settings.profile(&a.id).unwrap().name, "A");
        app.dialog = None;
        app.page = 4;
        app.selected = 0;
        app.action(21);
        assert_eq!(
            app.settings.profiles.len(),
            2,
            "nothing deleted before confirmation"
        );
        key(&mut app, K::Enter); // default is Cancel
        assert_eq!(app.settings.profiles.len(), 2);
        assert!(
            app.dialog.is_none(),
            "cancel must not reopen an unrelated draft"
        );
        app.action(21);
        key(&mut app, K::Tab);
        key(&mut app, K::Enter);
        assert_eq!(app.settings.profiles.len(), 1);
        assert_eq!(app.settings.profiles[0].name, "B");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
