use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use harbor_core::*;
use harbor_themes::{desktop, rgb, ThemeId};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

const PAGES: [&str; 8] = [
    "Přehled serveru",
    "Databáze",
    "Uživatelé a oprávnění",
    "Aktivní spojení",
    "Proměnné",
    "Doktor",
    "Připojení",
    "Nastavení",
];
const OVERVIEW: usize = 0;
const DATABASES: usize = 1;
const ACCOUNTS: usize = 2;
const PROCESSES: usize = 3;
const VARIABLES: usize = 4;
const HEALTH: usize = 5;
const CONNECTION: usize = 6;
const SETTINGS: usize = 7;

/// Forms that prepare one change; the change itself is confirmed in a modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Draft {
    CreateDatabase,
    CreateAccount,
    ChangePassword,
    CreateRole,
    RoleMembership,
    DefaultRole,
    SetVariable,
}
impl Draft {
    /// Drafts that target the selected account (dropped when it disappears).
    fn account_bound(self) -> bool {
        matches!(
            self,
            Self::ChangePassword | Self::RoleMembership | Self::DefaultRole
        )
    }
}
/// Checkbox editor of an account's privileges at one scope.
struct PrivilegeEditor {
    account: Account,
    /// Scope picked from the existing grants (keeps wildcard patterns verbatim).
    fixed: Option<GrantScope>,
    scope_text: String,
    /// Scope the checkboxes were initialised for.
    loaded_for: Option<GrantScope>,
    wanted: BTreeSet<String>,
}
impl PrivilegeEditor {
    fn scope(&self) -> Result<GrantScope> {
        match &self.fixed {
            Some(scope) => Ok(scope.clone()),
            None => GrantScope::parse(&self.scope_text),
        }
    }
}
struct PropertiesEditor {
    account: Account,
    locked: bool,
    expire: bool,
    tls: TlsRequirement,
    limits: Limits,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TableSort {
    Name,
    Rows,
    Size,
    Free,
}
pub struct Harbor {
    session: Session,
    store: Option<ConfigStore>,
    settings: Settings,
    theme: ThemeId,
    page: usize,
    /// One search per list page (index = page), so typing on one page does not
    /// filter another.
    filters: [String; 8],
    system: bool,
    long_only: bool,
    key_only: bool,
    changed_only: bool,
    selected_db: Option<String>,
    selected_table: Option<String>,
    table_sort: TableSort,
    selected_account: Option<Account>,
    selected_process: Option<Process>,
    selected_variable: Option<String>,
    profile: Profile,
    /// Saved-profile id waiting for delete confirmation.
    delete_profile: Option<String>,
    /// Worker generation the selections and drafts below belong to.
    scope_generation: u64,
    host: String,
    port: u16,
    socket: String,
    unix: bool,
    password: Zeroizing<String>,
    draft: Option<Draft>,
    change_db_name: String,
    change_user: String,
    change_host: String,
    change_password: Zeroizing<String>,
    collation: String,
    role_name: String,
    role_grant: bool,
    variable_value: String,
    privilege_editor: Option<PrivilegeEditor>,
    properties_editor: Option<PropertiesEditor>,
    /// Slider value while it is being dragged; persisted only on release.
    refresh_draft: Option<u64>,
    /// Theme whose egui style is currently installed (style is set only on change).
    styled: Option<ThemeId>,
    confirmation: Option<Change>,
    confirmation_focus: bool,
    /// Typed name that unlocks a destructive change.
    confirm_text: String,
    message: String,
    /// Generation for which the variables / health pages already loaded their data.
    loaded_variables: Option<u64>,
    loaded_health: Option<u64>,
}
#[derive(Clone, Copy)]
struct Palette {
    bg: Color32,
    surface: Color32,
    side: Color32,
    ink: Color32,
    muted: Color32,
    border: Color32,
    accent: Color32,
    tint: Color32,
    warn: Color32,
    radius: u8,
    terra: bool,
    clarity: bool,
}
impl Palette {
    fn new(theme: ThemeId) -> Self {
        let d = desktop(theme);
        let c = |key: &str| {
            let [r, g, b] = rgb(&d.colors[key]);
            Color32::from_rgb(r, g, b)
        };
        Self {
            bg: c("bg"),
            surface: c("surface"),
            side: c("side"),
            ink: c("ink"),
            muted: c("muted"),
            border: c("border"),
            accent: c("accent"),
            tint: c("tint"),
            warn: d
                .colors
                .get("warn")
                .map(|w| {
                    let [r, g, b] = rgb(w);
                    Color32::from_rgb(r, g, b)
                })
                .unwrap_or(Color32::from_rgb(0xB0, 0x5A, 0x00)),
            radius: d.shape.card_radius,
            terra: theme == ThemeId::Terra,
            clarity: theme == ThemeId::Clarity,
        }
    }
    fn severity(&self, severity: Severity) -> Color32 {
        match severity {
            Severity::Critical => Color32::from_rgb(0xC0, 0x23, 0x23),
            Severity::Warning => self.warn,
            Severity::Info => self.muted,
        }
    }
}
/// Light text on the dark Original sidebar: the navigation buttons must get dark fills too,
/// otherwise egui's light-theme button background makes them white on white.
const SIDEBAR_TEXT: Color32 = Color32::from_rgb(220, 238, 239);
fn dark_sidebar(v: &mut egui::Visuals, p: Palette) {
    v.override_text_color = Some(SIDEBAR_TEXT);
    let idle = Color32::from_rgb(0x1E, 0x3B, 0x44);
    for (widget, fill) in [
        (&mut v.widgets.inactive, idle),
        (&mut v.widgets.hovered, p.tint),
        (&mut v.widgets.active, p.tint),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = Stroke::new(1.0, p.tint);
        widget.fg_stroke.color = SIDEBAR_TEXT;
    }
    // Darker than the accent so the light label keeps WCAG AA contrast.
    v.selection.bg_fill = Color32::from_rgb(0x06, 0x66, 0x5E);
    v.selection.stroke = Stroke::new(1.0, SIDEBAR_TEXT);
}
fn card<R>(ui: &mut egui::Ui, p: Palette, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(p.radius)
        .shadow(if p.clarity {
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 16,
                spread: 0,
                color: Color32::from_black_alpha(12),
            }
        } else {
            egui::epaint::Shadow::NONE
        })
        .inner_margin(20.0)
        .show(ui, add)
        .inner
}
impl Harbor {
    pub fn new(ctx: &egui::Context, demo: bool) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        for (name, bytes) in [
            (
                "Harbor Sans",
                include_bytes!("../assets/LiberationSans-Regular.ttf").as_slice(),
            ),
            (
                "Harbor Serif",
                include_bytes!("../assets/LiberationSerif-Regular.ttf").as_slice(),
            ),
            (
                "Harbor Mono",
                include_bytes!("../assets/LiberationMono-Regular.ttf").as_slice(),
            ),
        ] {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_static(bytes).into());
        }
        fonts
            .families
            .get_mut(&FontFamily::Proportional)
            .expect("sans")
            .insert(0, "Harbor Sans".into());
        fonts.families.insert(
            FontFamily::Name("serif".into()),
            vec!["Harbor Serif".into()],
        );
        fonts
            .families
            .get_mut(&FontFamily::Monospace)
            .expect("mono")
            .insert(0, "Harbor Mono".into());
        ctx.set_fonts(fonts);
        let store = ConfigStore::standard().ok();
        let loaded = store.as_ref().map(ConfigStore::load).transpose();
        let message = loaded
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let settings = loaded.ok().flatten().unwrap_or_default();
        let theme = ThemeId::parse(&settings.desktop_theme)
            .filter(|t| *t != ThemeId::TurboClassic)
            .unwrap_or_default();
        let mut app = Self {
            session: Session::default(),
            store,
            settings,
            theme,
            page: if demo { OVERVIEW } else { CONNECTION },
            filters: Default::default(),
            system: false,
            long_only: false,
            key_only: true,
            changed_only: false,
            selected_db: None,
            selected_table: None,
            table_sort: TableSort::Size,
            selected_account: None,
            selected_process: None,
            selected_variable: None,
            profile: Profile::default(),
            delete_profile: None,
            scope_generation: 0,
            host: "localhost".into(),
            port: 3306,
            socket: "/run/mysqld/mysqld.sock".into(),
            unix: false,
            password: Zeroizing::new(String::new()),
            draft: None,
            change_db_name: String::new(),
            change_user: String::new(),
            change_host: "localhost".into(),
            change_password: Zeroizing::new(String::new()),
            collation: "utf8mb4_unicode_ci".into(),
            role_name: String::new(),
            role_grant: true,
            variable_value: String::new(),
            privilege_editor: None,
            properties_editor: None,
            refresh_draft: None,
            styled: None,
            confirmation: None,
            confirmation_focus: false,
            confirm_text: String::new(),
            message,
            loaded_variables: None,
            loaded_health: None,
        };
        if demo {
            app.session.connect(
                Profile {
                    name: "Ukázkový server".into(),
                    ..Default::default()
                },
                Secret::default(),
                true,
            );
        }
        app.scope_generation = app.session.worker.generation();
        app
    }
    fn drop_account_drafts(&mut self) {
        self.change_password.clear();
        if self.draft.is_some_and(Draft::account_bound) {
            self.draft = None;
        }
        self.privilege_editor = None;
        self.properties_editor = None;
        if matches!(
            self.confirmation,
            Some(
                Change::ChangePassword { .. }
                    | Change::Privileges { .. }
                    | Change::RoleMembership { .. }
                    | Change::DefaultRole { .. }
                    | Change::AlterAccount { .. }
                    | Change::DropAccount { .. }
            )
        ) {
            self.confirmation = None;
        }
    }
    /// Selections and account-bound drafts belong to one server. After connect/demo/
    /// disconnect they would silently target an object on a different server.
    fn sync_server_scope(&mut self) {
        let generation = self.session.worker.generation();
        if generation != self.scope_generation {
            self.scope_generation = generation;
            self.selected_db = None;
            self.selected_table = None;
            self.selected_account = None;
            self.selected_process = None;
            self.selected_variable = None;
            self.confirmation = None;
            self.drop_account_drafts();
            if self.draft == Some(Draft::SetVariable) {
                self.draft = None;
            }
        }
        let Some(snapshot) = self.session.snapshot.clone() else {
            return;
        };
        if self
            .selected_account
            .as_ref()
            .is_some_and(|a| !snapshot.accounts.contains(a))
        {
            self.selected_account = None;
            self.session.account = None;
            self.drop_account_drafts();
        }
        if self
            .selected_db
            .as_ref()
            .is_some_and(|name| !snapshot.databases.iter().any(|d| &d.name == name))
        {
            self.selected_db = None;
            self.selected_table = None;
            self.session.database = None;
            self.session.table = None;
            if matches!(
                self.confirmation,
                Some(Change::Maintenance { .. } | Change::DropDatabase { .. })
            ) {
                self.confirmation = None;
            }
        }
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
    fn try_save<T>(&mut self, change: impl FnOnce(&mut Settings) -> Result<T>) -> Option<T> {
        match self
            .store
            .as_ref()
            .ok_or_else(|| Error::Config("Úložiště není dostupné.".into()))
            .and_then(|s| s.try_update(change))
        {
            Ok((settings, value)) => {
                self.settings = settings;
                self.message.clear();
                Some(value)
            }
            Err(error) => {
                self.message = error.to_string();
                None
            }
        }
    }
    fn save_profile(&mut self) {
        let profile = self.profile.clone();
        if let Some(id) = self.try_save(move |s| s.upsert_profile(profile)) {
            self.profile.id = id;
        }
    }
    fn request_delete_profile(&mut self) {
        // By id of the loaded profile, never by the (possibly edited) name in the form.
        if self.settings.profile(&self.profile.id).is_some() {
            self.delete_profile = Some(self.profile.id.clone());
            self.confirmation_focus = true;
        } else {
            self.message = "Formulář neodpovídá žádnému uloženému profilu.".into();
        }
    }
    fn confirm_delete_profile(&mut self) {
        let Some(id) = self.delete_profile.take() else {
            return;
        };
        let target = id.clone();
        if self.try_save(move |s| s.remove_profile(&target)).is_some() && self.profile.id == id {
            // The form stays filled in as an unsaved draft.
            self.profile.id.clear();
        }
    }
    fn save(&mut self, change: impl FnOnce(&mut Settings)) {
        self.try_save(|settings| {
            change(settings);
            Ok(())
        });
    }
    /// Opens the confirmation modal for a change (with its SQL preview).
    fn confirm(&mut self, change: Change) {
        self.confirm_text.clear();
        self.confirmation = Some(change);
        self.confirmation_focus = true;
    }
    fn style(&self, ctx: &egui::Context, p: Palette) {
        let mut v = if self.theme == ThemeId::Nocturne {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = Some(p.ink);
        v.panel_fill = p.bg;
        v.window_fill = p.surface;
        v.extreme_bg_color = p.surface;
        v.selection.bg_fill = p.tint;
        v.selection.stroke = Stroke::new(1.0, p.ink);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            widget.corner_radius = p.radius.into();
            widget.bg_stroke = Stroke::new(1.0, p.border);
        }
        v.widgets.hovered.bg_fill = p.tint;
        v.widgets.active.bg_fill = p.tint;
        ctx.set_visuals(v);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(12.0, 10.0);
        style.spacing.button_padding = Vec2::new(14.0, 8.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(16.0));
        ctx.set_style(style);
    }
    fn connection(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.label("Uložené profily (hesla zůstávají pouze v paměti relace)");
        ui.horizontal_wrapped(|ui| {
            for profile in self.settings.profiles.clone() {
                if ui
                    .selectable_label(self.profile.id == profile.id, safe_text(&profile.name))
                    .clicked()
                {
                    self.load_profile(profile);
                }
            }
            if ui.button("+ Nový profil").clicked() {
                self.load_profile(Profile::default());
            }
        });
        card(ui, p, |ui| {
            ui.heading("Údaje o připojení");
            field(ui, "Název připojení", &mut self.profile.name, false);
            ui.checkbox(&mut self.unix, "Lokální Unix socket");
            if self.unix {
                field(ui, "Cesta k socketu", &mut self.socket, false);
            } else {
                field(ui, "Hostitel", &mut self.host, false);
                ui.horizontal(|ui| {
                    ui.label("Port");
                    ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                });
            }
            field(ui, "Uživatel", &mut self.profile.user, false);
            field(ui, "Heslo (neukládá se)", &mut self.password, true);
            if !self.unix {
                let mut insecure = self.profile.tls == TlsMode::InsecureTestOnly;
                ui.checkbox(
                    &mut insecure,
                    "Výslovně povolit nešifrované TCP pro testovací instanci",
                );
                self.profile.tls = if insecure {
                    TlsMode::InsecureTestOnly
                } else {
                    TlsMode::VerifyIdentity
                };
                let mut ca = self.profile.ca.clone().unwrap_or_default();
                field(ui, "Vlastní CA (volitelné, cesta PEM)", &mut ca, false);
                self.profile.ca = (!ca.is_empty()).then_some(ca);
            }
            self.profile.transport = if self.unix {
                Transport::Unix {
                    path: self.socket.clone(),
                }
            } else {
                Transport::Tcp {
                    host: self.host.clone(),
                    port: self.port,
                }
            };
            ui.label(self.profile.security_label());
            if let Some(result) = &self.session.test_result {
                ui.label(result);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button("Uložit profil").clicked() {
                    self.save_profile();
                }
                if ui
                    .add_enabled(
                        self.settings.profile(&self.profile.id).is_some(),
                        egui::Button::new("Odstranit uložený profil…"),
                    )
                    .clicked()
                {
                    self.request_delete_profile();
                }
                // Queued if a refresh is running; never silently dropped.
                if ui.button("Otestovat spojení").clicked() {
                    self.session.request(Operation::TestConnection {
                        profile: self.profile.clone(),
                        password: Secret(self.password.clone()),
                    });
                }
                // Connect cancels a running read; Session refuses it only during a write.
                if ui.button("Připojit").clicked() {
                    self.session.connect(
                        self.profile.clone(),
                        Secret(std::mem::take(&mut self.password)),
                        false,
                    );
                    self.page = OVERVIEW;
                }
                if ui.button("Otevřít DEMO").clicked() {
                    self.session
                        .connect(self.profile.clone(), Secret::default(), true);
                    self.page = OVERVIEW;
                }
                if ui.button("Odpojit").clicked() {
                    self.session.disconnect();
                }
            });
        });
    }
    fn load_profile(&mut self, profile: Profile) {
        match &profile.transport {
            Transport::Tcp { host, port } => {
                self.host = host.clone();
                self.port = *port;
                self.unix = false;
            }
            Transport::Unix { path } => {
                self.socket = path.clone();
                self.unix = true;
            }
        };
        self.profile = profile;
        self.password.clear();
    }
    fn overview(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(s) = self.session.snapshot.clone() else {
            ui.label("Připojte testovací server nebo otevřete označené demo v Připojení.");
            return;
        };
        let rate = self
            .session
            .qps
            .map(|q| format!("{q:.1}"))
            .unwrap_or_else(|| "Čekám na druhý vzorek".into());
        let uptime = format!("{} dní {:02} h", s.uptime / 86400, s.uptime / 3600 % 24);
        let connections = format!("{} / {}", s.connections, s.max_connections);
        let bytes = bytes_label(s.bytes());
        if self.theme == ThemeId::Nocturne {
            ui.columns(2, |cols| {
                metric(
                    &mut cols[0],
                    Palette {
                        surface: p.tint,
                        ..p
                    },
                    "AKTUÁLNÍ PROVOZ • dotazů/s",
                    &rate,
                    44.0,
                );
                metric(&mut cols[1], p, "AKTIVNÍ SPOJENÍ", &connections, 30.0);
            });
        } else {
            ui.columns(4, |cols| {
                for (column, (label, value)) in cols.iter_mut().zip([
                    ("DOSTUPNOST", &uptime),
                    ("DOTAZY ZA SEKUNDU", &rate),
                    ("AKTIVNÍ SPOJENÍ", &connections),
                    ("VELIKOST DATABÁZÍ", &bytes),
                ]) {
                    metric(column, p, label, value, 26.0);
                }
            });
        }
        ui.add_space(12.0);
        card(ui, p, |ui| {
            ui.heading("Provoz serveru");
            ui.label(
                "Dotazy klientů za sekundu • bez dotazů Harboru • měřeno od připojení • nejvýše 120 vzorků",
            );
            chart(
                ui,
                &self.session.history.iter().copied().collect::<Vec<_>>(),
                p,
            );
        });
        if self.theme == ThemeId::Nocturne {
            ui.columns(2, |cols| {
                metric(&mut cols[0], p, "DOSTUPNOST", &uptime, 26.0);
                metric(&mut cols[1], p, "DATABÁZE", &bytes, 26.0);
            });
        }
        card(ui, p, |ui| {
            ui.heading("Informace o instanci");
            ui.label(format!("MariaDB: {}", safe_text(&s.version)));
            ui.label("Operační systém: Nezjištěno");
            ui.label(format!("Časové pásmo serveru: {}", safe_text(&s.timezone)));
            if let Some(profile) = &self.session.profile {
                ui.label(format!(
                    "Účet: {} • {}",
                    safe_text(&profile.user),
                    if s.demo {
                        "DEMO bez spojení"
                    } else {
                        profile.security_label()
                    }
                ));
            }
            ui.label(format!(
                "{} viditelných databází • {} dotazů běží alespoň 30 sekund",
                s.databases.len(),
                s.long_queries()
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Zobrazit aktivní spojení →").clicked() {
                    self.page = PROCESSES;
                }
                if ui.button("Spustit Doktora →").clicked() {
                    self.page = HEALTH;
                }
            });
        });
    }
    fn databases(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[DATABASES]);
            ui.checkbox(&mut self.system, "Systémové databáze");
            if ui.button("+ Vytvořit databázi").clicked() {
                self.draft = Some(Draft::CreateDatabase);
            }
            let selected = self
                .selected_db
                .clone()
                .filter(|name| !Database::system_name(name));
            if ui
                .add_enabled(
                    selected.is_some(),
                    egui::Button::new("Smazat vybranou databázi…"),
                )
                .clicked()
            {
                if let Some(name) = selected {
                    self.confirm(Change::DropDatabase { name });
                }
            }
        });
        // Arc clone: the frame keeps the rows without copying them.
        let snapshot = self.session.snapshot.clone();
        let databases = snapshot.as_deref().map_or(&[][..], |s| &s.databases);
        card(ui, p, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("databases")
                .show(ui, |ui| {
                    egui::Grid::new("databases")
                        .striped(true)
                        .min_row_height(34.0)
                        .show(ui, |ui| {
                            for h in [
                                "NÁZEV",
                                "TABULKY",
                                "VELIKOST (ODHAD)",
                                "ZNAKOVÁ SADA",
                                "KOLACE",
                            ] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            let visible: Vec<&Database> = databases
                                .iter()
                                .filter(|d| self.database_visible(d))
                                .collect();
                            for db in visible {
                                if ui
                                    .selectable_label(
                                        self.selected_db.as_ref() == Some(&db.name),
                                        safe_text(&db.name),
                                    )
                                    .clicked()
                                    && self.session.request(Operation::Tables(db.name.clone()))
                                {
                                    self.selected_db = Some(db.name.clone());
                                    self.selected_table = None;
                                }
                                ui.label(db.tables.to_string());
                                ui.label(bytes_label(db.bytes));
                                ui.label(safe_text(&db.charset));
                                ui.label(safe_text(&db.collation));
                                ui.end_row();
                            }
                        });
                });
        });
        let Some(name) = self.selected_db.clone() else {
            return;
        };
        let Some(detail) = self.session.database.clone().filter(|d| d.name == name) else {
            card(ui, p, |ui| {
                ui.heading(format!("Databáze {}", safe_text(&name)));
                ui.label("Načítám podrobnosti…");
            });
            return;
        };
        card(ui, p, |ui| {
            ui.heading(format!("Databáze {}", safe_text(&detail.name)));
            ui.label(format!(
                "Kolace {} • {} tabulek • {} pohledů • {} procedur/funkcí • {} triggerů • {} událostí",
                safe_text(&detail.collation),
                detail.base_tables().count(),
                detail.views,
                detail.routines,
                detail.triggers,
                detail.events
            ));
            ui.label(format!(
                "Data {} • indexy {} • nevyužité místo {} • údaje jsou odhady InnoDB",
                human_bytes(detail.data_bytes()),
                human_bytes(detail.index_bytes()),
                human_bytes(detail.free_bytes())
            ));
            for warning in detail.warnings() {
                ui.label(RichText::new(format!("⚠ {}", safe_text(&warning))).color(p.warn));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Řadit:");
                for (sort, label) in [
                    (TableSort::Size, "velikost"),
                    (TableSort::Rows, "řádky"),
                    (TableSort::Free, "nevyužité místo"),
                    (TableSort::Name, "název"),
                ] {
                    ui.radio_value(&mut self.table_sort, sort, label);
                }
            });
            let mut tables: Vec<&TableInfo> = detail.tables.iter().collect();
            sort_tables(&mut tables, self.table_sort);
            egui::ScrollArea::horizontal()
                .id_salt("tables")
                .show(ui, |ui| {
                    egui::Grid::new("tables")
                        .striped(true)
                        .min_row_height(30.0)
                        .show(ui, |ui| {
                            for h in [
                                "TABULKA",
                                "ENGINE",
                                "ŘÁDKY",
                                "DATA",
                                "INDEXY",
                                "NEVYUŽITO",
                                "KOLACE",
                                "UPOZORNĚNÍ",
                            ] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for table in tables {
                                if ui
                                    .selectable_label(
                                        self.selected_table.as_ref() == Some(&table.name),
                                        safe_text(&table.name),
                                    )
                                    .clicked()
                                    && self.session.request(Operation::Table {
                                        database: detail.name.clone(),
                                        table: table.name.clone(),
                                    })
                                {
                                    self.selected_table = Some(table.name.clone());
                                }
                                if table.view() {
                                    ui.label("pohled");
                                    for _ in 0..5 {
                                        ui.label("");
                                    }
                                } else {
                                    ui.label(safe_text(&table.engine));
                                    ui.label(format!("≈ {}", table.rows));
                                    ui.label(human_bytes(table.data_bytes));
                                    ui.label(human_bytes(table.index_bytes));
                                    ui.label(human_bytes(table.free_bytes));
                                    ui.label(safe_text(&table.collation));
                                }
                                ui.label(RichText::new(table.warnings().join(", ")).color(p.warn));
                                ui.end_row();
                            }
                        });
                });
        });
        if let Some(table) = self.selected_table.clone() {
            self.table_card(ui, p, &detail.name, &table);
        }
    }
    fn table_card(&mut self, ui: &mut egui::Ui, p: Palette, database: &str, table: &str) {
        let detail = self
            .session
            .table
            .clone()
            .filter(|t| t.database == database && t.table == table);
        let view = self
            .session
            .tables()
            .iter()
            .any(|t| t.name == table && t.view());
        card(ui, p, |ui| {
            ui.heading(format!(
                "Tabulka {}.{}",
                safe_text(database),
                safe_text(table)
            ));
            let Some(detail) = detail else {
                ui.label("Načítám strukturu…");
                return;
            };
            ui.strong("Sloupce");
            egui::ScrollArea::horizontal()
                .id_salt("columns")
                .show(ui, |ui| {
                    egui::Grid::new("columns").striped(true).show(ui, |ui| {
                        for h in [
                            "SLOUPEC",
                            "TYP",
                            "NULL",
                            "VÝCHOZÍ",
                            "KLÍČ",
                            "EXTRA",
                            "KOMENTÁŘ",
                        ] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for c in &detail.columns {
                            ui.label(safe_text(&c.name));
                            ui.monospace(safe_text(&c.column_type));
                            ui.label(if c.nullable { "ano" } else { "ne" });
                            ui.label(
                                c.default
                                    .as_deref()
                                    .map(safe_text)
                                    .unwrap_or_else(|| "—".into()),
                            );
                            ui.label(safe_text(&c.key));
                            ui.label(safe_text(&c.extra));
                            ui.label(safe_text(&c.comment));
                            ui.end_row();
                        }
                    });
                });
            if !detail.indexes.is_empty() {
                ui.strong("Indexy");
                for index in &detail.indexes {
                    ui.label(format!(
                        "{}{} ({}) • {} • kardinalita ≈ {}",
                        safe_text(&index.name),
                        if index.unique { " UNIQUE" } else { "" },
                        safe_text(&index.columns.join(", ")),
                        safe_text(&index.index_type),
                        index.cardinality
                    ));
                }
            }
            if !detail.foreign_keys.is_empty() {
                ui.strong("Cizí klíče");
                for fk in &detail.foreign_keys {
                    ui.label(format!(
                        "{} ({}) → {} ({}) • ON UPDATE {} • ON DELETE {}",
                        safe_text(&fk.name),
                        safe_text(&fk.columns.join(", ")),
                        safe_text(&fk.referenced_table),
                        safe_text(&fk.referenced_columns.join(", ")),
                        safe_text(&fk.on_update),
                        safe_text(&fk.on_delete)
                    ));
                }
            }
            egui::CollapsingHeader::new("Definice (SHOW CREATE)")
                .id_salt("ddl")
                .show(ui, |ui| {
                    ui.monospace(safe_text(&detail.create_sql));
                });
            if !view {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Údržba:");
                    for action in Maintenance::ALL {
                        if ui
                            .button(action.keyword())
                            .on_hover_text(action.description())
                            .clicked()
                        {
                            self.confirm(Change::Maintenance {
                                database: database.into(),
                                table: table.into(),
                                action,
                            });
                        }
                    }
                });
            }
            if !self.session.change_output.is_empty() {
                ui.strong("Výsledek poslední operace");
                for line in &self.session.change_output {
                    ui.monospace(safe_text(line));
                }
            }
        });
    }
    fn accounts(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[ACCOUNTS]);
            if ui.button("+ Vytvořit účet").clicked() {
                self.draft = Some(Draft::CreateAccount);
            }
            if ui.button("+ Vytvořit roli").clicked() {
                self.draft = Some(Draft::CreateRole);
            }
        });
        let snapshot = self.session.snapshot.clone();
        let accounts = snapshot.as_deref().map_or(&[][..], |s| &s.accounts);
        card(ui, p, |ui| {
            egui::Grid::new("accounts")
                .striped(true)
                .min_row_height(34.0)
                .show(ui, |ui| {
                    ui.strong("ÚČET / ROLE");
                    ui.strong("HOSTITEL");
                    ui.strong("AUTENTIZACE");
                    ui.end_row();
                    let visible: Vec<&Account> = accounts
                        .iter()
                        .filter(|a| self.account_visible(a))
                        .collect();
                    for account in visible {
                        if ui
                            .selectable_label(
                                self.selected_account.as_ref() == Some(account),
                                safe_text(&account.user),
                            )
                            .clicked()
                            && self.session.request(Operation::Grants(account.clone()))
                        {
                            if self.selected_account.as_ref() != Some(account) {
                                self.drop_account_drafts();
                            }
                            self.selected_account = Some(account.clone());
                        }
                        ui.label(if account.is_role() {
                            "role".into()
                        } else {
                            safe_text(&account.host)
                        });
                        ui.label(safe_text(&account.plugin));
                        ui.end_row();
                    }
                });
        });
        let Some(account) = self.selected_account.clone() else {
            return;
        };
        let detail = self
            .session
            .account
            .clone()
            .filter(|d| d.account.as_ref() == Some(&account));
        card(ui, p, |ui| {
            ui.heading(safe_text(&account.label()));
            let Some(detail) = detail else {
                ui.label("Načítám oprávnění…");
                return;
            };
            if let Some(props) = &detail.properties {
                ui.label(format!(
                    "Autentizace {} • {} • heslo {} • TLS {}",
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
                ui.label(format!(
                    "Limity (0 = bez limitu): dotazy/h {} • změny/h {} • připojení/h {} • souběžná spojení {}",
                    l.queries_per_hour, l.updates_per_hour, l.connections_per_hour, l.user_connections
                ));
            } else if !account.is_role() {
                ui.label(
                    RichText::new("Vlastnosti účtu nejsou dostupné (chybí právo čtení mysql.*).")
                        .color(p.muted),
                );
            }
            ui.label(format!(
                "Role: {} • výchozí role: {}",
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
            ui.horizontal_wrapped(|ui| {
                if !account.is_role() {
                    if ui.button("Změnit heslo…").clicked() {
                        self.draft = Some(Draft::ChangePassword);
                    }
                    if ui
                        .add_enabled(
                            detail.properties.is_some(),
                            egui::Button::new("Vlastnosti a limity…"),
                        )
                        .clicked()
                    {
                        if let Some(props) = &detail.properties {
                            self.properties_editor = Some(PropertiesEditor {
                                account: account.clone(),
                                locked: props.locked,
                                expire: false,
                                tls: props.tls,
                                limits: props.limits,
                            });
                        }
                    }
                    if ui.button("Výchozí role…").clicked() {
                        self.role_name = detail.default_role.clone().unwrap_or_default();
                        self.draft = Some(Draft::DefaultRole);
                    }
                }
                if ui.button("Přidělit / odebrat roli…").clicked() {
                    self.draft = Some(Draft::RoleMembership);
                }
                if ui.button("Smazat…").clicked() {
                    self.confirm(Change::DropAccount {
                        account: account.clone(),
                    });
                }
            });
            ui.separator();
            ui.strong("Oprávnění");
            egui::Grid::new("privileges")
                .striped(true)
                .min_row_height(30.0)
                .show(ui, |ui| {
                    ui.strong("ROZSAH");
                    ui.strong("OPRÁVNĚNÍ");
                    ui.strong("GRANT OPTION");
                    ui.strong("");
                    ui.end_row();
                    for set in &detail.privileges {
                        ui.label(safe_text(&set.scope.label()));
                        ui.label(if set.privileges.is_empty() {
                            "USAGE".into()
                        } else {
                            safe_text(
                                &set.privileges
                                    .iter()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            )
                        });
                        ui.label(if set.grantable { "ano" } else { "" });
                        if ui.button("Upravit…").clicked() {
                            self.privilege_editor = Some(PrivilegeEditor {
                                account: account.clone(),
                                fixed: Some(set.scope.clone()),
                                scope_text: String::new(),
                                loaded_for: None,
                                wanted: BTreeSet::new(),
                            });
                        }
                        ui.end_row();
                    }
                });
            if detail.privileges.is_empty() {
                ui.label("Žádná přímá oprávnění (jen přes role, pokud nějaké má).");
            }
            for line in &detail.columns {
                ui.label(format!("Sloupec: {}", safe_text(line)));
            }
            if ui
                .button("+ Oprávnění na další rozsah / šablona…")
                .clicked()
            {
                self.privilege_editor = Some(PrivilegeEditor {
                    account: account.clone(),
                    fixed: None,
                    scope_text: self
                        .selected_db
                        .as_ref()
                        .map(|db| format!("{db}.*"))
                        .unwrap_or_else(|| "*.*".into()),
                    loaded_for: None,
                    wanted: BTreeSet::new(),
                });
            }
            egui::CollapsingHeader::new("SHOW GRANTS")
                .id_salt("show_grants")
                .show(ui, |ui| {
                    for grant in &detail.grants {
                        ui.monospace(safe_text(grant));
                    }
                });
        });
    }
    fn processes(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[PROCESSES]);
            ui.checkbox(
                &mut self.long_only,
                "Jen běžící dotazy nad 30 sekund (Sleep se nepočítá)",
            );
        });
        let snapshot = self.session.snapshot.clone();
        let processes = snapshot.as_deref().map_or(&[][..], |s| &s.processes);
        card(ui, p, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("processes")
                .show(ui, |ui| {
                    egui::Grid::new("processes")
                        .striped(true)
                        .min_row_height(34.0)
                        .show(ui, |ui| {
                            for h in ["ID / UŽIVATEL", "DATABÁZE", "ČAS", "PŘÍKAZ", "STAV"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            let visible: Vec<&Process> = processes
                                .iter()
                                .filter(|p| self.process_visible(p))
                                .collect();
                            for process in visible {
                                if ui
                                    .selectable_label(
                                        self.selected_process.as_ref().map(|p| p.id)
                                            == Some(process.id),
                                        format!("{} • {}", process.id, safe_text(&process.user)),
                                    )
                                    .clicked()
                                {
                                    self.selected_process = Some(process.clone());
                                }
                                ui.label(safe_text(&process.database));
                                ui.label(format!("{} s", process.seconds));
                                ui.label(safe_text(&process.command));
                                ui.label(safe_text(&process.state));
                                ui.end_row();
                            }
                        });
                });
        });
        if let Some(process) = self.selected_process.clone() {
            card(ui, p, |ui| {
                ui.heading(format!("Dotaz #{}", process.id));
                ui.monospace(safe_text(&process.query));
                if ui
                    .add_enabled(process.running_query(), egui::Button::new("Ukončit dotaz…"))
                    .clicked()
                {
                    self.confirm(Change::KillQuery { target: process });
                }
            });
        }
    }
    fn variables(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[VARIABLES]);
            ui.checkbox(&mut self.key_only, "Jen klíčové parametry");
            ui.checkbox(&mut self.changed_only, "Jen odlišné od výchozích");
            if ui.button("Načíst znovu").clicked() {
                self.session.request(Operation::Variables);
            }
        });
        ui.label(
            RichText::new(
                "Změna přes SET GLOBAL platí okamžitě, ale jen do restartu serveru. Trvalé uložení do konfigurace přinese verze 0.4.",
            )
            .color(p.muted),
        );
        let Some(variables) = self.session.variables.clone() else {
            ui.label(if self.session.snapshot.is_some() {
                "Načítám proměnné…"
            } else {
                "Nejprve se připojte k serveru."
            });
            return;
        };
        let visible: Vec<&Variable> = variables
            .iter()
            .filter(|v| self.variable_visible(v))
            .collect();
        card(ui, p, |ui| {
            ui.label(format!(
                "{} z {} proměnných",
                visible.len(),
                variables.len()
            ));
            egui::ScrollArea::horizontal()
                .id_salt("variables")
                .show(ui, |ui| {
                    egui::Grid::new("variables")
                        .striped(true)
                        .min_row_height(30.0)
                        .show(ui, |ui| {
                            for h in ["PROMĚNNÁ", "HODNOTA", "PŮVOD", "ZMĚNA ZA BĚHU"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for variable in &visible {
                                if ui
                                    .selectable_label(
                                        self.selected_variable.as_ref() == Some(&variable.name),
                                        safe_text(&variable.name),
                                    )
                                    .clicked()
                                {
                                    self.selected_variable = Some(variable.name.clone());
                                }
                                ui.monospace(clip(&variable_value(variable), 60));
                                let origin = origin_label(variable);
                                if variable.runtime_only() {
                                    ui.label(RichText::new(origin).color(p.warn));
                                } else {
                                    ui.label(origin);
                                }
                                ui.label(if variable.settable() { "ano" } else { "ne" });
                                ui.end_row();
                            }
                        });
                });
        });
        let Some(variable) = self
            .selected_variable
            .as_ref()
            .and_then(|name| variables.iter().find(|v| &v.name == name))
            .cloned()
        else {
            return;
        };
        card(ui, p, |ui| {
            ui.heading(safe_text(&variable.name));
            if let Some(description) = variable.key_description() {
                ui.label(description);
            }
            if !variable.comment.is_empty() {
                ui.label(RichText::new(safe_text(&variable.comment)).color(p.muted));
            }
            ui.monospace(format!(
                "Hodnota: {}",
                safe_text(&variable_value(&variable))
            ));
            ui.label(format!(
                "Výchozí: {} • původ: {} • typ: {}",
                variable
                    .default
                    .as_deref()
                    .map(safe_text)
                    .unwrap_or_else(|| "—".into()),
                origin_label(&variable),
                safe_text(&variable.var_type)
            ));
            if variable.min.is_some() || variable.max.is_some() {
                ui.label(format!(
                    "Rozsah: {} až {}",
                    variable.min.as_deref().unwrap_or("?"),
                    variable.max.as_deref().unwrap_or("?")
                ));
            }
            if !variable.allowed.is_empty() {
                ui.label(format!(
                    "Povolené hodnoty: {}",
                    safe_text(&variable.allowed.join(", "))
                ));
            }
            if variable.settable() {
                if ui.button("Změnit hodnotu (SET GLOBAL)…").clicked() {
                    self.variable_value = variable.value.clone();
                    self.draft = Some(Draft::SetVariable);
                }
            } else {
                ui.label("Tuto proměnnou nelze měnit za běhu; patří do konfiguračního souboru.");
            }
        });
    }
    fn health(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Spustit kontrolu").clicked() {
                self.session.request(Operation::Health);
            }
            if let Some((at, _)) = &self.session.health {
                ui.label(format!(
                    "Poslední kontrola před {} s",
                    at.elapsed().as_secs()
                ));
            }
        });
        ui.label(
            RichText::new(
                "Kontrola čte stav serveru (účty, statistiky, tabulky); nic nemění. Rozsah závisí na oprávněních účtu.",
            )
            .color(p.muted),
        );
        let Some((_, findings)) = self.session.health.clone() else {
            ui.label(if self.session.snapshot.is_some() {
                "Probíhá kontrola…"
            } else {
                "Nejprve se připojte k serveru."
            });
            return;
        };
        if findings.is_empty() {
            card(ui, p, |ui| {
                ui.heading("Bez nálezů");
                ui.label("Žádné kontrolované pravidlo nehlásí problém.");
            });
        }
        for finding in &findings {
            card(ui, p, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(finding.severity.label())
                            .strong()
                            .color(p.severity(finding.severity)),
                    );
                    ui.heading(safe_text(&finding.title));
                });
                ui.label(safe_text(&finding.detail));
                ui.label(
                    RichText::new(format!(
                        "Doporučení: {}",
                        safe_text(&finding.recommendation)
                    ))
                    .color(p.muted),
                );
            });
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui, p: Palette) {
        card(ui, p, |ui| {
            ui.heading("Vzhled desktopu");
            for id in ThemeId::DESKTOP {
                if ui.radio_value(&mut self.theme, id, id.label()).changed() {
                    self.save(move |s| s.desktop_theme = id.key().into());
                }
            }
            ui.label("Spojení, rozpracované formuláře i výběr se při přepnutí zachovají.");
            let mut refresh = self.refresh_draft.unwrap_or(self.settings.refresh_seconds);
            let slider = ui.add(egui::Slider::new(&mut refresh, 1..=300).text("Obnova (s)"));
            self.refresh_slider(
                refresh,
                slider.dragged(),
                slider.drag_stopped(),
                slider.changed(),
            );
        });
    }
    fn database_visible(&self, d: &Database) -> bool {
        (self.system || !d.system())
            && matches_filter(
                &self.filters[DATABASES],
                &[&d.name, &d.charset, &d.collation],
            )
    }
    fn account_visible(&self, a: &Account) -> bool {
        matches_filter(&self.filters[ACCOUNTS], &[&a.user, &a.host, &a.plugin])
    }
    fn process_visible(&self, p: &Process) -> bool {
        (!self.long_only || p.long_query())
            && matches_filter(
                &self.filters[PROCESSES],
                &[&p.user, &p.host, &p.database, &p.command, &p.query],
            )
    }
    fn variable_visible(&self, v: &Variable) -> bool {
        (!self.key_only || v.key_description().is_some())
            && (!self.changed_only || v.changed())
            && matches_filter(&self.filters[VARIABLES], &[&v.name, &v.value, &v.comment])
    }
    /// Dragging only updates the draft; the config file (with fsync) is written once,
    /// on release or on a keyboard change, not for every pixel of a drag.
    fn refresh_slider(&mut self, value: u64, dragging: bool, released: bool, changed: bool) {
        if dragging && !released {
            self.refresh_draft = Some(value);
            return;
        }
        if released || changed {
            self.refresh_draft = None;
            if value != self.settings.refresh_seconds {
                self.save(move |s| s.refresh_seconds = value);
            }
        }
    }
    /// Role names on the connected server.
    fn roles(&self) -> Vec<String> {
        self.session
            .snapshot
            .as_ref()
            .map(|s| {
                s.accounts
                    .iter()
                    .filter(|a| a.is_role())
                    .map(|a| a.user.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    /// The change a draft form describes (None when it lacks its target).
    fn draft_change(&self, draft: Draft) -> Option<Change> {
        Some(match draft {
            Draft::CreateDatabase => Change::CreateDatabase {
                name: self.change_db_name.clone(),
                collation: self.collation.clone(),
            },
            Draft::CreateAccount => Change::CreateAccount {
                user: self.change_user.clone(),
                host: self.change_host.clone(),
                password: Secret(self.change_password.clone()),
            },
            Draft::ChangePassword => Change::ChangePassword {
                account: self.selected_account.clone()?,
                password: Secret(self.change_password.clone()),
            },
            Draft::CreateRole => Change::CreateRole {
                name: self.role_name.clone(),
            },
            Draft::RoleMembership => Change::RoleMembership {
                role: self.role_name.clone(),
                account: self.selected_account.clone()?,
                grant: self.role_grant,
            },
            Draft::DefaultRole => Change::DefaultRole {
                account: self.selected_account.clone()?,
                role: (!self.role_name.is_empty()).then(|| self.role_name.clone()),
            },
            Draft::SetVariable => {
                let name = self.selected_variable.as_ref()?;
                let variable = self
                    .session
                    .variables
                    .as_ref()?
                    .iter()
                    .find(|v| &v.name == name)?
                    .clone();
                Change::SetGlobal {
                    variable,
                    value: self.variable_value.clone(),
                }
            }
        })
    }
    fn draft_window(&mut self, ctx: &egui::Context, draft: Draft) {
        let title = match draft {
            Draft::CreateDatabase => "Vytvořit databázi",
            Draft::CreateAccount => "Vytvořit účet",
            Draft::ChangePassword => "Změnit heslo",
            Draft::CreateRole => "Vytvořit roli",
            Draft::RoleMembership => "Přidělit nebo odebrat roli",
            Draft::DefaultRole => "Výchozí role",
            Draft::SetVariable => "Změnit proměnnou",
        };
        let roles = self.roles();
        egui::Window::new(title)
            .id(egui::Id::new("draft"))
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                match draft {
                    Draft::CreateDatabase => {
                        field(ui, "Název databáze", &mut self.change_db_name, false);
                        field(ui, "Kolace (ověří server)", &mut self.collation, false);
                    }
                    Draft::CreateAccount => {
                        field(ui, "Uživatel", &mut self.change_user, false);
                        field(
                            ui,
                            "Hostitel účtu (% = odkudkoli)",
                            &mut self.change_host,
                            false,
                        );
                        field(ui, "Heslo", &mut self.change_password, true);
                    }
                    Draft::ChangePassword => {
                        ui.label(
                            self.selected_account
                                .as_ref()
                                .map(|a| safe_text(&a.label()))
                                .unwrap_or_default(),
                        );
                        field(ui, "Nové heslo", &mut self.change_password, true);
                    }
                    Draft::CreateRole => field(ui, "Název role", &mut self.role_name, false),
                    Draft::RoleMembership | Draft::DefaultRole => {
                        ui.label(
                            self.selected_account
                                .as_ref()
                                .map(|a| safe_text(&a.label()))
                                .unwrap_or_default(),
                        );
                        if roles.is_empty() {
                            ui.label("Na serveru není žádná role (nebo ji účet nevidí).");
                        }
                        ui.horizontal_wrapped(|ui| {
                            if draft == Draft::DefaultRole {
                                ui.radio_value(&mut self.role_name, String::new(), "žádná");
                            }
                            for role in &roles {
                                ui.radio_value(&mut self.role_name, role.clone(), safe_text(role));
                            }
                        });
                        if draft == Draft::RoleMembership {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut self.role_grant, true, "Přidělit");
                                ui.radio_value(&mut self.role_grant, false, "Odebrat");
                            });
                        }
                    }
                    Draft::SetVariable => {
                        ui.label(self.selected_variable.clone().unwrap_or_default());
                        field(
                            ui,
                            "Nová hodnota (velikosti lze s příponou K/M/G)",
                            &mut self.variable_value,
                            false,
                        );
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Ponechat rozpracované a zavřít").clicked() {
                        self.draft = None;
                    }
                    if ui.button("Zkontrolovat změnu…").clicked() {
                        match self.draft_change(draft) {
                            Some(change) => self.confirm(change),
                            None => self.message = "Formulář nemá vybraný cíl.".into(),
                        }
                    }
                });
            });
    }
    fn privilege_window(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.privilege_editor.take() else {
            return;
        };
        let detail = self
            .session
            .account
            .clone()
            .filter(|d| d.account.as_ref() == Some(&editor.account));
        let scope = editor.scope();
        let current = |scope: &GrantScope| -> BTreeSet<String> {
            detail
                .as_ref()
                .and_then(|d| d.privileges_at(scope))
                .map(|set| {
                    let mut all = set.privileges.clone();
                    if set.grantable {
                        all.insert(GRANT_OPTION.into());
                    }
                    all
                })
                .unwrap_or_default()
        };
        if let Ok(scope) = &scope {
            if editor.loaded_for.as_ref() != Some(scope) {
                editor.wanted = current(scope);
                editor.loaded_for = Some(scope.clone());
            }
        }
        let mut open = true;
        let mut review = None;
        egui::Window::new(format!("Oprávnění • {}", safe_text(&editor.account.label())))
            .id(egui::Id::new("privileges_editor"))
            .collapsible(false)
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                match &editor.fixed {
                    Some(fixed) => {
                        ui.label(format!("Rozsah: {}", safe_text(&fixed.label())));
                    }
                    None => {
                        field(
                            ui,
                            "Rozsah: *.* (server), databáze.* nebo databáze.tabulka",
                            &mut editor.scope_text,
                            false,
                        );
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Šablona:");
                    for template in Template::ALL {
                        if ui.button(template.label()).clicked() {
                            let target = if template.needs_database() {
                                match &scope {
                                    Ok(GrantScope::Database(db)) => {
                                        Some(GrantScope::Database(db.clone()))
                                    }
                                    _ => None,
                                }
                            } else {
                                Some(GrantScope::Global)
                            };
                            match target {
                                Some(target) => {
                                    editor.fixed = None;
                                    editor.scope_text = match &target {
                                        GrantScope::Database(db) => format!("{db}.*"),
                                        _ => "*.*".into(),
                                    };
                                    let mut wanted = current(&target);
                                    wanted.extend(template.privileges().iter().map(|p| p.to_string()));
                                    editor.wanted = wanted;
                                    editor.loaded_for = Some(target);
                                }
                                None => {
                                    self.message = "Šablona pro databázi potřebuje rozsah databáze.* (zadejte ho nahoře).".into()
                                }
                            }
                        }
                    }
                });
                match &scope {
                    Err(error) => {
                        ui.label(RichText::new(error.to_string()).color(Color32::RED));
                    }
                    Ok(scope) => {
                        let options: Vec<&str> = scope
                            .privileges()
                            .iter()
                            .copied()
                            .chain([GRANT_OPTION])
                            .collect();
                        egui::Grid::new("privilege_checkboxes")
                            .num_columns(3)
                            .show(ui, |ui| {
                                for (i, privilege) in options.iter().enumerate() {
                                    let mut on = editor.wanted.contains(*privilege);
                                    if ui.checkbox(&mut on, *privilege).changed() {
                                        if on {
                                            editor.wanted.insert(privilege.to_string());
                                        } else {
                                            editor.wanted.remove(*privilege);
                                        }
                                    }
                                    if i % 3 == 2 {
                                        ui.end_row();
                                    }
                                }
                            });
                        ui.horizontal(|ui| {
                            if ui.button("Vše").clicked() {
                                editor.wanted =
                                    options.iter().map(|p| p.to_string()).collect();
                            }
                            if ui.button("Nic").clicked() {
                                editor.wanted.clear();
                            }
                            if ui.button("Zkontrolovat změnu…").clicked() {
                                let (grant, revoke) =
                                    privilege_diff(&current(scope), &editor.wanted);
                                review = Some(Change::Privileges {
                                    account: editor.account.clone(),
                                    scope: scope.clone(),
                                    grant,
                                    revoke,
                                });
                            }
                        });
                    }
                }
            });
        if let Some(change) = review {
            match preview(&change) {
                Ok(_) => self.confirm(change),
                Err(error) => self.message = error.to_string(),
            }
        }
        if open {
            self.privilege_editor = Some(editor);
        }
    }
    fn properties_window(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.properties_editor.take() else {
            return;
        };
        let mut open = true;
        let mut review = None;
        egui::Window::new(format!(
            "Vlastnosti účtu • {}",
            safe_text(&editor.account.label())
        ))
        .id(egui::Id::new("properties_editor"))
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.checkbox(&mut editor.locked, "Účet zamčen (nelze se přihlásit)");
            ui.checkbox(
                &mut editor.expire,
                "Nechat heslo vypršet (uživatel ho musí změnit při přihlášení)",
            );
            ui.label("Vyžadovat TLS:");
            if editor.tls == TlsRequirement::Specified {
                ui.label("Účet má konkrétní požadavek (vydavatel/subjekt); Harbor ho nemění.");
            } else {
                ui.horizontal_wrapped(|ui| {
                    for tls in [
                        TlsRequirement::None,
                        TlsRequirement::Ssl,
                        TlsRequirement::X509,
                    ] {
                        ui.radio_value(&mut editor.tls, tls, tls.label());
                    }
                });
            }
            ui.label("Limity (0 = bez limitu):");
            egui::Grid::new("limits").show(ui, |ui| {
                for (label, value) in [
                    ("Dotazy za hodinu", &mut editor.limits.queries_per_hour),
                    ("Změny za hodinu", &mut editor.limits.updates_per_hour),
                    (
                        "Připojení za hodinu",
                        &mut editor.limits.connections_per_hour,
                    ),
                    ("Souběžná spojení", &mut editor.limits.user_connections),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).range(0..=4_294_967_295u64));
                    ui.end_row();
                }
            });
            if ui.button("Zkontrolovat změnu…").clicked() {
                review = Some(Change::AlterAccount {
                    account: editor.account.clone(),
                    locked: editor.locked,
                    expire_password: editor.expire,
                    tls: (editor.tls != TlsRequirement::Specified).then_some(editor.tls),
                    limits: editor.limits,
                });
            }
        });
        if let Some(change) = review {
            self.confirm(change);
        }
        if open {
            self.properties_editor = Some(editor);
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(draft) = self.draft {
            self.draft_window(ctx, draft);
        }
        self.privilege_window(ctx);
        self.properties_window(ctx);
        if let Some(id) = self.delete_profile.clone() {
            let name = self
                .settings
                .profile(&id)
                .map(|p| safe_text(&p.name))
                .unwrap_or_else(|| "(už neexistuje)".into());
            egui::Modal::new(egui::Id::new("confirm_profile_delete")).show(ctx, |ui| {
                ui.heading("Odstranit uložený profil?");
                ui.label(format!(
                    "Profil „{name}“ bude odstraněn z konfigurace. Server se nemění."
                ));
                ui.horizontal(|ui| {
                    let cancel = ui.button("Zrušit");
                    if self.confirmation_focus {
                        cancel.request_focus();
                        self.confirmation_focus = false;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || cancel.clicked() {
                        self.delete_profile = None;
                    }
                    if ui.button("Odstranit profil").clicked() {
                        self.confirm_delete_profile();
                    }
                });
            });
        }
        if let Some(change) = self.confirmation.clone() {
            let sql = preview(&change);
            egui::Modal::new(egui::Id::new("confirm_change")).show(ctx, |ui| {
                ui.set_max_width(720.0);
                ui.heading("Potvrdit změnu na serveru");
                ui.strong(
                    self.session
                        .profile
                        .as_ref()
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "Odpojeno".into()),
                );
                ui.label(safe_text(&change.summary()));
                if self.session.snapshot.as_ref().is_some_and(|s| s.demo) {
                    ui.label("DEMO • změna pouze v paměti");
                }
                if let Change::KillQuery { target } = &change {
                    ui.monospace(safe_text(&target.query));
                }
                let valid = match &sql {
                    Ok(sql) => {
                        ui.label("Provede se:");
                        ui.monospace(safe_text(sql));
                        true
                    }
                    Err(error) => {
                        ui.label(RichText::new(error.to_string()).color(Color32::RED));
                        false
                    }
                };
                if matches!(change, Change::Maintenance { action: Maintenance::Optimize | Maintenance::Repair, .. }) {
                    ui.label("Během údržby se data v Harboru neobnovují; velká tabulka může být po dobu operace zamčená.");
                }
                if let Some(name) = change.destructive_name() {
                    ui.label(
                        RichText::new(format!(
                            "Nevratná operace. Pro potvrzení napište název: {}",
                            safe_text(name)
                        ))
                        .strong(),
                    );
                    ui.text_edit_singleline(&mut self.confirm_text);
                }
                let unlocked = unlocked(&change, &self.confirm_text);
                ui.horizontal(|ui| {
                    let cancel = ui.button("Zrušit");
                    if self.confirmation_focus {
                        cancel.request_focus();
                        self.confirmation_focus = false;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || cancel.clicked() {
                        self.confirmation = None;
                    }
                    if ui
                        .add_enabled(
                            valid
                                && unlocked
                                && !self.session.worker.busy
                                && self.session.snapshot.is_some(),
                            egui::Button::new("Provést změnu"),
                        )
                        .clicked()
                        && self.session.request(Operation::Apply(change))
                    {
                        self.confirmation = None;
                        self.confirm_text.clear();
                        self.draft = None;
                        self.privilege_editor = None;
                        self.properties_editor = None;
                        self.change_password.clear();
                    }
                });
            });
        }
    }
}
impl eframe::App for Harbor {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.render(ctx);
    }
}
impl Harbor {
    fn render(&mut self, ctx: &egui::Context) {
        self.session.tick(self.settings.refresh_seconds);
        self.sync_server_scope();
        self.autoload();
        // Results arrive over a channel egui cannot see: poll quickly only while work is
        // in flight, otherwise wake for the next refresh.
        ctx.request_repaint_after(self.session.next_wakeup(self.settings.refresh_seconds));
        let p = Palette::new(self.theme);
        if self.styled != Some(self.theme) {
            self.style(ctx, p);
            self.styled = Some(self.theme);
        }
        egui::SidePanel::left("navigation")
            .exact_width(224.0)
            .frame(egui::Frame::new().fill(p.side).inner_margin(20.0))
            .show(ctx, |ui| {
                if self.theme == ThemeId::Original {
                    dark_sidebar(ui.visuals_mut(), p);
                }
                ui.add_space(8.0);
                ui.heading(RichText::new("h  harbor").size(28.0));
                ui.small("MARIADB WORKSPACE");
                ui.add_space(16.0);
                ui.label(
                    self.session
                        .profile
                        .as_ref()
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "Žádné spojení".into()),
                );
                ui.label(safe_text(&self.session.status));
                ui.add_space(16.0);
                for (index, label) in PAGES.iter().enumerate() {
                    if ui
                        .add_sized(
                            [180.0, 36.0],
                            egui::Button::new(*label).selected(self.page == index),
                        )
                        .clicked()
                    {
                        self.page = index;
                    }
                }
                ui.add_space(16.0);
                ui.label("Ubuntu / AlmaLinux");
                ui.small(format!("Harbor {}", env!("CARGO_PKG_VERSION")));
            });
        egui::TopBottomPanel::top("status")
            .frame(egui::Frame::new().fill(p.surface).inner_margin(20.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "Servery  /  {}",
                        self.session
                            .profile
                            .as_ref()
                            .map(|p| safe_text(&p.name))
                            .unwrap_or_else(|| "—".into())
                    ));
                    ui.strong(&self.session.status);
                    if self.session.worker.busy {
                        ui.spinner();
                        ui.label("Pracuji…");
                    }
                    if ui.button("Obnovit (F5)").clicked() {
                        self.session.request(Operation::Refresh);
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(30.0))
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width().max(620.0));
                        let font = if self.theme == ThemeId::Terra {
                            FontFamily::Name("serif".into())
                        } else {
                            FontFamily::Proportional
                        };
                        ui.label(
                            RichText::new(
                                if self.page == OVERVIEW && self.theme == ThemeId::Original {
                                    "Server pod kontrolou."
                                } else {
                                    PAGES[self.page]
                                },
                            )
                            .font(FontId::new(desktop(self.theme).typography.title_size, font)),
                        );
                        ui.label(
                            RichText::new(if self.session.stale {
                                "Data nejsou aktuální — ověřte spojení."
                            } else {
                                "Správa MariaDB • společné jádro pro desktop a terminál"
                            })
                            .color(p.muted),
                        );
                        ui.add_space(14.0);
                        if !self.message.is_empty() {
                            ui.label(format!("Upozornění: {}", self.message));
                        }
                        if let Some(error) = &self.session.error {
                            ui.strong(format!("CHYBA: {error}"));
                        }
                        match self.page {
                            SETTINGS => self.settings(ui, p),
                            DATABASES => self.databases(ui, p),
                            ACCOUNTS => self.accounts(ui, p),
                            PROCESSES => self.processes(ui, p),
                            VARIABLES => self.variables(ui, p),
                            HEALTH => self.health(ui, p),
                            CONNECTION => self.connection(ui, p),
                            _ => self.overview(ui, p),
                        }
                        if let Some(snapshot) = &self.session.snapshot {
                            for notice in &snapshot.notices {
                                ui.label(RichText::new(safe_text(notice)).small().color(p.muted));
                            }
                        }
                    });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.session.request(Operation::Refresh);
        }
        self.dialogs(ctx);
    }
}
/// Destructive changes need the object's name typed exactly.
fn unlocked(change: &Change, typed: &str) -> bool {
    change.destructive_name().is_none_or(|name| typed == name)
}
fn sort_tables(tables: &mut [&TableInfo], sort: TableSort) {
    match sort {
        TableSort::Name => tables.sort_by(|a, b| a.name.cmp(&b.name)),
        TableSort::Rows => tables.sort_by(|a, b| b.rows.cmp(&a.rows)),
        TableSort::Size => tables.sort_by_key(|t| std::cmp::Reverse(t.bytes())),
        TableSort::Free => tables.sort_by_key(|t| std::cmp::Reverse(t.free_bytes)),
    }
}
/// Value with a readable size for large byte counts.
fn variable_value(v: &Variable) -> String {
    match v.value.parse::<u64>() {
        Ok(n) if v.var_type.contains("INT") && n >= 1_048_576 => {
            format!("{} ({})", v.value, human_bytes(n))
        }
        _ => v.value.clone(),
    }
}
fn origin_label(v: &Variable) -> &'static str {
    match v.origin.as_str() {
        "COMPILE-TIME" => "výchozí",
        "CONFIG" => "konfigurace",
        "COMMAND-LINE" => "příkazová řádka",
        "SQL" => "změněno za běhu",
        "AUTO" => "automaticky",
        "ENVIRONMENT" => "prostředí",
        _ => "—",
    }
}
fn clip(value: &str, max: usize) -> String {
    let text = safe_text(value);
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .password(password)
            .desired_width(520.0),
    );
}
fn metric(ui: &mut egui::Ui, p: Palette, label: &str, value: &str, size: f32) {
    if p.terra {
        ui.vertical(|ui| {
            ui.set_min_height(116.0);
            ui.label(RichText::new(label).size(12.0).color(p.muted));
            ui.label(
                RichText::new(value).font(FontId::new(size, FontFamily::Name("serif".into()))),
            );
            ui.add_space(16.0);
            ui.separator();
        });
        return;
    }
    card(ui, p, |ui| {
        ui.set_min_height(86.0);
        ui.label(RichText::new(label).size(12.0).color(p.muted));
        ui.label(RichText::new(value).size(size));
    });
}
fn chart(ui: &mut egui::Ui, values: &[f64], p: Palette) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 180.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for i in 0..=3 {
        let y = egui::lerp(rect.y_range(), i as f32 / 3.0);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, p.border),
        );
    }
    if values.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Graf čeká na naměřené vzorky",
            FontId::proportional(16.0),
            p.muted,
        );
        return;
    }
    let max = values.iter().copied().fold(1.0, f64::max);
    let points = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.left() + rect.width() * i as f32 / (values.len() - 1) as f32,
                rect.bottom() - rect.height() * (*v / max) as f32,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, p.accent)));
}

#[cfg(test)]
mod tests;
