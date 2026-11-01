use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use harbor_core::*;
use harbor_themes::{desktop, rgb, ThemeId};
use std::time::Duration;

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
    filter: String,
    system: bool,
    long_only: bool,
    selected_db: Option<String>,
    selected_account: Option<Account>,
    selected_process: Option<Process>,
    profile: Profile,
    editing_profile: Option<String>,
    host: String,
    port: u16,
    socket: String,
    unix: bool,
    password: String,
    change_kind: Option<usize>,
    change_name: String,
    change_host: String,
    change_db: String,
    change_password: String,
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
            filter: String::new(),
            system: false,
            long_only: false,
            selected_db: None,
            selected_account: None,
            selected_process: None,
            profile: Profile::default(),
            editing_profile: None,
            host: "localhost".into(),
            port: 3306,
            socket: "/run/mysqld/mysqld.sock".into(),
            unix: false,
            password: String::new(),
            change_kind: None,
            change_name: String::new(),
            change_host: "localhost".into(),
            change_db: String::new(),
            change_password: String::new(),
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
        app
    }
    fn save(&mut self, change: impl FnOnce(&mut Settings)) {
        match self
            .store
            .as_ref()
            .ok_or_else(|| Error::Config("Úložiště není dostupné.".into()))
            .and_then(|s| s.update(change))
        {
            Ok(settings) => {
                self.settings = settings;
                self.message.clear();
            }
            Err(error) => self.message = error.to_string(),
        }
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
                if ui.button(&profile.name).clicked() {
                    self.load_profile(profile);
                }
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
                    match self.profile.validate() {
                        Ok(()) => {
                            let profile = self.profile.clone();
                            let old_name = self.editing_profile.clone();
                            self.save(move |s| {
                                s.profiles.retain(|p| {
                                    p.name != profile.name && Some(&p.name) != old_name.as_ref()
                                });
                                s.profiles.push(profile);
                            });
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
                if ui.button("Odstranit uložený profil").clicked() {
                    let name = self.profile.name.clone();
                    self.save(move |s| s.profiles.retain(|p| p.name != name));
                }
                if ui
                    .add_enabled(
                        !self.session.worker.busy,
                        egui::Button::new("Otestovat spojení"),
                    )
                    .clicked()
                {
                    self.session.worker.send(Operation::TestConnection {
                        profile: self.profile.clone(),
                        password: Secret::new(self.password.clone()),
                    });
                }
                if ui
                    .add_enabled(!self.session.worker.busy, egui::Button::new("Připojit"))
                    .clicked()
                {
                    self.session.connect(
                        self.profile.clone(),
                        Secret::new(std::mem::take(&mut self.password)),
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
        self.editing_profile = Some(profile.name.clone());
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
            ui.label("Questions • měřeno od připojení • nejvýše 120 vzorků");
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
            ui.label(format!("MariaDB: {}", s.version));
            ui.label("Operační systém: Nezjištěno");
            ui.label(format!("Časové pásmo serveru: {}", s.timezone));
            if let Some(profile) = &self.session.profile {
                ui.label(format!(
                    "Účet: {} • {}",
                    profile.user,
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
            ui.text_edit_singleline(&mut self.filter);
            ui.checkbox(&mut self.system, "Systémové databáze");
            if ui.button("+ Vytvořit databázi").clicked() {
                self.change_kind = Some(0);
            }
        });
        let databases = self
            .session
            .snapshot
            .as_ref()
            .map(|s| s.databases.clone())
            .unwrap_or_default();
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
                        for db in databases.iter().filter(|d| {
                            (self.system || !d.system())
                                && d.name.to_lowercase().contains(&self.filter.to_lowercase())
                        }) {
                            if ui
                                .selectable_label(
                                    self.selected_db.as_ref() == Some(&db.name),
                                    &db.name,
                                )
                                .clicked()
                                && self.session.worker.send(Operation::Tables(db.name.clone()))
                            {
                                self.selected_db = Some(db.name.clone());
                                self.session.tables.clear();
                            }
                            ui.label(db.tables.to_string());
                            ui.label(bytes_label(db.bytes));
                            ui.label(&db.charset);
                            ui.label(&db.collation);
                            ui.end_row();
                        }
                    });
            });
        });
        if let Some(name) = &self.selected_db {
            card(ui, p, |ui| {
                ui.heading(format!("Tabulky • {name}"));
                for t in &self.session.tables {
                    ui.label(format!(
                        "{} • {} • ≈ {} řádků • {}",
                        t.name,
                        t.engine,
                        t.rows,
                        bytes_label(t.bytes)
                    ));
                }
            });
        }
    }
    fn accounts(&mut self, ui: &mut egui::Ui, p: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.filter);
            if ui.button("+ Vytvořit účet").clicked() {
                self.change_kind = Some(1);
            }
        });
        let accounts = self
            .session
            .snapshot
            .as_ref()
            .map(|s| s.accounts.clone())
            .unwrap_or_default();
        card(ui, p, |ui| {
            egui::Grid::new("accounts")
                .striped(true)
                .min_row_height(42.0)
                .show(ui, |ui| {
                    ui.strong("ÚČET");
                    ui.strong("HOSTITEL");
                    ui.strong("AUTENTIZACE");
                    ui.end_row();
                    for account in accounts.iter().filter(|a| a.label().contains(&self.filter)) {
                        if ui
                            .selectable_label(
                                self.selected_account.as_ref() == Some(account),
                                &account.user,
                            )
                            .clicked()
                            && self.session.worker.send(Operation::Grants(account.clone()))
                        {
                            self.selected_account = Some(account.clone());
                            self.session.grants.clear();
                        }
                        ui.label(&account.host);
                        ui.label(&account.plugin);
                        ui.end_row();
                    }
                });
        });
        if let Some(account) = &self.selected_account {
            card(ui, p, |ui| {
                ui.heading(account.label());
                for grant in &self.session.grants {
                    ui.monospace(grant);
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
        ui.checkbox(
            &mut self.long_only,
            "Jen běžící dotazy nad 30 sekund (Sleep se nepočítá)",
        );
        let processes = self
            .session
            .snapshot
            .as_ref()
            .map(|s| s.processes.clone())
            .unwrap_or_default();
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
                        for process in processes
                            .iter()
                            .filter(|p| !self.long_only || p.long_query())
                        {
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
                            ui.label(&process.command);
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
            let mut refresh = self.settings.refresh_seconds;
            if ui
                .add(egui::Slider::new(&mut refresh, 1..=300).text("Obnova (s)"))
                .changed()
            {
                self.save(move |s| s.refresh_seconds = refresh);
            }
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(kind) = self.change_kind {
            egui::Window::new("Připravit změnu")
                .collapsible(false)
                .resizable(true)
                .show(ctx, |ui| {
                    if kind == 0 {
                        field(ui, "Název databáze", &mut self.change_name, false);
                        field(ui, "Kolace (ověří server)", &mut self.collation, false);
                    }
                    if kind == 1 {
                        field(ui, "Uživatel", &mut self.change_name, false);
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
                                    name: self.change_name.clone(),
                                    collation: self.collation.clone(),
                                }),
                                1 => Some(Change::CreateAccount {
                                    user: self.change_name.clone(),
                                    host: self.change_host.clone(),
                                    password: Secret::new(self.change_password.clone()),
                                }),
                                2 => self.selected_account.clone().map(|account| {
                                    Change::ChangePassword {
                                        account,
                                        password: Secret::new(self.change_password.clone()),
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
        if let Some(change) = self.confirmation.clone() {
            egui::Modal::new(egui::Id::new("confirm_change")).show(ctx, |ui| {
                ui.heading("Potvrdit změnu na serveru");
                ui.strong(
                    self.session
                        .profile
                        .as_ref()
                        .map(|p| p.name.as_str())
                        .unwrap_or("Odpojeno"),
                );
                ui.label(change.summary());
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
                        && self.session.worker.send(Operation::Apply(change))
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
        ctx.request_repaint_after(Duration::from_millis(100));
        let p = Palette::new(self.theme);
        self.style(ctx, p);
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
                        .map(|p| p.name.as_str())
                        .unwrap_or("Žádné spojení"),
                );
                ui.label(&self.session.status);
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
                            .map(|p| p.name.as_str())
                            .unwrap_or("—")
                    ));
                    ui.strong(&self.session.status);
                    if self.session.worker.busy {
                        ui.spinner();
                        ui.label("Pracuji…");
                    }
                    if ui.button("Obnovit (F5)").clicked() {
                        self.session.worker.send(Operation::Refresh);
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
                                ui.label(RichText::new(notice).small().color(p.muted));
                            }
                        }
                    });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.session.worker.send(Operation::Refresh);
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
    #[test]
    fn themes_preserve_drafts_selection_and_render_at_both_sizes() {
        let ctx = egui::Context::default();
        let mut app = Harbor::new(&ctx, false);
        app.store = None;
        app.change_name = "rozpracovaná_db".into();
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
        assert_eq!(app.change_name, "rozpracovaná_db");
        assert_eq!(app.selected_db.as_deref(), Some("commerce"));
    }
}
