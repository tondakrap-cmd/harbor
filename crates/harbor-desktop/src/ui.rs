use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use harbor_core::*;
use harbor_themes::{desktop, rgb, ThemeId};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

const PAGES: [&str; 10] = [
    "Přehled serveru",
    "Databáze",
    "Uživatelé a oprávnění",
    "Aktivní spojení",
    "Proměnné",
    "Doktor",
    "Služba a logy",
    "Zálohy",
    "Připojení",
    "Nastavení",
];
const OVERVIEW: usize = 0;
const DATABASES: usize = 1;
const ACCOUNTS: usize = 2;
const PROCESSES: usize = 3;
const VARIABLES: usize = 4;
const HEALTH: usize = 5;
const SERVICE: usize = 6;
const BACKUPS: usize = 7;
const CONNECTION: usize = 8;
const SETTINGS: usize = 9;

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
/// Confirmations of file and job actions (not server changes).
#[derive(Clone)]
enum Ask {
    Restore(RestoreRequest),
    DeleteBackup(std::path::PathBuf),
    Unschedule(String),
    Service(ServiceAction),
    /// The complete new content of Harbor's option file.
    WriteConfig(Vec<(String, String)>),
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
    filters: [String; 10],
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
    backups: Vec<BackupFile>,
    backups_loaded: bool,
    jobs_finished_seen: usize,
    selected_backup: Option<std::path::PathBuf>,
    /// Databases of the next backup (and of a schedule).
    backup_selection: BTreeSet<String>,
    backup_dir_text: String,
    encrypt: bool,
    encryption: Zeroizing<String>,
    encryption_again: Zeroizing<String>,
    /// Password of the selected encrypted archive (verify / restore).
    archive_password: Zeroizing<String>,
    restore_open: bool,
    restore_db: String,
    restore_target: String,
    restore_replace: bool,
    ask: Option<Ask>,
    ask_text: String,
    schedules: Vec<schedule::InstalledSchedule>,
    schedule_calendar: String,
    schedule_keep: u32,
    schedule_password_file: String,
    schedule_encryption_file: String,
    schedule_preview: Option<(String, String)>,
    selected_finding: Option<String>,
    backup_form_open: bool,
    schedule_open: bool,
    log_from_file: bool,
    log_lines: u32,
    /// 0 = all, 1 = errors, 2 = warnings and errors.
    log_level: u8,
    /// Variable whose value is being saved into Harbor's option file.
    config_variable: Option<String>,
    config_value: String,
    /// Last SSH settings typed in the profile form (kept when switching access type).
    ssh_target: String,
    ssh_port: u16,
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
    /// Background of selected rows and hovered controls: the theme's tint when text
    /// stays readable on it, otherwise a light accent wash.
    fn highlight(&self) -> Color32 {
        if contrast(self.ink, self.tint) >= 4.5 {
            self.tint
        } else {
            let mix = |a: u8, b: u8| ((a as f32) * 0.84 + (b as f32) * 0.16) as u8;
            Color32::from_rgb(
                mix(self.surface.r(), self.accent.r()),
                mix(self.surface.g(), self.accent.g()),
                mix(self.surface.b(), self.accent.b()),
            )
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
/// WCAG contrast ratio of two colours.
fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
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
            backups: Vec::new(),
            backups_loaded: false,
            jobs_finished_seen: 0,
            selected_backup: None,
            backup_selection: BTreeSet::new(),
            backup_dir_text: String::new(),
            encrypt: false,
            encryption: Zeroizing::new(String::new()),
            encryption_again: Zeroizing::new(String::new()),
            archive_password: Zeroizing::new(String::new()),
            restore_open: false,
            restore_db: String::new(),
            restore_target: String::new(),
            restore_replace: false,
            ask: None,
            ask_text: String::new(),
            schedules: Vec::new(),
            schedule_calendar: "*-*-* 02:30".into(),
            schedule_keep: 7,
            schedule_password_file: String::new(),
            schedule_encryption_file: String::new(),
            schedule_preview: None,
            selected_finding: None,
            backup_form_open: false,
            schedule_open: false,
            log_from_file: false,
            log_lines: 300,
            log_level: 0,
            config_variable: None,
            config_value: String::new(),
            ssh_target: String::new(),
            ssh_port: 22,
        };
        app.backup_dir_text = app.settings.backup_dir().display().to_string();
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
            // Another server has other databases; a restore targets the new one only
            // after an explicit new choice.
            self.backup_selection.clear();
            self.restore_open = false;
            self.config_variable = None;
            // Restores, service actions and configuration writes target one server.
            if matches!(
                self.ask,
                Some(Ask::Restore(_) | Ask::Service(_) | Ask::WriteConfig(_))
            ) {
                self.ask = None;
            }
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
        // Selected rows and labels keep the normal text colour, so the highlight must be
        // light on light themes (Original's tint is dark and made selections unreadable).
        v.selection.bg_fill = p.highlight();
        v.selection.stroke = Stroke::new(1.0, p.ink);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            widget.corner_radius = p.radius.min(8).into();
            widget.bg_stroke = Stroke::new(1.0, p.border);
        }
        v.widgets.hovered.bg_fill = p.highlight();
        v.widgets.hovered.weak_bg_fill = p.highlight();
        v.widgets.active.bg_fill = p.highlight();
        v.widgets.active.weak_bg_fill = p.highlight();
        v.widgets.hovered.fg_stroke.color = p.ink;
        v.widgets.active.fg_stroke.color = p.ink;
        ctx.set_visuals(v);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.icon_width = 17.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.icon_spacing = 6.0;
        // One text size for labels, buttons and table rows; headings clearly larger.
        for (text_style, size, family) in [
            (egui::TextStyle::Body, 15.0, FontFamily::Proportional),
            (egui::TextStyle::Button, 15.0, FontFamily::Proportional),
            (egui::TextStyle::Heading, 21.0, FontFamily::Proportional),
            (egui::TextStyle::Small, 12.5, FontFamily::Proportional),
            (egui::TextStyle::Monospace, 13.5, FontFamily::Monospace),
        ] {
            style
                .text_styles
                .insert(text_style, FontId::new(size, family));
        }
        ctx.set_style(style);
    }
    /// Two panes below the toolbar: a list on the left and the detail of the selected
    /// item on the right, each scrolling on its own. Narrow windows stack them.
    fn split(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        list: impl FnOnce(&mut Self, &mut egui::Ui),
        detail: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        let available = ui.available_size();
        if available.x >= 980.0 {
            egui::SidePanel::left(egui::Id::new(id))
                .resizable(true)
                .default_width((available.x * 0.46).max(360.0))
                .width_range(320.0..=(available.x - 360.0).max(320.0))
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    right: 8,
                    ..Default::default()
                }))
                .show_inside(ui, |ui| list(self, ui));
        } else {
            egui::TopBottomPanel::top(egui::Id::new(id))
                .resizable(true)
                .default_height(available.y * 0.45)
                .height_range(160.0..=(available.y - 160.0).max(160.0))
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    bottom: 8,
                    ..Default::default()
                }))
                .show_inside(ui, |ui| list(self, ui));
        }
        // The separator line sits on the list panel's edge: equal gaps on both sides.
        let gap = if available.x >= 980.0 {
            egui::Margin {
                left: 8,
                ..Default::default()
            }
        } else {
            egui::Margin {
                top: 8,
                ..Default::default()
            }
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(gap))
            .show_inside(ui, |ui| detail(self, ui));
    }
    /// Detail card: a fixed header with the item's name and its actions, then the
    /// scrolling content. Actions are never hidden below a long detail.
    fn detail_pane(
        &mut self,
        ui: &mut egui::Ui,
        p: Palette,
        id: &str,
        header: impl FnOnce(&mut Self, &mut egui::Ui),
        body: impl FnOnce(&mut Self, &mut egui::Ui),
    ) {
        panel_frame(p).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal_wrapped(|ui| header(self, ui));
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| body(self, ui));
        });
    }
    fn connection(&mut self, ui: &mut egui::Ui, p: Palette) {
        toolbar(ui, |ui| {
            if ui.button("+ Nový profil").clicked() {
                self.load_profile(Profile::default());
            }
            if ui.button("Otevřít DEMO").clicked() {
                self.session
                    .connect(self.profile.clone(), Secret::default(), true);
                self.page = OVERVIEW;
            }
            if ui
                .add_enabled(self.session.profile.is_some(), egui::Button::new("Odpojit"))
                .clicked()
            {
                self.session.disconnect();
            }
            ui.label(
                RichText::new("Hesla se neukládají; zůstávají jen v paměti relace.").color(p.muted),
            );
        });
        let profiles = self.settings.profiles.clone();
        self.split(
            ui,
            "connection_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    let selected = profiles.iter().position(|x| x.id == app.profile.id);
                    if profiles.is_empty() {
                        ui.label("Zatím žádný uložený profil. Vyplňte údaje vpravo a uložte.");
                    }
                    if let Some(i) = data_table(
                        ui,
                        "profiles",
                        &["PROFIL", "UŽIVATEL", "ZABEZPEČENÍ"],
                        &[Some(170.0), Some(110.0), None],
                        profiles.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let x = &profiles[row];
                            match col {
                                0 => cell(ui, &x.name),
                                1 => cell(ui, &x.user),
                                _ => cell(ui, x.security_label()),
                            }
                        },
                    ) {
                        app.load_profile(profiles[i].clone());
                    }
                });
            },
            |app, ui| {
                app.detail_pane(
                    ui,
                    p,
                    "profile_form",
                    |app, ui| {
                        ui.heading(if app.profile.id.is_empty() {
                            "Nový profil".to_string()
                        } else {
                            safe_text(&app.profile.name)
                        });
                        // Connect cancels a running read; Session refuses it only during a write.
                        if ui.button("Připojit").clicked() {
                            app.session.connect(
                                app.profile.clone(),
                                Secret(std::mem::take(&mut app.password)),
                                false,
                            );
                            app.page = OVERVIEW;
                        }
                        // Queued if a refresh is running; never silently dropped.
                        if ui.button("Otestovat").clicked() {
                            app.session.request(Operation::TestConnection {
                                profile: app.profile.clone(),
                                password: Secret(app.password.clone()),
                            });
                        }
                        if ui.button("Uložit profil").clicked() {
                            app.save_profile();
                        }
                        if ui
                            .add_enabled(
                                app.settings.profile(&app.profile.id).is_some(),
                                egui::Button::new("Odstranit…"),
                            )
                            .clicked()
                        {
                            app.request_delete_profile();
                        }
                    },
                    |app, ui| app.profile_form(ui),
                );
            },
        );
    }
    fn profile_form(&mut self, ui: &mut egui::Ui) {
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
        ui.add_space(6.0);
        ui.label(self.profile.security_label());
        if let Some(result) = &self.session.test_result {
            ui.label(result);
        }
        ui.add_space(10.0);
        ui.heading("Přístup k serveru");
        ui.label(
            RichText::new(
                "Volitelné: pro spouštění a zastavení služby, logy a trvalou konfiguraci.",
            )
            .small(),
        );
        let host = &mut self.profile.host;
        let mut kind = match host.access {
            HostAccess::None => 0,
            HostAccess::Local => 1,
            HostAccess::Ssh { .. } => 2,
        };
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut kind, 0, "žádný");
            ui.radio_value(&mut kind, 1, "lokálně (stejný počítač)");
            ui.radio_value(&mut kind, 2, "SSH");
        });
        let (mut target, mut port) = match &host.access {
            HostAccess::Ssh { target, port } => (target.clone(), *port),
            _ => (self.ssh_target.clone(), self.ssh_port),
        };
        if kind == 2 {
            field(
                ui,
                "SSH cíl (uživatel@hostitel; přihlášení klíčem nebo agentem)",
                &mut target,
                false,
            );
            ui.horizontal(|ui| {
                ui.label("SSH port");
                ui.add(egui::DragValue::new(&mut port).range(1..=65535));
            });
            self.ssh_target = target.clone();
            self.ssh_port = port;
        }
        host.access = match kind {
            1 => HostAccess::Local,
            2 => HostAccess::Ssh { target, port },
            _ => HostAccess::None,
        };
        if kind != 0 {
            ui.label("Práva správce:");
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(
                    &mut host.elevation,
                    Elevation::Sudo,
                    Elevation::Sudo.label(),
                );
                if kind == 1 {
                    ui.radio_value(
                        &mut host.elevation,
                        Elevation::Pkexec,
                        Elevation::Pkexec.label(),
                    );
                }
                ui.radio_value(
                    &mut host.elevation,
                    Elevation::None,
                    Elevation::None.label(),
                );
            });
            field(ui, "Služba systemd", &mut host.service, false);
            field(
                ui,
                "Adresář konfigurace (prázdné = najít automaticky)",
                &mut host.config_dir,
                false,
            );
            egui::CollapsingHeader::new("Pravidlo sudo pro správce serveru")
                .id_salt("sudoers")
                .show(ui, |ui| {
                    ui.label("Do /etc/sudoers.d/harbor (visudo -f) na serveru vložte, s vaším uživatelem:");
                    ui.monospace(SUDOERS_EXAMPLE);
                });
        }
    }
    fn overview(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(s) = self.session.snapshot.clone() else {
            ui.label("Připojte server v Připojení, nebo otevřete označené demo.");
            if ui.button("Přejít na Připojení →").clicked() {
                self.page = CONNECTION;
            }
            return;
        };
        egui::ScrollArea::vertical()
            .id_salt("overview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let rate = self
                    .session
                    .qps
                    .map(|q| format!("{q:.1}"))
                    .unwrap_or_else(|| "měřím…".into());
                let uptime = format!("{} d {:02} h", s.uptime / 86400, s.uptime / 3600 % 24);
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
                            40.0,
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
                ui.add_space(8.0);
                card(ui, p, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.heading("Provoz serveru");
                    ui.label(
                        RichText::new("Dotazy klientů za sekundu • bez dotazů Harboru • od připojení • nejvýše 120 vzorků")
                            .color(p.muted),
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
                    ui.set_min_width(ui.available_width());
                    ui.heading("Informace o instanci");
                    egui::Grid::new("instance").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                        let mut row = |label: &str, value: String| {
                            ui.label(RichText::new(label).color(p.muted));
                            ui.label(value);
                            ui.end_row();
                        };
                        row("MariaDB", safe_text(&s.version));
                        row("Časové pásmo", safe_text(&s.timezone));
                        row("Operační systém", "Nezjištěno".into());
                        if let Some(profile) = &self.session.profile {
                            row(
                                "Účet",
                                format!(
                                    "{} • {}",
                                    safe_text(&profile.user),
                                    if s.demo { "DEMO bez spojení" } else { profile.security_label() }
                                ),
                            );
                        }
                        row("Databáze", format!("{} viditelných", s.databases.len()));
                        row("Dlouhé dotazy", format!("{} běží alespoň 30 s", s.long_queries()));
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Aktivní spojení →").clicked() {
                            self.page = PROCESSES;
                        }
                        if ui.button("Spustit Doktora →").clicked() {
                            self.page = HEALTH;
                        }
                    });
                });
                for notice in &s.notices {
                    ui.label(RichText::new(safe_text(notice)).small().color(p.muted));
                }
            });
    }
    fn databases(&mut self, ui: &mut egui::Ui, p: Palette) {
        toolbar(ui, |ui| {
            search(ui, &mut self.filters[DATABASES]);
            ui.checkbox(&mut self.system, "Systémové databáze");
            if ui.button("+ Vytvořit databázi").clicked() {
                self.draft = Some(Draft::CreateDatabase);
            }
        });
        let databases: Vec<Database> = self
            .session
            .snapshot
            .as_deref()
            .map(|s| {
                s.databases
                    .iter()
                    .filter(|d| self.database_visible(d))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.split(
            ui,
            "databases_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(RichText::new(format!("{} databází", databases.len())).color(p.muted));
                    let selected = databases
                        .iter()
                        .position(|d| app.selected_db.as_ref() == Some(&d.name));
                    if let Some(i) = data_table(
                        ui,
                        "databases",
                        &["NÁZEV", "TABULKY", "VELIKOST", "KOLACE"],
                        &[Some(170.0), Some(72.0), Some(96.0), None],
                        databases.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let d = &databases[row];
                            match col {
                                0 => cell(ui, &d.name),
                                1 => cell(ui, &d.tables.to_string()),
                                2 => cell(ui, &bytes_label(d.bytes)),
                                _ => cell(ui, &d.collation),
                            }
                        },
                    ) {
                        let name = databases[i].name.clone();
                        if app.session.request(Operation::Tables(name.clone())) {
                            app.selected_db = Some(name);
                            app.selected_table = None;
                        }
                    }
                });
            },
            |app, ui| app.database_detail(ui, p),
        );
    }
    fn database_detail(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(name) = self.selected_db.clone() else {
            empty_detail(ui, p, "Vyberte databázi v seznamu.");
            return;
        };
        let detail = self.session.database.clone().filter(|d| d.name == name);
        self.detail_pane(
            ui,
            p,
            "database_detail",
            |app, ui| {
                ui.heading(safe_text(&name));
                if ui
                    .add_enabled(
                        !Database::system_name(&name),
                        egui::Button::new("Smazat databázi…"),
                    )
                    .clicked()
                {
                    app.confirm(Change::DropDatabase { name: name.clone() });
                }
            },
            |app, ui| {
                let Some(detail) = detail else {
                    ui.label("Načítám podrobnosti…");
                    return;
                };
                ui.label(format!(
                    "Kolace {} • {} tabulek • {} pohledů • {} procedur • {} triggerů • {} událostí",
                    safe_text(&detail.collation),
                    detail.base_tables().count(),
                    detail.views,
                    detail.routines,
                    detail.triggers,
                    detail.events
                ));
                ui.label(format!(
                    "Data {} • indexy {} • nevyužito {} (odhady)",
                    human_bytes(detail.data_bytes()),
                    human_bytes(detail.index_bytes()),
                    human_bytes(detail.free_bytes())
                ));
                for warning in detail.warnings() {
                    ui.label(RichText::new(format!("⚠ {}", safe_text(&warning))).color(p.warn));
                }
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Tabulky");
                    ui.label(RichText::new("řadit:").color(p.muted));
                    for (sort, label) in [
                        (TableSort::Size, "velikost"),
                        (TableSort::Rows, "řádky"),
                        (TableSort::Free, "nevyužito"),
                        (TableSort::Name, "název"),
                    ] {
                        ui.radio_value(&mut app.table_sort, sort, label);
                    }
                });
                let mut tables: Vec<&TableInfo> = detail.tables.iter().collect();
                sort_tables(&mut tables, app.table_sort);
                let selected = tables
                    .iter()
                    .position(|t| app.selected_table.as_ref() == Some(&t.name));
                if let Some(i) = data_table(
                    ui,
                    "tables",
                    &[
                        "TABULKA",
                        "ENGINE",
                        "ŘÁDKY",
                        "VELIKOST",
                        "NEVYUŽITO",
                        "UPOZORNĚNÍ",
                    ],
                    &[
                        Some(170.0),
                        Some(62.0),
                        Some(70.0),
                        Some(76.0),
                        Some(76.0),
                        None,
                    ],
                    tables.len(),
                    selected,
                    Some(260.0),
                    |ui, row, col| {
                        let t = tables[row];
                        match (col, t.view()) {
                            (0, _) => cell(ui, &t.name),
                            (1, true) => cell(ui, "pohled"),
                            (_, true) => (),
                            (1, _) => cell(ui, &t.engine),
                            (2, _) => cell(ui, &format!("≈ {}", t.rows)),
                            (3, _) => cell(ui, &human_bytes(t.bytes())),
                            (4, _) => cell(ui, &human_bytes(t.free_bytes)),
                            _ => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(t.warnings().join(", ")).color(p.warn),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                    },
                ) {
                    let table = tables[i].name.clone();
                    if app.session.request(Operation::Table {
                        database: detail.name.clone(),
                        table: table.clone(),
                    }) {
                        app.selected_table = Some(table);
                    }
                }
                if let Some(table) = app.selected_table.clone() {
                    ui.add_space(10.0);
                    app.table_section(ui, p, &detail.name, &table);
                }
            },
        );
    }
    fn table_section(&mut self, ui: &mut egui::Ui, p: Palette, database: &str, table: &str) {
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
        // Section header with the table's actions first, structure below.
        ui.horizontal_wrapped(|ui| {
            ui.heading(format!("Tabulka {}", safe_text(table)));
            if !view {
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
            }
        });
        if !self.session.change_output.is_empty() {
            egui::Frame::new()
                .fill(p.highlight())
                .corner_radius(4)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.strong("Výsledek poslední operace");
                    for line in &self.session.change_output {
                        ui.monospace(safe_text(line));
                    }
                });
        }
        let Some(detail) = detail else {
            ui.label("Načítám strukturu…");
            return;
        };
        ui.strong("Sloupce");
        egui::Grid::new("columns")
            .striped(true)
            .min_row_height(24.0)
            .show(ui, |ui| {
                for h in ["SLOUPEC", "TYP", "NULL", "VÝCHOZÍ", "KLÍČ", "EXTRA"] {
                    ui.label(RichText::new(h).small().color(p.muted));
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
                    ui.end_row();
                }
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
    }
    fn accounts(&mut self, ui: &mut egui::Ui, p: Palette) {
        toolbar(ui, |ui| {
            search(ui, &mut self.filters[ACCOUNTS]);
            if ui.button("+ Vytvořit účet").clicked() {
                self.draft = Some(Draft::CreateAccount);
            }
            if ui.button("+ Vytvořit roli").clicked() {
                self.draft = Some(Draft::CreateRole);
            }
        });
        let accounts: Vec<Account> = self
            .session
            .snapshot
            .as_deref()
            .map(|s| {
                s.accounts
                    .iter()
                    .filter(|a| self.account_visible(a))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.split(
            ui,
            "accounts_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(
                        RichText::new(format!("{} účtů a rolí", accounts.len())).color(p.muted),
                    );
                    let selected = accounts
                        .iter()
                        .position(|a| app.selected_account.as_ref() == Some(a));
                    if let Some(i) = data_table(
                        ui,
                        "accounts",
                        &["ÚČET / ROLE", "HOSTITEL", "AUTENTIZACE"],
                        &[Some(160.0), Some(120.0), None],
                        accounts.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let a = &accounts[row];
                            match col {
                                0 => cell(ui, &a.user),
                                1 => cell(ui, if a.is_role() { "role" } else { &a.host }),
                                _ => cell(ui, &a.plugin),
                            }
                        },
                    ) {
                        let account = accounts[i].clone();
                        if app.session.request(Operation::Grants(account.clone())) {
                            if app.selected_account.as_ref() != Some(&account) {
                                app.drop_account_drafts();
                            }
                            app.selected_account = Some(account);
                        }
                    }
                });
            },
            |app, ui| app.account_detail(ui, p),
        );
    }
    fn account_detail(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(account) = self.selected_account.clone() else {
            empty_detail(ui, p, "Vyberte účet nebo roli v seznamu.");
            return;
        };
        let detail = self
            .session
            .account
            .clone()
            .filter(|d| d.account.as_ref() == Some(&account));
        let has_properties = detail.as_ref().is_some_and(|d| d.properties.is_some());
        let header_detail = detail.clone();
        self.detail_pane(
            ui,
            p,
            "account_detail",
            |app, ui| {
                ui.heading(safe_text(&account.label()));
                if ui.button("Oprávnění…").clicked() {
                    let fixed = header_detail
                        .as_ref()
                        .and_then(|d| d.privileges.first())
                        .map(|s| s.scope.clone());
                    app.privilege_editor = Some(PrivilegeEditor {
                        account: account.clone(),
                        scope_text: if fixed.is_none() { "*.*".into() } else { String::new() },
                        fixed,
                        loaded_for: None,
                        wanted: BTreeSet::new(),
                    });
                }
                if !account.is_role() {
                    if ui.button("Změnit heslo…").clicked() {
                        app.draft = Some(Draft::ChangePassword);
                    }
                    if ui
                        .add_enabled(has_properties, egui::Button::new("Vlastnosti a limity…"))
                        .clicked()
                    {
                        if let Some(props) = header_detail.as_ref().and_then(|d| d.properties.clone()) {
                            app.properties_editor = Some(PropertiesEditor {
                                account: account.clone(),
                                locked: props.locked,
                                expire: false,
                                tls: props.tls,
                                limits: props.limits,
                            });
                        }
                    }
                    if ui.button("Výchozí role…").clicked() {
                        app.role_name = header_detail
                            .as_ref()
                            .and_then(|d| d.default_role.clone())
                            .unwrap_or_default();
                        app.draft = Some(Draft::DefaultRole);
                    }
                }
                if ui.button("Role…").clicked() {
                    app.draft = Some(Draft::RoleMembership);
                }
                if ui.button("Smazat…").clicked() {
                    app.confirm(Change::DropAccount {
                        account: account.clone(),
                    });
                }
            },
            |app, ui| {
                let Some(detail) = detail else {
                    ui.label("Načítám oprávnění…");
                    return;
                };
                egui::Grid::new("account_props").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    let mut row = |label: &str, value: String| {
                        ui.label(RichText::new(label).color(p.muted));
                        ui.label(value);
                        ui.end_row();
                    };
                    if let Some(props) = &detail.properties {
                        row("Autentizace", safe_text(&props.plugin));
                        row(
                            "Stav",
                            format!(
                                "{} • heslo {}",
                                if props.locked { "ZAMČEN" } else { "odemčen" },
                                if props.password_expired { "VYPRŠELO" } else { "platné" }
                            ),
                        );
                        row("TLS", props.tls.label().into());
                        let l = props.limits;
                        row(
                            "Limity",
                            format!(
                                "dotazy/h {} • změny/h {} • připojení/h {} • souběžně {} (0 = bez limitu)",
                                l.queries_per_hour, l.updates_per_hour, l.connections_per_hour, l.user_connections
                            ),
                        );
                    }
                    row(
                        "Role",
                        if detail.roles.is_empty() {
                            "žádné".into()
                        } else {
                            safe_text(&detail.roles.join(", "))
                        },
                    );
                    if !account.is_role() {
                        row(
                            "Výchozí role",
                            detail.default_role.as_deref().map(safe_text).unwrap_or_else(|| "žádná".into()),
                        );
                    }
                });
                if detail.properties.is_none() && !account.is_role() {
                    ui.label(
                        RichText::new("Vlastnosti účtu nejsou dostupné (chybí právo čtení mysql.*).")
                            .color(p.muted),
                    );
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.heading("Oprávnění");
                    ui.label(RichText::new("klepnutím na řádek je upravíte").color(p.muted));
                });
                if detail.privileges.is_empty() {
                    ui.label("Žádná přímá oprávnění (jen přes role, pokud nějaké má).");
                }
                let sets = detail.privileges.clone();
                if let Some(i) = data_table(
                    ui,
                    "privileges",
                    &["ROZSAH", "GRANT", "OPRÁVNĚNÍ"],
                    &[Some(200.0), Some(56.0), None],
                    sets.len(),
                    None,
                    Some(320.0),
                    |ui, row, col| {
                        let set = &sets[row];
                        match col {
                            0 => cell(ui, &set.scope.label()),
                            1 => cell(ui, if set.grantable { "ano" } else { "" }),
                            _ => cell(
                                ui,
                                &if set.privileges.is_empty() {
                                    "USAGE".to_string()
                                } else {
                                    set.privileges.iter().cloned().collect::<Vec<_>>().join(", ")
                                },
                            ),
                        }
                    },
                ) {
                    app.privilege_editor = Some(PrivilegeEditor {
                        account: account.clone(),
                        fixed: Some(sets[i].scope.clone()),
                        scope_text: String::new(),
                        loaded_for: None,
                        wanted: BTreeSet::new(),
                    });
                }
                for line in &detail.columns {
                    ui.label(format!("Sloupec: {}", safe_text(line)));
                }
                egui::CollapsingHeader::new("SHOW GRANTS")
                    .id_salt("show_grants")
                    .show(ui, |ui| {
                        for grant in &detail.grants {
                            ui.monospace(safe_text(grant));
                        }
                    });
            },
        );
    }
    fn processes(&mut self, ui: &mut egui::Ui, p: Palette) {
        toolbar(ui, |ui| {
            search(ui, &mut self.filters[PROCESSES]);
            ui.checkbox(&mut self.long_only, "Jen dotazy běžící přes 30 s");
        });
        let processes: Vec<Process> = self
            .session
            .snapshot
            .as_deref()
            .map(|s| {
                s.processes
                    .iter()
                    .filter(|p| self.process_visible(p))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.split(
            ui,
            "processes_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(RichText::new(format!("{} spojení", processes.len())).color(p.muted));
                    let selected = processes
                        .iter()
                        .position(|x| app.selected_process.as_ref().map(|s| s.id) == Some(x.id));
                    if let Some(i) = data_table(
                        ui,
                        "processes",
                        &["ID", "UŽIVATEL", "DATABÁZE", "ČAS", "PŘÍKAZ", "STAV"],
                        &[
                            Some(64.0),
                            Some(120.0),
                            Some(110.0),
                            Some(60.0),
                            Some(80.0),
                            None,
                        ],
                        processes.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let x = &processes[row];
                            match col {
                                0 => cell(ui, &x.id.to_string()),
                                1 => cell(ui, &x.user),
                                2 => cell(ui, &x.database),
                                3 => cell(ui, &format!("{} s", x.seconds)),
                                4 => cell(ui, &x.command),
                                _ => cell(ui, &x.state),
                            }
                        },
                    ) {
                        app.selected_process = Some(processes[i].clone());
                    }
                });
            },
            |app, ui| {
                let Some(process) = app.selected_process.clone() else {
                    empty_detail(ui, p, "Vyberte spojení v seznamu.");
                    return;
                };
                app.detail_pane(
                    ui,
                    p,
                    "process_detail",
                    |app, ui| {
                        ui.heading(format!("Spojení #{}", process.id));
                        if ui
                            .add_enabled(
                                process.running_query(),
                                egui::Button::new("Ukončit dotaz…"),
                            )
                            .clicked()
                        {
                            app.confirm(Change::KillQuery {
                                target: process.clone(),
                            });
                        }
                    },
                    |_, ui| {
                        ui.label(format!(
                            "{}@{} • {} • {} • {} s • {}",
                            safe_text(&process.user),
                            safe_text(&process.host),
                            safe_text(&process.database),
                            safe_text(&process.command),
                            process.seconds,
                            safe_text(&process.state)
                        ));
                        ui.monospace(if process.query.is_empty() {
                            "(žádný běžící dotaz)".into()
                        } else {
                            safe_text(&process.query)
                        });
                    },
                );
            },
        );
    }
    fn variables(&mut self, ui: &mut egui::Ui, p: Palette) {
        toolbar(ui, |ui| {
            search(ui, &mut self.filters[VARIABLES]);
            ui.checkbox(&mut self.key_only, "Jen klíčové");
            ui.checkbox(&mut self.changed_only, "Jen odlišné od výchozích");
            if ui.button("Načíst znovu").clicked() {
                self.session.request(Operation::Variables);
            }
        });
        let Some(all) = self.session.variables.clone() else {
            ui.label(if self.session.snapshot.is_some() {
                "Načítám proměnné…"
            } else {
                "Nejprve se připojte k serveru."
            });
            return;
        };
        let visible: Vec<Variable> = all
            .iter()
            .filter(|v| self.variable_visible(v))
            .cloned()
            .collect();
        let total = all.len();
        self.split(
            ui,
            "variables_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.label(RichText::new(format!("{} z {} proměnných", visible.len(), total)).color(p.muted));
                    let selected = visible
                        .iter()
                        .position(|v| app.selected_variable.as_ref() == Some(&v.name));
                    if let Some(i) = data_table(
                        ui,
                        "variables",
                        &["PROMĚNNÁ", "HODNOTA", "PŮVOD"],
                        &[Some(230.0), None, Some(120.0)],
                        visible.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let v = &visible[row];
                            match col {
                                0 => cell(ui, &v.name),
                                1 => cell(ui, &variable_value(v)),
                                _ => {
                                    let text = RichText::new(origin_label(v));
                                    ui.label(if v.runtime_only() { text.color(p.warn) } else { text });
                                }
                            }
                        },
                    ) {
                        app.selected_variable = Some(visible[i].name.clone());
                    }
                });
            },
            |app, ui| {
                let Some(variable) = app
                    .selected_variable
                    .as_ref()
                    .and_then(|name| all.iter().find(|v| &v.name == name))
                    .cloned()
                else {
                    empty_detail(ui, p, "Vyberte proměnnou v seznamu.");
                    return;
                };
                app.detail_pane(
                    ui,
                    p,
                    "variable_detail",
                    |app, ui| {
                        ui.heading(safe_text(&variable.name));
                        if ui
                            .add_enabled(variable.settable(), egui::Button::new("Změnit za běhu…"))
                            .clicked()
                        {
                            app.variable_value = variable.value.clone();
                            app.draft = Some(Draft::SetVariable);
                        }
                        if ui
                            .add_enabled(
                                app.session.host.is_some(),
                                egui::Button::new("Uložit do konfigurace…"),
                            )
                            .on_disabled_hover_text("Nastavte v profilu přístup k serveru.")
                            .clicked()
                        {
                            app.config_value = app
                                .session
                                .host_config
                                .as_ref()
                                .and_then(|c| c.entries.iter().find(|(n, _)| *n == variable.name))
                                .map(|(_, v)| v.clone())
                                .unwrap_or_else(|| variable.value.clone());
                            app.config_variable = Some(variable.name.clone());
                        }
                    },
                    |app, ui| {
                        if let Some(description) = variable.key_description() {
                            ui.label(description);
                        }
                        if !variable.comment.is_empty() {
                            ui.label(RichText::new(safe_text(&variable.comment)).color(p.muted));
                        }
                        ui.add_space(6.0);
                        egui::Grid::new("variable").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                            let mut row = |label: &str, value: String| {
                                ui.label(RichText::new(label).color(p.muted));
                                ui.label(value);
                                ui.end_row();
                            };
                            row("Hodnota", safe_text(&variable_value(&variable)));
                            row("Výchozí", variable.default.as_deref().map(safe_text).unwrap_or_else(|| "—".into()));
                            row("Původ", origin_label(&variable).into());
                            if let Some(config) = &app.session.host_config {
                                row(
                                    "V konfiguraci Harboru",
                                    config
                                        .entries
                                        .iter()
                                        .find(|(n, _)| *n == variable.name)
                                        .map(|(_, v)| safe_text(v))
                                        .unwrap_or_else(|| "—".into()),
                                );
                            }
                            row("Typ", safe_text(&variable.var_type));
                            if variable.min.is_some() || variable.max.is_some() {
                                row(
                                    "Rozsah",
                                    format!(
                                        "{} až {}",
                                        variable.min.as_deref().unwrap_or("?"),
                                        variable.max.as_deref().unwrap_or("?")
                                    ),
                                );
                            }
                            if !variable.allowed.is_empty() {
                                row("Povolené", safe_text(&variable.allowed.join(", ")));
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(if variable.settable() {
                                "Změna za běhu (SET GLOBAL) platí okamžitě, ale jen do restartu. Trvale ji uloží Uložit do konfigurace."
                            } else {
                                "Tuto proměnnou nelze měnit za běhu; patří do konfiguračního souboru."
                            })
                            .color(p.muted),
                        );
                    },
                );
            },
        );
    }
    fn health(&mut self, ui: &mut egui::Ui, p: Palette) {
        let findings = self.session.health.clone();
        toolbar(ui, |ui| {
            if ui.button("Spustit kontrolu").clicked() {
                self.session.request(Operation::Health);
            }
            match &findings {
                Some((at, list)) => {
                    let count = |s: Severity| list.iter().filter(|f| f.severity == s).count();
                    ui.label(format!(
                        "Před {} s • kritické {} • varování {} • informace {}",
                        at.elapsed().as_secs(),
                        count(Severity::Critical),
                        count(Severity::Warning),
                        count(Severity::Info)
                    ));
                }
                None => {
                    ui.label(if self.session.snapshot.is_some() {
                        "Probíhá kontrola…"
                    } else {
                        "Nejprve se připojte k serveru."
                    });
                }
            }
            ui.label(
                RichText::new("Kontrola jen čte; rozsah závisí na oprávněních účtu.")
                    .color(p.muted),
            );
        });
        let Some((_, findings)) = findings else {
            return;
        };
        if self.selected_finding.is_none() {
            self.selected_finding = findings.first().map(|f| f.title.clone());
        }
        self.split(
            ui,
            "health_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    if findings.is_empty() {
                        ui.label("Bez nálezů: žádné kontrolované pravidlo nehlásí problém.");
                    }
                    let selected = findings
                        .iter()
                        .position(|f| app.selected_finding.as_ref() == Some(&f.title));
                    if let Some(i) = data_table(
                        ui,
                        "findings",
                        &["ZÁVAŽNOST", "NÁLEZ"],
                        &[Some(110.0), None],
                        findings.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let f = &findings[row];
                            if col == 0 {
                                ui.label(
                                    RichText::new(f.severity.label())
                                        .strong()
                                        .color(p.severity(f.severity)),
                                );
                            } else {
                                cell(ui, &f.title);
                            }
                        },
                    ) {
                        app.selected_finding = Some(findings[i].title.clone());
                    }
                });
            },
            |app, ui| {
                let Some(finding) = findings
                    .iter()
                    .find(|f| app.selected_finding.as_ref() == Some(&f.title))
                    .cloned()
                else {
                    empty_detail(ui, p, "Vyberte nález v seznamu.");
                    return;
                };
                app.detail_pane(
                    ui,
                    p,
                    "finding_detail",
                    |_, ui| {
                        ui.label(
                            RichText::new(finding.severity.label())
                                .strong()
                                .color(p.severity(finding.severity)),
                        );
                        ui.heading(safe_text(&finding.title));
                    },
                    |_, ui| {
                        ui.label(safe_text(&finding.detail));
                        ui.add_space(8.0);
                        ui.strong("Doporučení");
                        ui.label(safe_text(&finding.recommendation));
                    },
                );
            },
        );
    }
    /// Loads what the service page and the variables page need from the host, one
    /// request at a time (the host worker runs a single command).
    fn host_autoload(&mut self) {
        if self.session.host.is_none()
            || self.session.host_busy()
            || self.session.host_error.is_some()
        {
            return;
        }
        let datadir = self
            .session
            .snapshot
            .as_ref()
            .map(|s| s.datadir.clone())
            .unwrap_or_default();
        if self.page == SERVICE {
            if self.session.host_status.is_none() {
                self.session.host_request(HostRequest::Status { datadir });
            } else if self.session.host_config.is_none() {
                self.session.host_request(HostRequest::ReadConfig);
            } else if self.session.host_logs.is_none() {
                let source = self.log_source();
                self.session.host_request(HostRequest::Logs {
                    source,
                    lines: self.log_lines,
                });
            }
        } else if self.page == VARIABLES && self.session.host_config.is_none() {
            self.session.host_request(HostRequest::ReadConfig);
        }
    }
    fn log_source(&self) -> LogSource {
        let file = self
            .session
            .snapshot
            .as_ref()
            .map(|s| s.log_error.clone())
            .filter(|f| f.starts_with('/'));
        match file {
            Some(file) if self.log_from_file => LogSource::File(file),
            _ => LogSource::Journal,
        }
    }
    fn log_visible(&self, line: &str) -> bool {
        let level = match self.log_level {
            1 => line.contains("[ERROR]") || line.contains("ERROR"),
            2 => line.contains("[ERROR]") || line.contains("[Warning]") || line.contains("ERROR"),
            _ => true,
        };
        level && matches_filter(&self.filters[SERVICE], &[line])
    }
    fn service_page(&mut self, ui: &mut egui::Ui, p: Palette) {
        let host = self.session.profile.as_ref().map(|p| p.host.clone());
        let demo = self.session.snapshot.as_ref().is_some_and(|s| s.demo);
        if self.session.host.is_none() {
            panel_frame(p).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.heading("Přístup k serveru není nastavený");
                ui.label("Pro spouštění a zastavení služby, logy a trvalou konfiguraci nastavte v profilu přístup k serveru: lokálně (Harbor běží na stejném stroji) nebo přes SSH s klíčem.");
                ui.label(RichText::new("Administrativní příkazy potřebují práva správce: sudo bez hesla pro vybrané příkazy, nebo pkexec s dotazem na heslo (jen lokálně).").color(p.muted));
                if ui.button("Nastavit v Připojení →").clicked() {
                    if let Some(profile) = self.session.profile.clone() {
                        self.load_profile(profile);
                    }
                    self.page = CONNECTION;
                }
            });
            return;
        }
        let busy = self.session.host_busy();
        let running = self.session.host_status.as_ref().map(|s| s.running());
        toolbar(ui, |ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("Obnovit stav"))
                .clicked()
            {
                self.session.host_status = None;
                self.session.host_logs = None;
                self.session.host_config = None;
                self.session.host_error = None;
            }
            if ui
                .add_enabled(
                    !busy && running == Some(false),
                    egui::Button::new("▶ Spustit"),
                )
                .clicked()
            {
                self.ask = Some(Ask::Service(ServiceAction::Start));
            }
            if ui
                .add_enabled(
                    !busy && running == Some(true),
                    egui::Button::new("⟳ Restartovat…"),
                )
                .clicked()
            {
                self.ask_text.clear();
                self.ask = Some(Ask::Service(ServiceAction::Restart));
            }
            if ui
                .add_enabled(
                    !busy && running == Some(true),
                    egui::Button::new("■ Zastavit…"),
                )
                .clicked()
            {
                self.ask_text.clear();
                self.ask = Some(Ask::Service(ServiceAction::Stop));
            }
            if busy {
                ui.spinner();
                ui.label("Provádím příkaz na serveru…");
            }
            ui.label(
                RichText::new(if demo {
                    "DEMO • simulovaný hostitel".to_string()
                } else {
                    host.as_ref()
                        .map(|h| safe_text(&h.label()))
                        .unwrap_or_default()
                })
                .color(p.muted),
            );
        });
        if let Some(error) = self.session.host_error.clone() {
            if banner(ui, p, Severity::Critical, &error, Some("Zkusit znovu")) {
                self.session.host_error = None;
            }
        }
        if let Some(message) = self.session.host_message.clone() {
            if banner(ui, p, Severity::Info, &message, Some("Zavřít")) {
                self.session.host_message = None;
            }
        }
        self.split(
            ui,
            "service_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    egui::ScrollArea::vertical().id_salt("service_left").auto_shrink([false, false]).show(ui, |ui| {
                        ui.heading("Služba");
                        match &app.session.host_status {
                            None => {
                                ui.label("Zjišťuji stav…");
                            }
                            Some(status) => {
                                let color = if status.running() {
                                    p.accent
                                } else {
                                    p.severity(Severity::Critical)
                                };
                                ui.label(
                                    RichText::new(format!(
                                        "● {} ({})",
                                        if status.running() { "běží" } else { "neběží" },
                                        safe_text(&status.sub)
                                    ))
                                    .strong()
                                    .color(color),
                                );
                                egui::Grid::new("service_status").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                                    let mut row = |label: &str, value: String| {
                                        ui.label(RichText::new(label).color(p.muted));
                                        ui.label(value);
                                        ui.end_row();
                                    };
                                    row("Jednotka", safe_text(&status.unit));
                                    row("Popis", safe_text(&status.description));
                                    row("PID", if status.pid > 0 { status.pid.to_string() } else { "—".into() });
                                    row("Běží od", safe_text(&status.since));
                                    row("Po startu systému", if status.enabled == "enabled" { "spouští se".into() } else { safe_text(&status.enabled) });
                                });
                                if let Some(disk) = &status.disk {
                                    ui.add_space(8.0);
                                    ui.strong(format!("Disk s daty ({})", safe_text(&disk.mount)));
                                    let used = disk.used_percent();
                                    let fraction = used as f32 / 100.0;
                                    let bar = egui::ProgressBar::new(fraction)
                                        .desired_width(ui.available_width().min(420.0))
                                        .text(format!(
                                            "{used} % • volno {}",
                                            human_bytes(disk.available_kib * 1024)
                                        ));
                                    ui.add(if used >= 85 {
                                        bar.fill(p.severity(Severity::Critical))
                                    } else {
                                        bar
                                    });
                                    if used >= 85 {
                                        ui.label(RichText::new("Disk je téměř plný: zvažte smazání starých binlogů, záloh nebo zvětšení disku.").color(p.warn));
                                    }
                                }
                            }
                        }
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.heading("Konfigurace Harboru");
                            ui.label(RichText::new("klepnutím na řádek volbu odeberete").color(p.muted));
                        });
                        match app.session.host_config.clone() {
                            None => {
                                ui.label("Načítám…");
                            }
                            Some(config) => {
                                ui.label(RichText::new(safe_text(&config.path)).monospace().color(p.muted));
                                if config.entries.is_empty() {
                                    ui.label("Harbor zatím nic neuložil. Hodnotu uložíte na obrazovce Proměnné → Uložit do konfigurace.");
                                }
                                let entries = config.entries.clone();
                                if let Some(i) = data_table(ui, "harbor_config", &["VOLBA", "HODNOTA"], &[Some(220.0), None], entries.len(), None, Some(240.0), |ui, row, col| {
                                    let (name, value) = &entries[row];
                                    cell(ui, if col == 0 { name } else { value });
                                }) {
                                    let mut rest = entries.clone();
                                    rest.remove(i);
                                    app.ask = Some(Ask::WriteConfig(rest));
                                }
                                ui.label(RichText::new("Změny se projeví po restartu serveru. Předchozí verze souboru zůstává jako záloha .bak.").small().color(p.muted));
                                egui::CollapsingHeader::new("Obsah souboru").id_salt("cnf_raw").show(ui, |ui| {
                                    ui.monospace(safe_text(&config.raw));
                                });
                            }
                        }
                    });
                });
            },
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Log");
                        let has_file = app.session.snapshot.as_ref().is_some_and(|s| s.log_error.starts_with('/'));
                        let before = (app.log_from_file, app.log_lines);
                        ui.radio_value(&mut app.log_from_file, false, "journal");
                        ui.add_enabled_ui(has_file, |ui| {
                            ui.radio_value(&mut app.log_from_file, true, "error log");
                        });
                        egui::ComboBox::from_id_salt("log_lines")
                            .selected_text(format!("{} řádků", app.log_lines))
                            .show_ui(ui, |ui| {
                                for n in [100, 300, 1000, 3000] {
                                    ui.selectable_value(&mut app.log_lines, n, format!("{n} řádků"));
                                }
                            });
                        if before != (app.log_from_file, app.log_lines) || ui.button("Načíst").clicked() {
                            app.session.host_logs = None;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        search(ui, &mut app.filters[SERVICE]);
                        ui.radio_value(&mut app.log_level, 0, "vše");
                        ui.radio_value(&mut app.log_level, 2, "varování a chyby");
                        ui.radio_value(&mut app.log_level, 1, "jen chyby");
                    });
                    ui.separator();
                    let Some(lines) = app.session.host_logs.clone() else {
                        ui.label("Načítám log…");
                        return;
                    };
                    let visible: Vec<&String> = lines.iter().filter(|l| app.log_visible(l)).collect();
                    ui.label(RichText::new(format!("{} z {} řádků • nejnovější dole", visible.len(), lines.len())).small().color(p.muted));
                    egui::ScrollArea::both()
                        .id_salt("log_lines")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show_rows(ui, 18.0, visible.len(), |ui, range| {
                            for line in &visible[range] {
                                let text = RichText::new(safe_text(line)).monospace();
                                // One line per entry (fixed row height); long lines scroll sideways.
                                ui.add(
                                    egui::Label::new(if line.contains("ERROR") {
                                        text.color(p.severity(Severity::Critical))
                                    } else if line.contains("[Warning]") {
                                        text.color(p.warn)
                                    } else {
                                        text
                                    })
                                    .extend(),
                                );
                            }
                        });
                });
            },
        );
    }
    fn config_window(&mut self, ctx: &egui::Context) {
        let Some(name) = self.config_variable.clone() else {
            return;
        };
        let mut open = true;
        let p = Palette::new(self.theme);
        let mut review = None;
        centered_window("Uložit do konfigurace", ctx).open(&mut open).show(ctx, |ui| {
            ui.label(format!("Volba {} v souboru {}", safe_text(&name), host::HARBOR_CONFIG_FILE));
            field(ui, "Hodnota (velikosti lze s příponou K/M/G)", &mut self.config_value, false);
            ui.label(RichText::new("Uloží se do vlastního souboru Harboru, ostatní konfigurace se nemění. Server hodnotu ověří nanečisto; projeví se po restartu.").small().color(p.muted));
            if ui.button("Zkontrolovat…").clicked() {
                review = Some(name.clone());
            }
        });
        if let Some(name) = review {
            match self.config_change(&name, &self.config_value.clone()) {
                Ok(entries) => {
                    self.ask = Some(Ask::WriteConfig(entries));
                    self.config_variable = None;
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        if !open {
            self.config_variable = None;
        }
    }
    /// Harbor's option file with `name = value` set (validated against the server's
    /// metadata for the variable).
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
    fn backups_page(&mut self, ui: &mut egui::Ui, p: Palette) {
        if !self.backups_loaded {
            self.reload_backups();
        }
        let finished = self.session.jobs.iter().filter(|j| j.finished()).count();
        if finished != self.jobs_finished_seen {
            self.jobs_finished_seen = finished;
            self.reload_backups();
        }
        let dir = self.settings.backup_dir();
        toolbar(ui, |ui| {
            let real = self.session.snapshot.as_ref().is_some_and(|s| !s.demo);
            if ui
                .add_enabled(real, egui::Button::new("+ Nová záloha…"))
                .on_disabled_hover_text("Připojte skutečný server (demo nemá data).")
                .clicked()
            {
                self.backup_form_open = true;
            }
            if ui.button("Plánované zálohy…").clicked() {
                self.schedule_open = true;
                self.schedules = schedule::list();
            }
            if ui.button("Načíst znovu").clicked() {
                self.reload_backups();
            }
            ui.label(RichText::new(dir.display().to_string()).color(p.muted));
        });
        if !self.session.jobs.is_empty() {
            panel_frame(p).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong("Úlohy");
                    if ui.button("Skrýt dokončené").clicked() {
                        self.session.jobs.retain(|j| !j.finished());
                        self.jobs_finished_seen = 0;
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("jobs")
                    .max_height(150.0)
                    .show(ui, |ui| {
                        for job in self.session.jobs.iter().rev() {
                            let progress = job.progress();
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::ProgressBar::new(progress.fraction())
                                        .desired_width(160.0)
                                        .text(format!(
                                            "{} %",
                                            (progress.fraction() * 100.0) as u32
                                        )),
                                );
                                if !progress.finished()
                                    && ui
                                        .add_enabled(
                                            !job.cancel_requested(),
                                            egui::Button::new("Zrušit"),
                                        )
                                        .clicked()
                                {
                                    job.cancel();
                                }
                                let text = match &progress.result {
                                    None => format!("{} • {}", job.title, progress.phase),
                                    Some(Ok(summary)) => summary.clone(),
                                    Some(Err(error)) => format!("{}: {error}", job.title),
                                };
                                let text = RichText::new(safe_text(&text));
                                ui.add(
                                    egui::Label::new(if matches!(progress.result, Some(Err(_))) {
                                        text.color(p.severity(Severity::Critical))
                                    } else {
                                        text
                                    })
                                    .truncate(),
                                );
                            });
                        }
                    });
            });
            ui.add_space(8.0);
        }
        let files = self.backups.clone();
        self.split(
            ui,
            "backups_split",
            |app, ui| {
                panel_frame(p).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    if files.is_empty() {
                        ui.label("V adresáři zatím nejsou žádné zálohy.");
                    }
                    let selected = files
                        .iter()
                        .position(|f| app.selected_backup.as_ref() == Some(&f.path));
                    if let Some(i) = data_table(
                        ui,
                        "backups",
                        &["SOUBOR", "VYTVOŘENO (UTC)", "VELIKOST", "DATABÁZE"],
                        &[Some(210.0), Some(150.0), Some(80.0), None],
                        files.len(),
                        selected,
                        None,
                        |ui, row, col| {
                            let f = &files[row];
                            match (col, &f.manifest) {
                                (0, _) => cell(ui, &f.name()),
                                (1, Ok(m)) => {
                                    cell(ui, &m.created_utc.replace('T', " ").replace('Z', ""))
                                }
                                (2, _) => cell(ui, &human_bytes(f.bytes)),
                                (3, Ok(m)) => cell(
                                    ui,
                                    &format!(
                                        "{}{}",
                                        m.databases
                                            .iter()
                                            .map(|d| d.name.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        if m.encrypted { " 🔒" } else { "" }
                                    ),
                                ),
                                (3, Err(_)) => {
                                    ui.label(RichText::new("nečitelná").color(p.warn));
                                }
                                _ => (),
                            }
                        },
                    ) {
                        app.selected_backup = Some(files[i].path.clone());
                        app.archive_password.clear();
                    }
                });
            },
            |app, ui| app.backup_detail(ui, p),
        );
    }
    fn backup_detail(&mut self, ui: &mut egui::Ui, p: Palette) {
        let Some(file) = self.selected_backup_file() else {
            empty_detail(ui, p, "Vyberte zálohu v seznamu.");
            return;
        };
        let manifest = file.manifest.clone().ok();
        let encrypted = manifest.as_ref().is_some_and(|m| m.encrypted);
        let real = self.session.snapshot.as_ref().is_some_and(|s| !s.demo);
        self.detail_pane(
            ui,
            p,
            "backup_detail",
            |app, ui| {
                ui.heading(safe_text(&file.name()));
                if ui.button("Ověřit").clicked() {
                    app.session.start_job(JobSpec::Verify {
                        path: file.path.clone(),
                        encryption: app.archive_password(encrypted),
                    });
                }
                if ui
                    .add_enabled(manifest.is_some() && real, egui::Button::new("Obnovit…"))
                    .on_disabled_hover_text("Obnovit lze do připojeného skutečného serveru.")
                    .clicked()
                {
                    if let Some(db) = manifest.as_ref().and_then(|m| m.databases.first()) {
                        app.restore_db = db.name.clone();
                        app.restore_target = db.name.clone();
                        app.restore_replace = false;
                        app.restore_open = true;
                    }
                }
                if ui.button("Smazat soubor…").clicked() {
                    app.ask_text.clear();
                    app.ask = Some(Ask::DeleteBackup(file.path.clone()));
                }
            },
            |app, ui| {
                let Some(m) = &manifest else {
                    ui.label(
                        RichText::new(
                            file.manifest
                                .as_ref()
                                .err()
                                .map(|e| safe_text(e))
                                .unwrap_or_default(),
                        )
                        .color(p.warn),
                    );
                    return;
                };
                if m.encrypted {
                    field(
                        ui,
                        "Heslo zálohy (pro ověření a obnovu)",
                        &mut app.archive_password,
                        true,
                    );
                }
                egui::Grid::new("manifest")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        let mut row = |label: &str, value: String| {
                            ui.label(RichText::new(label).color(p.muted));
                            ui.label(value);
                            ui.end_row();
                        };
                        row("Vytvořena", m.created_utc.clone());
                        row("Server", safe_text(&m.server_version));
                        row("Profil", safe_text(&m.profile));
                        row("Velikost", human_bytes(file.bytes));
                        row(
                            "Konzistence",
                            if m.consistent {
                                "jeden konzistentní snímek".into()
                            } else {
                                "netransakční tabulky mimo snímek".into()
                            },
                        );
                        row(
                            "Šifrování",
                            if m.encrypted {
                                "AES-256".into()
                            } else {
                                "ne".into()
                            },
                        );
                    });
                ui.add_space(6.0);
                ui.strong("Databáze v záloze");
                for db in &m.databases {
                    ui.label(format!(
                        "{} • {} tabulek • {} řádků • SQL {}",
                        safe_text(&db.name),
                        db.tables,
                        db.rows,
                        human_bytes(db.bytes)
                    ));
                }
                for warning in &m.warnings {
                    ui.label(RichText::new(format!("⚠ {}", safe_text(warning))).color(p.warn));
                }
            },
        );
    }
    fn backup_form_window(&mut self, ctx: &egui::Context) {
        if !self.backup_form_open {
            return;
        }
        let names: Vec<String> = self
            .session
            .snapshot
            .as_ref()
            .map(|s| {
                s.databases
                    .iter()
                    .filter(|d| {
                        !["information_schema", "performance_schema"].contains(&d.name.as_str())
                    })
                    .map(|d| d.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut open = true;
        let p = Palette::new(self.theme);
        centered_window("Nová záloha", ctx)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("Databáze ({} vybráno)", self.backup_selection.len()));
                    if ui.button("Všechny uživatelské").clicked() {
                        self.backup_selection = names
                            .iter()
                            .filter(|n| !Database::system_name(n))
                            .cloned()
                            .collect();
                    }
                    if ui.button("Nic").clicked() {
                        self.backup_selection.clear();
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("backup_dbs")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        egui::Grid::new("backup_db_grid").num_columns(3).show(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                let mut on = self.backup_selection.contains(name);
                                if ui.checkbox(&mut on, safe_text(name)).changed() {
                                    if on {
                                        self.backup_selection.insert(name.clone());
                                    } else {
                                        self.backup_selection.remove(name);
                                    }
                                }
                                if i % 3 == 2 {
                                    ui.end_row();
                                }
                            }
                        });
                    });
                ui.separator();
                field(ui, "Adresář záloh", &mut self.backup_dir_text, false);
                if ui.button("Uložit adresář jako výchozí").clicked() {
                    let dir = self.backup_dir_text.trim().to_string();
                    self.save(move |s| s.backup_dir = dir);
                    self.backups_loaded = false;
                }
                ui.checkbox(&mut self.encrypt, "Šifrovat (AES-256; heslo se nikam neukládá)");
                if self.encrypt {
                    field(ui, "Heslo zálohy (alespoň 8 znaků)", &mut self.encryption, true);
                    field(ui, "Heslo znovu", &mut self.encryption_again, true);
                }
                ui.label(
                    RichText::new("InnoDB se čte v jednom konzistentním snímku, server běží dál. Soubor je čitelný jen pro vás (0600).")
                        .small()
                        .color(p.muted),
                );
                if ui.button("Spustit zálohu").clicked() {
                    match self.backup_request() {
                        Ok(spec) => {
                            if self.session.start_job(spec) {
                                self.encryption.clear();
                                self.encryption_again.clear();
                                self.backup_form_open = false;
                            } else {
                                self.message = "Jiná operace právě běží; zkuste to za chvíli.".into();
                            }
                        }
                        Err(message) => self.message = message,
                    }
                }
            });
        if !open {
            self.backup_form_open = false;
        }
    }
    fn schedule_window(&mut self, ctx: &egui::Context) {
        if !self.schedule_open {
            return;
        }
        let mut open = true;
        let p = Palette::new(self.theme);
        centered_window("Plánované zálohy", ctx)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Uživatelský časovač systemd spouští tento program s příkazem backup. Databáze a adresář se berou z formuláře Nová záloha (žádná vybraná = všechny uživatelské).")
                        .color(p.muted),
                );
                if self.schedules.is_empty() {
                    ui.label("Zatím žádný plán.");
                }
                for installed in self.schedules.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} • {} • {}",
                            safe_text(&installed.description),
                            safe_text(&installed.calendar),
                            match installed.active {
                                Some(true) => "aktivní",
                                Some(false) => "neaktivní",
                                None => "stav neznámý",
                            }
                        ));
                        if ui.button("Odebrat…").clicked() {
                            self.ask_text.clear();
                            self.ask = Some(Ask::Unschedule(installed.name.clone()));
                        }
                    });
                }
                ui.separator();
                ui.strong("Nový plán pro připojený profil");
                field(ui, "Kdy (daily, *-*-* 02:30, Mon..Fri 22:00)", &mut self.schedule_calendar, false);
                ui.horizontal(|ui| {
                    ui.label("Uchovávat záloh");
                    ui.add(egui::DragValue::new(&mut self.schedule_keep).range(1..=365));
                });
                field(ui, "Soubor s heslem serveru (chmod 600; prázdné = socket bez hesla)", &mut self.schedule_password_file, false);
                field(ui, "Soubor s heslem šifrování (volitelné, chmod 600)", &mut self.schedule_encryption_file, false);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Náhled jednotek").clicked() {
                        self.schedule_preview = match self.schedule_form().and_then(|s| {
                            let binary = schedule::current_binary().map_err(|e| e.to_string())?;
                            schedule::render_units(&s, &binary).map_err(|e| e.to_string())
                        }) {
                            Ok(units) => Some(units),
                            Err(message) => {
                                self.message = message;
                                None
                            }
                        };
                    }
                    if ui.button("Naplánovat").clicked() {
                        let result = self.schedule_form().and_then(|s| {
                            let binary = schedule::current_binary().map_err(|e| e.to_string())?;
                            schedule::install(&s, &binary).map_err(|e| e.to_string())
                        });
                        self.message = match result {
                            Ok(note) => note,
                            Err(message) => message,
                        };
                        self.schedules = schedule::list();
                    }
                });
                if let Some((service, timer)) = &self.schedule_preview {
                    egui::ScrollArea::vertical().id_salt("units").max_height(220.0).show(ui, |ui| {
                        ui.monospace(safe_text(service));
                        ui.monospace(safe_text(timer));
                    });
                }
            });
        if !open {
            self.schedule_open = false;
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui, p: Palette) {
        egui::ScrollArea::vertical()
            .id_salt("settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                card(ui, p, |ui| {
                    ui.set_min_width(ui.available_width().min(720.0));
                    ui.heading("Vzhled desktopu");
                    ui.horizontal_wrapped(|ui| {
                        for id in ThemeId::DESKTOP {
                            if ui.radio_value(&mut self.theme, id, id.label()).changed() {
                                self.save(move |s| s.desktop_theme = id.key().into());
                            }
                        }
                    });
                    ui.label(
                        RichText::new(
                            "Spojení, rozpracované formuláře i výběr se při přepnutí zachovají.",
                        )
                        .color(p.muted),
                    );
                    ui.add_space(8.0);
                    ui.heading("Obnovování dat");
                    let mut refresh = self.refresh_draft.unwrap_or(self.settings.refresh_seconds);
                    let slider = ui.add(egui::Slider::new(&mut refresh, 1..=300).text("sekund"));
                    self.refresh_slider(
                        refresh,
                        slider.dragged(),
                        slider.drag_stopped(),
                        slider.changed(),
                    );
                });
            });
    }
    fn reload_backups(&mut self) {
        self.backups = list_backups(&self.settings.backup_dir());
        self.schedules = schedule::list();
        self.backups_loaded = true;
        if self
            .selected_backup
            .as_ref()
            .is_some_and(|p| !self.backups.iter().any(|b| &b.path == p))
        {
            self.selected_backup = None;
        }
    }
    fn selected_backup_file(&self) -> Option<BackupFile> {
        let path = self.selected_backup.as_ref()?;
        self.backups.iter().find(|b| &b.path == path).cloned()
    }
    /// The backup this form describes, or a message why it cannot start.
    fn backup_request(&self) -> std::result::Result<JobSpec, String> {
        let profile = self
            .session
            .profile
            .clone()
            .filter(|_| self.session.snapshot.as_ref().is_some_and(|s| !s.demo))
            .ok_or("Zálohovat lze jen připojený skutečný server (ne demo).")?;
        if self.backup_selection.is_empty() {
            return Err("Vyberte alespoň jednu databázi.".into());
        }
        let encryption = if self.encrypt {
            if self.encryption.chars().count() < 8 {
                return Err("Heslo pro šifrování musí mít alespoň 8 znaků.".into());
            }
            if self.encryption != self.encryption_again {
                return Err("Hesla pro šifrování se neshodují.".into());
            }
            Some(Secret(self.encryption.clone()))
        } else {
            None
        };
        let dir = std::path::PathBuf::from(self.backup_dir_text.trim());
        if self.backup_dir_text.trim().is_empty() {
            return Err("Zadejte adresář záloh.".into());
        }
        Ok(JobSpec::Backup(BackupRequest {
            path: dir.join(backup_file_name(
                &profile.name,
                std::time::SystemTime::now(),
            )),
            profile: profile.name,
            databases: self.backup_selection.iter().cloned().collect(),
            encryption,
        }))
    }
    fn archive_password(&self, encrypted: bool) -> Option<Secret> {
        (encrypted && !self.archive_password.is_empty())
            .then(|| Secret(self.archive_password.clone()))
    }
    /// Schedule for the connected, saved profile from the form.
    fn schedule_form(&self) -> std::result::Result<schedule::Schedule, String> {
        let profile = self
            .session
            .profile
            .clone()
            .ok_or("Nejprve se připojte k serveru.")?;
        if self.settings.profile(&profile.id).is_none() {
            return Err("Plán potřebuje uložený profil; uložte ho v Připojení.".into());
        }
        let file = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| std::path::PathBuf::from(text))
        };
        Ok(schedule::Schedule {
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            calendar: self.schedule_calendar.clone(),
            keep: self.schedule_keep,
            databases: self.backup_selection.iter().cloned().collect(),
            dir: std::path::PathBuf::from(self.backup_dir_text.trim()),
            password_file: file(&self.schedule_password_file),
            encryption_file: file(&self.schedule_encryption_file),
        })
    }
    fn restore_window(&mut self, ctx: &egui::Context) {
        if !self.restore_open {
            return;
        }
        let Some(file) = self.selected_backup_file() else {
            self.restore_open = false;
            return;
        };
        let Ok(manifest) = file.manifest.clone() else {
            self.restore_open = false;
            return;
        };
        let mut open = true;
        egui::Window::new(format!("Obnovit ze zálohy {}", safe_text(&file.name())))
            .id(egui::Id::new("restore"))
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Databáze ze zálohy:");
                for db in &manifest.databases {
                    if ui
                        .radio_value(&mut self.restore_db, db.name.clone(), safe_text(&db.name))
                        .changed()
                    {
                        self.restore_target = db.name.clone();
                    }
                }
                field(
                    ui,
                    "Obnovit do databáze (nový název = kopie vedle originálu)",
                    &mut self.restore_target,
                    false,
                );
                let exists = self
                    .session
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.databases.iter().any(|d| d.name == self.restore_target));
                if exists {
                    ui.checkbox(
                        &mut self.restore_replace,
                        "Databáze existuje: smazat ji a nahradit obsahem zálohy",
                    );
                }
                if manifest.encrypted {
                    field(ui, "Heslo zálohy", &mut self.archive_password, true);
                }
                if ui.button("Zkontrolovat…").clicked() {
                    match identifier(&self.restore_target) {
                        Err(error) => self.message = error.to_string(),
                        Ok(_) => {
                            self.ask_text.clear();
                            self.ask = Some(Ask::Restore(RestoreRequest {
                                path: file.path.clone(),
                                encryption: self.archive_password(manifest.encrypted),
                                database: self.restore_db.clone(),
                                target: self.restore_target.clone(),
                                replace: exists && self.restore_replace,
                            }));
                        }
                    }
                }
            });
        if !open {
            self.restore_open = false;
        }
    }
    fn ask_modal(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.ask.clone() else {
            return;
        };
        egui::Modal::new(egui::Id::new("ask")).show(ctx, |ui| {
            ui.set_max_width(680.0);
            let (heading, lines, name, action) = match &ask {
                Ask::Restore(r) => (
                    "Obnovit databázi ze zálohy",
                    vec![
                        format!(
                            "Server: {}",
                            self.session
                                .profile
                                .as_ref()
                                .map(|p| safe_text(&p.name))
                                .unwrap_or_default()
                        ),
                        safe_text(&JobSpec::Restore(r.clone()).title()),
                        "Nejdřív se ověří kontrolní součet, pak se databáze vytvoří a naplní."
                            .into(),
                        if r.replace {
                            "NEVRATNÉ: stávající databáze se nejdřív smaže.".into()
                        } else {
                            "Cílová databáze nesmí existovat.".into()
                        },
                    ],
                    r.replace.then(|| r.target.clone()),
                    "Obnovit",
                ),
                Ask::DeleteBackup(path) => (
                    "Smazat soubor zálohy?",
                    vec![safe_text(&path.display().to_string())],
                    None,
                    "Smazat",
                ),
                Ask::Service(action) => {
                    let sessions = self.session.snapshot.as_ref().map_or(0, |s| s.connections);
                    let unit = self
                        .session
                        .profile
                        .as_ref()
                        .map(|p| p.host.service.clone())
                        .unwrap_or_default();
                    (
                        match action {
                            ServiceAction::Start => "Spustit MariaDB?",
                            ServiceAction::Restart => "Restartovat MariaDB?",
                            ServiceAction::Stop => "Zastavit MariaDB?",
                        },
                        vec![
                            format!(
                                "Server: {} • služba {}",
                                self.session
                                    .profile
                                    .as_ref()
                                    .map(|p| safe_text(&p.name))
                                    .unwrap_or_default(),
                                safe_text(&unit)
                            ),
                            match action {
                                ServiceAction::Start => "Služba se spustí.".to_string(),
                                _ => format!(
                                    "Aktuálně {sessions} spojení; všechna se přeruší a běžící transakce se vrátí zpět."
                                ),
                            },
                        ],
                        (*action == ServiceAction::Stop).then_some(unit),
                        match action {
                            ServiceAction::Start => "Spustit",
                            ServiceAction::Restart => "Restartovat",
                            ServiceAction::Stop => "Zastavit",
                        },
                    )
                }
                Ask::WriteConfig(entries) => {
                    let mut lines = vec![
                        "Nový obsah souboru Harboru (ověří se nanečisto binárkou serveru):".to_string(),
                    ];
                    lines.extend(
                        host::render_config(entries, "…")
                            .unwrap_or_else(|e| e.to_string())
                            .lines()
                            .filter(|l| !l.starts_with('#'))
                            .map(|l| format!("    {}", safe_text(l))),
                    );
                    lines.push("Projeví se po restartu serveru; předchozí soubor zůstane jako .bak.".into());
                    (
                        "Uložit konfiguraci na server?",
                        lines,
                        None,
                        "Uložit",
                    )
                }
                Ask::Unschedule(name) => (
                    "Odebrat plánovanou zálohu?",
                    vec![format!(
                        "Časovač {} se zastaví a jeho soubory se smažou; zálohy zůstanou.",
                        safe_text(name)
                    )],
                    None,
                    "Odebrat",
                ),
            };
            ui.heading(heading);
            for line in lines {
                ui.label(line);
            }
            if let Some(name) = &name {
                ui.label(
                    RichText::new(format!("Pro potvrzení napište název: {}", safe_text(name)))
                        .strong(),
                );
                ui.text_edit_singleline(&mut self.ask_text);
            }
            let unlocked = name.as_ref().is_none_or(|n| *n == self.ask_text);
            ui.horizontal(|ui| {
                if ui.button("Zrušit").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    self.ask = None;
                }
                if ui
                    .add_enabled(unlocked, egui::Button::new(action))
                    .clicked()
                {
                    match ask {
                        Ask::Restore(request) => {
                            if self.session.start_job(JobSpec::Restore(request)) {
                                self.ask = None;
                                self.restore_open = false;
                            } else {
                                self.message =
                                    "Jiná operace právě běží; zkuste to za chvíli.".into();
                            }
                        }
                        Ask::DeleteBackup(path) => {
                            if let Err(error) = std::fs::remove_file(&path) {
                                self.message = format!("Soubor nelze smazat: {error}");
                            }
                            self.ask = None;
                            self.reload_backups();
                        }
                        Ask::Service(action) => {
                            if self.session.host_request(HostRequest::Control(action)) {
                                self.ask = None;
                            } else {
                                self.message = "Na serveru právě běží jiný příkaz.".into();
                            }
                        }
                        Ask::WriteConfig(entries) => {
                            let profile = self
                                .session
                                .profile
                                .as_ref()
                                .map(|p| p.name.clone())
                                .unwrap_or_default();
                            if self
                                .session
                                .host_request(HostRequest::WriteConfig { entries, profile })
                            {
                                self.ask = None;
                            } else {
                                self.message = "Na serveru právě běží jiný příkaz.".into();
                            }
                        }
                        Ask::Unschedule(name) => {
                            if let Err(error) = schedule::remove(&name) {
                                self.message = error.to_string();
                            }
                            self.ask = None;
                            self.schedules = schedule::list();
                        }
                    }
                }
            });
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
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
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
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
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
        self.restore_window(ctx);
        self.backup_form_window(ctx);
        self.config_window(ctx);
        self.schedule_window(ctx);
        self.ask_modal(ctx);
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
        self.host_autoload();
        // Results arrive over a channel egui cannot see: poll quickly only while work is
        // in flight, otherwise wake for the next refresh.
        ctx.request_repaint_after(self.session.next_wakeup(self.settings.refresh_seconds));
        let p = Palette::new(self.theme);
        if self.styled != Some(self.theme) {
            self.style(ctx, p);
            self.styled = Some(self.theme);
        }
        egui::SidePanel::left("navigation")
            .exact_width(212.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.side).inner_margin(18.0))
            .show(ctx, |ui| {
                if self.theme == ThemeId::Original {
                    dark_sidebar(ui.visuals_mut(), p);
                }
                ui.add_space(6.0);
                ui.heading(RichText::new("h  harbor").size(26.0));
                ui.small("MARIADB WORKSPACE");
                ui.add_space(14.0);
                ui.label(
                    self.session
                        .profile
                        .as_ref()
                        .map(|p| safe_text(&p.name))
                        .unwrap_or_else(|| "Žádné spojení".into()),
                );
                ui.small(safe_text(&self.session.status));
                ui.add_space(14.0);
                for (index, label) in PAGES.iter().enumerate() {
                    if ui
                        .add_sized(
                            [176.0, 34.0],
                            egui::Button::new(*label).selected(self.page == index),
                        )
                        .clicked()
                    {
                        self.page = index;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.small(format!("Harbor {}", env!("CARGO_PKG_VERSION")));
                });
            });
        egui::TopBottomPanel::top("status")
            .frame(
                egui::Frame::new()
                    .fill(p.surface)
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        self.session
                            .profile
                            .as_ref()
                            .map(|p| safe_text(&p.name))
                            .unwrap_or_else(|| "—".into()),
                    );
                    ui.strong(&self.session.status);
                    if self.session.worker.busy {
                        ui.spinner();
                    }
                    let running: Vec<Progress> = self
                        .session
                        .jobs
                        .iter()
                        .filter(|j| !j.finished())
                        .map(|j| j.progress())
                        .collect();
                    if let Some(progress) = running.first() {
                        if ui
                            .link(format!(
                                "{} {} %",
                                safe_text(&progress.phase),
                                (progress.fraction() * 100.0) as u32
                            ))
                            .clicked()
                        {
                            self.page = BACKUPS;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Obnovit (F5)").clicked() {
                            self.session.request(Operation::Refresh);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ctx, |ui| {
                let font = if self.theme == ThemeId::Terra {
                    FontFamily::Name("serif".into())
                } else {
                    FontFamily::Proportional
                };
                let title = if self.page == OVERVIEW && self.theme == ThemeId::Original {
                    "Server pod kontrolou."
                } else {
                    PAGES[self.page]
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(title).font(FontId::new(
                        desktop(self.theme).typography.title_size.min(34.0),
                        font,
                    )));
                    ui.label(RichText::new(page_subtitle(self.page)).color(p.muted));
                });
                if self.session.stale {
                    banner(
                        ui,
                        p,
                        Severity::Warning,
                        "Data nejsou aktuální — ověřte spojení.",
                        None,
                    );
                }
                if let Some(error) = self.session.error.clone() {
                    banner(ui, p, Severity::Critical, &format!("CHYBA: {error}"), None);
                }
                if !self.message.is_empty() {
                    let message = self.message.clone();
                    if banner(ui, p, Severity::Info, &message, Some("Zavřít")) {
                        self.message.clear();
                    }
                }
                ui.add_space(6.0);
                match self.page {
                    SETTINGS => self.settings(ui, p),
                    DATABASES => self.databases(ui, p),
                    ACCOUNTS => self.accounts(ui, p),
                    PROCESSES => self.processes(ui, p),
                    VARIABLES => self.variables(ui, p),
                    HEALTH => self.health(ui, p),
                    BACKUPS => self.backups_page(ui, p),
                    SERVICE => self.service_page(ui, p),
                    CONNECTION => self.connection(ui, p),
                    _ => self.overview(ui, p),
                }
            });
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.session.request(Operation::Refresh);
        }
        self.dialogs(ctx);
    }
}
/// sudoers rule allowing exactly the commands Harbor runs with elevation.
const SUDOERS_EXAMPLE: &str = "uzivatel ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/journalctl, \\\n  /usr/bin/tail, /usr/bin/cat, /usr/bin/tee, /usr/bin/cp, /usr/bin/mv, \\\n  /usr/bin/rm, /usr/bin/mktemp, /usr/sbin/mariadbd, /usr/libexec/mariadbd";
/// Short description under the page title.
fn page_subtitle(page: usize) -> &'static str {
    match page {
        OVERVIEW => "stav a provoz připojeného serveru",
        DATABASES => "velikosti, tabulky, struktura a údržba",
        ACCOUNTS => "účty, role a jejich oprávnění",
        PROCESSES => "kdo je připojen a co právě běží",
        VARIABLES => "parametry serveru a jejich změna za běhu",
        HEALTH => "kontrola bezpečnosti, výkonu a údržby",
        SERVICE => "spuštění a zastavení, disk, logy a trvalá konfigurace",
        BACKUPS => "zálohy do ZIP, obnova a plánování",
        CONNECTION => "uložené profily a připojení",
        _ => "vzhled a chování aplikace",
    }
}
/// Frame of a list or detail panel.
fn panel_frame(p: Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(p.radius.min(10))
        .inner_margin(12.0)
}
/// The row of filters and page actions above the panes.
fn toolbar(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(add);
    ui.add_space(6.0);
}
fn search(ui: &mut egui::Ui, text: &mut String) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text("🔍 Hledat…")
            .margin(egui::Margin::symmetric(8, 5))
            .desired_width(220.0),
    );
}
fn empty_detail(ui: &mut egui::Ui, p: Palette, text: &str) {
    panel_frame(p).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(text).color(p.muted));
        });
    });
}
/// A coloured message line; returns true when its button was clicked.
fn banner(
    ui: &mut egui::Ui,
    p: Palette,
    severity: Severity,
    text: &str,
    button: Option<&str>,
) -> bool {
    let mut clicked = false;
    let color = p.severity(severity);
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.6)))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(safe_text(text)).color(p.ink)).wrap());
                if let Some(label) = button {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clicked = ui.button(label).clicked();
                    });
                }
            });
        });
    clicked
}
/// One table cell: single line, cut with an ellipsis, full text on hover.
fn cell(ui: &mut egui::Ui, text: &str) {
    let text = safe_text(text);
    ui.add(egui::Label::new(&text).truncate())
        .on_hover_text(text);
}
/// A data table with a sticky header, resizable columns and clickable rows. Returns
/// the clicked row. `max_height` limits it inside a scrolling detail.
#[allow(clippy::too_many_arguments)]
fn data_table(
    ui: &mut egui::Ui,
    id: &str,
    headers: &[&str],
    widths: &[Option<f32>],
    rows: usize,
    selected: Option<usize>,
    max_height: Option<f32>,
    mut add_cell: impl FnMut(&mut egui::Ui, usize, usize),
) -> Option<usize> {
    use egui_extras::{Column, TableBuilder};
    let mut clicked = None;
    let muted = ui.visuals().weak_text_color();
    let mut table = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .auto_shrink([false, max_height.is_some()])
        .min_scrolled_height(0.0);
    if let Some(height) = max_height {
        table = table.max_scroll_height(height);
    }
    for width in widths {
        table = table.column(match width {
            Some(w) => Column::initial(*w).at_least(40.0).clip(true),
            None => Column::remainder().at_least(40.0).clip(true),
        });
    }
    table
        .header(28.0, |mut header| {
            for title in headers {
                header.col(|ui| {
                    ui.label(RichText::new(*title).small().strong().color(muted));
                });
            }
        })
        .body(|body| {
            body.rows(30.0, rows, |mut row| {
                let index = row.index();
                row.set_selected(selected == Some(index));
                for column in 0..headers.len() {
                    row.col(|ui| {
                        // Cell text must not capture the click (text selection cursor):
                        // the whole row selects.
                        ui.style_mut().interaction.selectable_labels = false;
                        add_cell(ui, index, column)
                    });
                }
                if row.response().clicked() {
                    clicked = Some(index);
                }
            });
        });
    clicked
}
/// Dialog windows open in the middle of the window.
fn centered_window<'a>(title: &'a str, ctx: &egui::Context) -> egui::Window<'a> {
    egui::Window::new(title)
        .id(egui::Id::new(title))
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.screen_rect().center())
        .collapsible(false)
        .resizable(true)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.screen_rect().center())
        .default_width(580.0)
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
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, password: bool) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .password(password)
            .margin(egui::Margin::symmetric(8, 5))
            .desired_width(ui.available_width().clamp(200.0, 520.0)),
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
#[cfg(debug_assertions)]
pub mod tour;
