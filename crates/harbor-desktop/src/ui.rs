use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use harbor_core::*;
use harbor_themes::{desktop, rgb, ThemeId};
use zeroize::Zeroizing;

const PAGES: [&str; 6] = [
    "Přehled serveru",
    "Databáze",
    "Uživatelé a oprávnění",
    "Aktivní spojení",
    "Připojení",
    "Nastavení",
];
pub struct Harbor {
    session: Session,
    store: Option<ConfigStore>,
    settings: Settings,
    theme: ThemeId,
    page: usize,
    /// One search per list page (index = page), so typing on one page does not
    /// filter another.
    filters: [String; 4],
    system: bool,
    long_only: bool,
    selected_db: Option<String>,
    selected_account: Option<Account>,
    selected_process: Option<Process>,
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
    change_kind: Option<usize>,
    change_db_name: String,
    change_user: String,
    change_host: String,
    change_db: String,
    change_password: Zeroizing<String>,
    /// Slider value while it is being dragged; persisted only on release.
    refresh_draft: Option<u64>,
    /// Theme whose egui style is currently installed (style is set only on change).
    styled: Option<ThemeId>,
    collation: String,
    privilege: usize,
    grant: bool,
    confirmation: Option<Change>,
    confirmation_focus: bool,
    message: String,
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
            radius: d.shape.card_radius,
            terra: theme == ThemeId::Terra,
            clarity: theme == ThemeId::Clarity,
        }
    }
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
            page: if demo { 0 } else { 4 },
            filters: Default::default(),
            system: false,
            long_only: false,
            selected_db: None,
            selected_account: None,
            selected_process: None,
            profile: Profile::default(),
            delete_profile: None,
            scope_generation: 0,
            host: "localhost".into(),
            port: 3306,
            socket: "/run/mysqld/mysqld.sock".into(),
            unix: false,
            password: Zeroizing::new(String::new()),
            change_kind: None,
            change_db_name: String::new(),
            change_user: String::new(),
            change_host: "localhost".into(),
            change_db: String::new(),
            change_password: Zeroizing::new(String::new()),
            refresh_draft: None,
            styled: None,
            collation: "utf8mb4_unicode_ci".into(),
            privilege: 0,
            grant: true,
            confirmation: None,
            confirmation_focus: false,
            message,
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
    /// Selections and account-bound drafts belong to one server. After connect/demo/
    /// disconnect they would silently target an account on a different server.
    fn sync_server_scope(&mut self) {
        let generation = self.session.worker.generation();
        if generation != self.scope_generation {
            self.scope_generation = generation;
            self.selected_db = None;
            self.selected_account = None;
            self.selected_process = None;
            self.confirmation = None;
            self.change_password.clear();
            if matches!(self.change_kind, Some(2 | 3)) {
                self.change_kind = None;
            }
        }
        let Some(snapshot) = &self.session.snapshot else {
            return;
        };
        if self
            .selected_account
            .as_ref()
            .is_some_and(|a| !snapshot.accounts.contains(a))
        {
            self.selected_account = None;
            self.session.grants.clear();
            if matches!(self.change_kind, Some(2 | 3)) {
                self.change_kind = None;
            }
            if matches!(
                self.confirmation,
                Some(Change::ChangePassword { .. } | Change::DatabaseGrant { .. })
            ) {
                self.confirmation = None;
            }
        }
        if self
            .selected_db
            .as_ref()
            .is_some_and(|name| !snapshot.databases.iter().any(|d| &d.name == name))
        {
            self.selected_db = None;
            self.session.tables.clear();
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
        style.spacing.item_spacing = Vec2::new(12.0, 12.0);
        style.spacing.button_padding = Vec2::new(14.0, 10.0);
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
                    self.page = 0;
                }
                if ui.button("Otevřít DEMO").clicked() {
                    self.session
                        .connect(self.profile.clone(), Secret::default(), true);
                    self.page = 0;
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
            if ui.button("Zobrazit aktivní spojení →").clicked() {
                self.page = 3;
            }
        });
    }
    fn databases(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[1]);
            ui.checkbox(&mut self.system, "Systémové databáze");
            if ui.button("+ Vytvořit databázi").clicked() {
                self.change_kind = Some(0);
            }
        });
        // Arc clone: the frame keeps the rows without copying them.
        let snapshot = self.session.snapshot.clone();
        let databases = snapshot.as_deref().map_or(&[][..], |s| &s.databases);
        card(ui, p, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("databases")
                    .striped(true)
                    .min_row_height(42.0)
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
                                self.session.tables.clear();
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
        if let Some(name) = &self.selected_db {
            card(ui, p, |ui| {
                ui.heading(format!("Tabulky • {}", safe_text(name)));
                for t in &self.session.tables {
                    ui.label(format!(
                        "{} • {} • ≈ {} řádků • {}",
                        safe_text(&t.name),
                        safe_text(&t.engine),
                        t.rows,
                        bytes_label(t.bytes)
                    ));
                }
            });
        }
    }
    fn accounts(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[2]);
            if ui.button("+ Vytvořit účet").clicked() {
                self.change_kind = Some(1);
            }
        });
        let snapshot = self.session.snapshot.clone();
        let accounts = snapshot.as_deref().map_or(&[][..], |s| &s.accounts);
        card(ui, p, |ui| {
            egui::Grid::new("accounts")
                .striped(true)
                .min_row_height(42.0)
                .show(ui, |ui| {
                    ui.strong("ÚČET");
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
                            self.selected_account = Some(account.clone());
                            self.session.grants.clear();
                        }
                        ui.label(safe_text(&account.host));
                        ui.label(safe_text(&account.plugin));
                        ui.end_row();
                    }
                });
        });
        if let Some(account) = &self.selected_account {
            card(ui, p, |ui| {
                ui.heading(safe_text(&account.label()));
                for grant in &self.session.grants {
                    ui.monospace(safe_text(grant));
                }
                ui.horizontal(|ui| {
                    if ui.button("Změnit heslo…").clicked() {
                        self.change_kind = Some(2);
                    }
                    if ui.button("Databázové oprávnění…").clicked() {
                        self.change_kind = Some(3);
                    }
                });
            });
        }
    }
    fn processes(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filters[3]);
            ui.checkbox(
                &mut self.long_only,
                "Jen běžící dotazy nad 30 sekund (Sleep se nepočítá)",
            );
        });
        let snapshot = self.session.snapshot.clone();
        let processes = snapshot.as_deref().map_or(&[][..], |s| &s.processes);
        card(ui, p, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("processes")
                    .striped(true)
                    .min_row_height(42.0)
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
                    self.confirmation = Some(Change::KillQuery { target: process });
                    self.confirmation_focus = true;
                }
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
            && matches_filter(&self.filters[1], &[&d.name, &d.charset, &d.collation])
    }
    fn account_visible(&self, a: &Account) -> bool {
        matches_filter(&self.filters[2], &[&a.user, &a.host, &a.plugin])
    }
    fn process_visible(&self, p: &Process) -> bool {
        (!self.long_only || p.long_query())
            && matches_filter(
                &self.filters[3],
                &[&p.user, &p.host, &p.database, &p.command, &p.query],
            )
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
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(kind) = self.change_kind {
            egui::Window::new("Připravit změnu")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    if kind == 0 {
                        field(ui, "Název databáze", &mut self.change_db_name, false);
                        field(ui, "Kolace (ověří server)", &mut self.collation, false);
                    }
                    if kind == 1 {
                        field(ui, "Uživatel", &mut self.change_user, false);
                        field(ui, "Hostitel účtu", &mut self.change_host, false);
                    }
                    if kind == 1 || kind == 2 {
                        field(ui, "Nové heslo", &mut self.change_password, true);
                    }
                    if kind == 3 {
                        field(ui, "Databáze", &mut self.change_db, false);
                        for (index, privilege) in Privilege::ALL.iter().enumerate() {
                            ui.radio_value(&mut self.privilege, index, privilege.sql());
                        }
                        ui.checkbox(&mut self.grant, "Přidat (vypnuto = odebrat)");
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Ponechat rozpracované a zavřít").clicked() {
                            self.change_kind = None;
                        }
                        if ui.button("Zkontrolovat změnu…").clicked() {
                            self.confirmation = match kind {
                                0 => Some(Change::CreateDatabase {
                                    name: self.change_db_name.clone(),
                                    collation: self.collation.clone(),
                                }),
                                1 => Some(Change::CreateAccount {
                                    user: self.change_user.clone(),
                                    host: self.change_host.clone(),
                                    password: Secret(self.change_password.clone()),
                                }),
                                2 => self.selected_account.clone().map(|account| {
                                    Change::ChangePassword {
                                        account,
                                        password: Secret(self.change_password.clone()),
                                    }
                                }),
                                3 => self.selected_account.clone().map(|account| {
                                    Change::DatabaseGrant {
                                        account,
                                        database: self.change_db.clone(),
                                        privilege: Privilege::ALL[self.privilege],
                                        grant: self.grant,
                                    }
                                }),
                                _ => None,
                            };
                            self.confirmation_focus = self.confirmation.is_some();
                        }
                    });
                });
        }
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
            egui::Modal::new(egui::Id::new("confirm_change")).show(ctx, |ui| {
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
                            !self.session.worker.busy && self.session.snapshot.is_some(),
                            egui::Button::new("Provést změnu"),
                        )
                        .clicked()
                        && self.session.request(Operation::Apply(change))
                    {
                        self.confirmation = None;
                        self.change_kind = None;
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
                    ui.visuals_mut().override_text_color = Some(Color32::from_rgb(220, 238, 239));
                }
                ui.add_space(12.0);
                ui.heading(RichText::new("h  harbor").size(28.0));
                ui.small("MARIADB WORKSPACE");
                ui.add_space(30.0);
                ui.label(
                    self.session
                        .profile
                        .as_ref()
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "Žádné spojení".into()),
                );
                ui.label(safe_text(&self.session.status));
                ui.add_space(30.0);
                for (index, label) in PAGES.iter().enumerate() {
                    if ui
                        .add_sized(
                            [180.0, 44.0],
                            egui::Button::new(*label).selected(self.page == index),
                        )
                        .clicked()
                    {
                        self.page = index;
                    }
                }
                ui.add_space(30.0);
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
                            RichText::new(if self.page == 0 && self.theme == ThemeId::Original {
                                "Server pod kontrolou."
                            } else {
                                PAGES[self.page]
                            })
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
                        ui.add_space(18.0);
                        if !self.message.is_empty() {
                            ui.label(format!("Nastavení: {}", self.message));
                        }
                        if let Some(error) = &self.session.error {
                            ui.strong(format!("CHYBA: {error}"));
                        }
                        match self.page {
                            0 => self.overview(ui, p),
                            1 => self.databases(ui, p),
                            2 => self.accounts(ui, p),
                            3 => self.processes(ui, p),
                            4 => self.connection(ui, p),
                            _ => self.settings(ui, p),
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
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn themes_preserve_drafts_selection_and_render_at_both_sizes() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, false);
        app.store = None;
        app.change_db_name = "rozpracovaná_db".into();
        app.selected_db = Some("commerce".into());
        for theme in ThemeId::DESKTOP {
            app.theme = theme;
            for size in [[1024.0, 720.0], [1440.0, 960.0]] {
                for page in 0..6 {
                    app.page = page;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::from(size),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.render(ctx),
                    );
                    assert!(!output.shapes.is_empty());
                }
            }
        }
        assert_eq!(app.change_db_name, "rozpracovaná_db");
        assert_eq!(app.selected_db.as_deref(), Some("commerce"));
    }
    fn frame(ctx: &egui::Context, app: &mut Harbor) -> egui::FullOutput {
        ctx.run(egui::RawInput::default(), |ctx| app.render(ctx))
    }
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => (),
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|s| walk(&s.shape, &mut out));
        out
    }
    #[test]
    fn server_text_with_control_or_bidi_characters_is_neutralized() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, true);
        app.store = None;
        wait_for_snapshot(&ctx, &mut app);
        let mut snapshot = (**app.session.snapshot.as_ref().unwrap()).clone();
        snapshot.databases[0].name = "faktury\u{202e}gpj.exe\nDROP".into();
        snapshot.accounts[0].host = "evil\u{2066}host".into();
        app.session.snapshot = Some(std::sync::Arc::new(snapshot));
        app.selected_account = None;
        for page in [1, 2] {
            app.page = page;
            let all = texts(&frame(&ctx, &mut app).shapes).join("");
            assert!(
                !all.contains('\u{202e}') && !all.contains('\u{2066}'),
                "page {page}"
            );
            assert!(all.contains("faktury") || page == 2);
        }
    }
    #[test]
    fn dragging_the_refresh_slider_writes_the_config_once() {
        let directory =
            std::env::temp_dir().join(format!("harbor-desktop-slider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, false);
        app.store = Some(ConfigStore::at(directory.clone()));
        app.settings = Settings::default();
        for value in 6..40 {
            app.refresh_slider(value, true, false, true);
        }
        assert!(
            !directory.join("settings.json").exists(),
            "no write while dragging"
        );
        assert_eq!(app.refresh_draft, Some(39));
        app.refresh_slider(39, false, true, false);
        assert_eq!(app.settings.refresh_seconds, 39);
        assert_eq!(app.refresh_draft, None);
        assert_eq!(
            ConfigStore::at(directory.clone())
                .load()
                .unwrap()
                .refresh_seconds,
            39
        );
        // Keyboard change (no drag) is saved right away.
        app.refresh_slider(40, false, false, true);
        assert_eq!(app.settings.refresh_seconds, 40);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn filters_are_per_page_case_insensitive_and_forms_do_not_share_drafts() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, true);
        app.store = None;
        wait_for_snapshot(&ctx, &mut app);
        let snapshot = app.session.snapshot.clone().unwrap();
        app.filters[2] = "ANALYTICS".into();
        assert_eq!(
            snapshot
                .accounts
                .iter()
                .filter(|a| app.account_visible(a))
                .count(),
            1
        );
        assert!(snapshot.databases.iter().any(|d| app.database_visible(d)));
        app.filters[3] = "select date".into();
        assert_eq!(
            snapshot
                .processes
                .iter()
                .filter(|p| app.process_visible(p))
                .count(),
            1
        );
        app.change_db_name = "nova_db".into();
        assert!(app.change_user.is_empty());
    }
    #[test]
    fn idle_window_does_not_repaint_ten_times_a_second() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, false);
        app.store = None;
        // The first frames repaint at once (fonts, style install); then it settles.
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let output = frame(&ctx, &mut app);
        let delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(delay >= Duration::from_millis(900), "{delay:?}");
    }
    fn wait_for_snapshot(ctx: &egui::Context, app: &mut Harbor) {
        let start = std::time::Instant::now();
        frame(ctx, app);
        while app.session.worker.busy && start.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
            frame(ctx, app);
        }
        assert!(app.session.snapshot.is_some());
    }
    #[test]
    fn switching_server_drops_selection_and_account_bound_drafts() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, true);
        app.store = None;
        wait_for_snapshot(&ctx, &mut app);
        let account = app.session.snapshot.as_ref().unwrap().accounts[0].clone();
        app.selected_account = Some(account.clone());
        app.selected_db = Some("commerce".into());
        app.change_kind = Some(2);
        app.change_password = Zeroizing::new("tajne".into());
        app.confirmation = Some(Change::ChangePassword {
            account,
            password: Secret::new("tajne".into()),
        });
        frame(&ctx, &mut app);
        assert!(
            app.selected_account.is_some(),
            "same server keeps the selection"
        );
        // Another server (here a fresh demo session) must not inherit the target account.
        app.session
            .connect(Profile::default(), Secret::default(), true);
        frame(&ctx, &mut app);
        assert!(app.selected_account.is_none());
        assert!(app.selected_db.is_none());
        assert!(app.confirmation.is_none());
        assert!(app.change_kind.is_none());
        assert!(app.change_password.is_empty());
    }
    #[test]
    fn selection_missing_from_new_snapshot_is_dropped() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, true);
        app.store = None;
        wait_for_snapshot(&ctx, &mut app);
        app.selected_account = Some(Account {
            user: "neexistuje".into(),
            host: "%".into(),
            plugin: "mysql_native_password".into(),
        });
        app.change_kind = Some(3);
        frame(&ctx, &mut app);
        assert!(app.selected_account.is_none());
        assert!(app.change_kind.is_none());
    }
    #[test]
    fn profiles_are_saved_and_deleted_by_id_with_confirmation() {
        let directory =
            std::env::temp_dir().join(format!("harbor-desktop-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, false);
        app.store = Some(ConfigStore::at(directory.clone()));
        app.settings = Settings::default();
        for name in ["A", "B"] {
            app.load_profile(Profile::default());
            app.profile.name = name.into();
            app.save_profile();
            assert!(!app.profile.id.is_empty());
        }
        // Saving the same form twice updates it instead of duplicating it.
        app.save_profile();
        assert_eq!(app.settings.profiles.len(), 2);
        let a = app.settings.profiles[0].clone();
        app.load_profile(a.clone());
        // Renaming A onto B is refused; B is not silently replaced.
        app.profile.name = "B".into();
        app.save_profile();
        assert!(app.message.contains("už existuje"));
        assert_eq!(app.settings.profiles.len(), 2);
        // Delete targets the loaded profile id even though the form now says "B".
        app.request_delete_profile();
        assert_eq!(
            app.settings.profiles.len(),
            2,
            "nothing deleted before confirmation"
        );
        assert_eq!(app.delete_profile.as_deref(), Some(a.id.as_str()));
        app.confirm_delete_profile();
        assert_eq!(app.settings.profiles.len(), 1);
        assert_eq!(app.settings.profiles[0].name, "B");
        assert!(app.profile.id.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
