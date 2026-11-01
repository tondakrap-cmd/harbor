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
#[derive(Clone)]
struct Field {
    label: String,
    value: String,
    secret: bool,
}
impl Field {
    fn new(label: &str, value: &str) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            secret: false,
        }
    }
    fn password() -> Self {
        Self {
            label: "Heslo (pouze v paměti)".into(),
            value: String::new(),
            secret: true,
        }
    }
}
#[derive(Clone)]
enum Kind {
    Profile(Option<String>),
    Database,
    Account,
    Password(Account),
    Grant(Account),
    Search,
}
#[derive(Clone)]
struct Dialog {
    kind: Kind,
    fields: Vec<Field>,
    focus: usize,
}
struct Confirmation {
    change: Change,
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
    horizontal: usize,
    detail_scroll: u16,
    detail_focus: bool,
    menu: Option<usize>,
    help: bool,
    dialog: Option<Dialog>,
    suspended: Option<Dialog>,
    confirm: Option<Confirmation>,
    filter: String,
    system: bool,
    long: bool,
    detail: String,
    status: String,
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
            horizontal: 0,
            detail_scroll: 0,
            detail_focus: false,
            menu: None,
            help: false,
            dialog: None,
            suspended: None,
            confirm: None,
            filter: String::new(),
            system: false,
            long: false,
            detail: String::new(),
            status,
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
        app
    }
    pub fn tick(&mut self) {
        self.session.tick(self.settings.refresh_seconds);
    }
    fn save(&mut self, change: impl FnOnce(&mut Settings)) {
        match self
            .store
            .as_ref()
            .ok_or_else(|| Error::Config("Úložiště není dostupné.".into()))
            .and_then(|s| s.update(change))
        {
            Ok(settings) => self.settings = settings,
            Err(e) => self.status = e.to_string(),
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
        let status = self
            .session
            .error
            .as_deref()
            .or(self.session.test_result.as_deref())
            .unwrap_or(&self.status);
        frame.render_widget(
            Paragraph::new(safe_text(status))
                .wrap(Wrap { trim: false })
                .style(p.base),
            rows[3],
        );
        frame.render_widget(
            Paragraph::new(
                " F1 Pomoc  F3 Profily  F5 Obnovit  F10/Ctrl+M Menu  Esc Zpět  Ctrl+C Konec",
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
            frame.render_widget(Paragraph::new("F10 nebo Ctrl+M: všechny akce v nabídce\nŠipky / PageUp / PageDown: výběr a posuv\nEnter: otevřít detail, upravit profil nebo nastavení\nTab / Shift+Tab: další/předchozí pole nebo obrazovka\nVlevo/vpravo: vodorovný posuv seznamu\nEsc: zavřít; formulář zůstane rozpracovaný\nF3: profily, Enter: upravit uložený profil\nF5: obnovit data; Ctrl+C: ukončit\n\nZápisy vyžadují potvrzení; výchozí je Zrušit.\nDEMO je simulace bez skutečného serveru.\nTCP standardně vyžaduje ověřenou identitu TLS.\nBarvy 16 / truecolor / mono a ASCII: Nastavení.\nPři chybě zůstávají data označena jako neaktuální.\n\nEsc zavře nápovědu.").block(self.block(" Nápověda ", p)).style(p.base).wrap(Wrap { trim: false }), area);
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
            let query = match &confirm.change {
                Change::KillQuery { target } => {
                    format!("\n#{} {}", target.id, safe_text(&target.query))
                }
                _ => String::new(),
            };
            let text = format!(
                "Server: {}\n{}{}\n\n{}\n\n{}\nTab / šipky změní volbu, Enter potvrdí, Esc zruší",
                safe_text(profile),
                safe_text(&confirm.change.summary()),
                query,
                if self.session.snapshot.as_ref().is_some_and(|s| s.demo) {
                    "DEMO • pouze paměť"
                } else {
                    "Změna na připojeném serveru"
                },
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
        let text = format!("MariaDB {} | Uptime {} dní {} h\nRelace {}/{} | {} databází | {}\nQuestions/s: {} | Dlouhé dotazy: {}\nOS: Nezjištěno | Časové pásmo: {}", safe_text(&s.version), s.uptime/86400, s.uptime/3600%24, s.connections, s.max_connections, s.databases.len(), bytes_label(s.bytes()), rate, s.long_queries(), safe_text(&s.timezone));
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
                .block(self.block(" Questions/s • naměřeno od připojení ", p))
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
                            && d.name.to_lowercase().contains(&self.filter.to_lowercase())
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
                    .filter(|a| {
                        a.label()
                            .to_lowercase()
                            .contains(&self.filter.to_lowercase())
                    })
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
                            && (p.user.contains(&self.filter) || p.query.contains(&self.filter))
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
                        safe_text(&self.filter)
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
                    if self.detail_focus {
                        " > Detail • šipky posouvají • Tab zpět "
                    } else {
                        " Detail • Tab aktivuje posuv • F10 akce "
                    },
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
        if key.code == K::Char('c') && key.modifiers.contains(M::CONTROL) {
            self.quit = true;
            return;
        }
        if self.confirm.is_some() {
            self.confirm_key(key);
            return;
        }
        if key.code == K::F(10) || (key.code == K::Char('m') && key.modifiers.contains(M::CONTROL))
        {
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
                self.session.worker.send(Operation::Refresh);
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
                    if self.session.worker.send(Operation::Tables(name)) {
                        self.session.tables.clear();
                    }
                }
            }
            2 => {
                if let Some(account) = self.accounts().get(self.selected) {
                    let account = (*account).clone();
                    if self.session.worker.send(Operation::Grants(account)) {
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
                self.session.worker.send(Operation::Refresh);
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
                    self.status = "Nejprve vyberte účet na obrazovce Uživatelé.".into();
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
                    self.status = "Nejprve vyberte účet na obrazovce Uživatelé.".into();
                }
            }
            15 => {
                if self.page != 3 {
                    self.status = "Nejprve vyberte běžící dotaz na obrazovce Spojení.".into();
                    return;
                }
                let target = self.processes().get(self.selected).map(|p| (*p).clone());
                if let Some(target) = target.filter(|p| p.running_query()) {
                    self.confirm = Some(Confirmation {
                        change: Change::KillQuery { target },
                        yes: false,
                    });
                } else {
                    self.status = "Vyberte běžící dotaz na obrazovce Spojení.".into();
                }
            }
            16 => {
                self.dialog = Some(Dialog {
                    kind: Kind::Search,
                    fields: vec![Field::new("Hledat", &self.filter)],
                    focus: 0,
                })
            }
            17 => self.system = !self.system,
            18 => self.long = !self.long,
            19 => self.next_theme(),
            20 => self.dialog = self.suspended.take(),
            21 => {
                if self.page != 4 {
                    self.status = "Profil k odstranění vyberte na obrazovce Profily.".into();
                    return;
                }
                if let Some(profile) = self.settings.profiles.get(self.selected) {
                    let name = profile.name.clone();
                    self.save(|s| s.profiles.retain(|p| p.name != name));
                    self.selected = 0;
                }
            }
            22 => self.quit = true,
            _ => (),
        }
    }
    fn profile_dialog(&mut self, profile: Option<Profile>) {
        let original_name = profile.as_ref().map(|p| p.name.clone());
        let profile = profile.unwrap_or_default();
        let (host, port, socket) = match profile.transport {
            Transport::Tcp { host, port } => (host, port.to_string(), String::new()),
            Transport::Unix { path } => ("localhost".into(), "3306".into(), path),
        };
        self.dialog = Some(Dialog {
            kind: Kind::Profile(original_name),
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
                self.status = "Formulář zůstal rozpracovaný. Otevřete ho přes nabídku.".into();
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
                self.status = "Formulář ponechán rozpracovaný.".into();
                return;
            }
            K::Enter => match self.submit(&d, d.focus - d.fields.len()) {
                Ok(true) => return,
                Ok(false) => (),
                Err(error) => self.status = error.to_string(),
            },
            _ => (),
        }
        self.dialog = Some(d);
    }
    fn submit(&mut self, d: &Dialog, button: usize) -> Result<bool> {
        let v = |i: usize| d.fields[i].value.clone();
        let change = match &d.kind {
            Kind::Search => {
                self.filter = v(0);
                self.selected = 0;
                return Ok(true);
            }
            Kind::Profile(original_name) => {
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
                };
                profile.validate()?;
                if button == 1 {
                    self.save(|s| {
                        s.profiles.retain(|p| {
                            p.name != profile.name && Some(&p.name) != original_name.as_ref()
                        });
                        s.profiles.push(profile);
                    });
                    self.status = "Profil uložen. Heslo není součástí konfigurace.".into();
                    return Ok(false);
                }
                if button == 2 {
                    self.session.worker.send(Operation::TestConnection {
                        profile,
                        password: Secret::new(v(4)),
                    });
                    return Ok(false);
                }
                self.session.connect(profile, Secret::new(v(4)), false);
                self.page = 0;
                return Ok(true);
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
                password: Secret::new(v(2)),
            },
            Kind::Password(account) => Change::ChangePassword {
                account: account.clone(),
                password: Secret::new(v(0)),
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
        self.confirm = Some(Confirmation { change, yes: false });
        self.suspended = Some(d.clone());
        Ok(true)
    }
    fn confirm_key(&mut self, key: KeyEvent) {
        let Some(mut confirm) = self.confirm.take() else {
            return;
        };
        match key.code {
            K::Esc => {
                self.dialog = self.suspended.take();
                return;
            }
            K::Tab | K::BackTab | K::Left | K::Right => confirm.yes = !confirm.yes,
            K::Enter if !confirm.yes => {
                self.dialog = self.suspended.take();
                return;
            }
            K::Enter => {
                if self
                    .session
                    .worker
                    .send(Operation::Apply(confirm.change.clone()))
                {
                    self.suspended = None;
                    return;
                }
                self.status = "Jiná operace právě běží; potvrzení zůstává otevřené.".into();
            }
            _ => (),
        }
        self.confirm = Some(confirm);
    }
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
        app.dialog.as_mut().unwrap().fields[0].value = "Rozpracováno".into();
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
        assert_eq!(app.dialog.unwrap().fields[0].value, "Rozpracováno");
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
            change: Change::CreateDatabase {
                name: "test".into(),
                collation: "utf8mb4_unicode_ci".into(),
            },
            yes: false,
        });
        app.key(KeyEvent::new(K::Enter, M::NONE));
        assert!(app.confirm.is_none());
        assert!(!app.session.worker.busy);
    }
}
