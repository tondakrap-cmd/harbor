use super::*;
use std::time::Duration;

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

#[test]
fn original_sidebar_buttons_are_readable() {
    let p = Palette::new(ThemeId::Original);
    let mut v = egui::Visuals::light();
    dark_sidebar(&mut v, p);
    let text = v.override_text_color.unwrap();
    for fill in [
        v.widgets.inactive.weak_bg_fill,
        v.widgets.hovered.weak_bg_fill,
        v.widgets.active.weak_bg_fill,
        v.selection.bg_fill,
        p.side,
    ] {
        assert!(
            contrast(text, fill) >= 4.5,
            "{fill:?}: {}",
            contrast(text, fill)
        );
    }
}
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
            for page in 0..PAGES.len() {
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
    for page in [DATABASES, ACCOUNTS] {
        app.page = page;
        let all = texts(&frame(&ctx, &mut app).shapes).join("");
        assert!(
            !all.contains('\u{202e}') && !all.contains('\u{2066}'),
            "page {page}"
        );
        assert!(all.contains("faktury") || page == ACCOUNTS);
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
    app.filters[ACCOUNTS] = "ANALYTICS".into();
    assert_eq!(
        snapshot
            .accounts
            .iter()
            .filter(|a| app.account_visible(a))
            .count(),
        1
    );
    assert!(snapshot.databases.iter().any(|d| app.database_visible(d)));
    app.filters[PROCESSES] = "select date".into();
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
    app.draft = Some(Draft::ChangePassword);
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
    assert!(app.draft.is_none());
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
    app.draft = Some(Draft::RoleMembership);
    frame(&ctx, &mut app);
    assert!(app.selected_account.is_none());
    assert!(app.draft.is_none());
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
fn settle(ctx: &egui::Context, app: &mut Harbor) {
    let start = std::time::Instant::now();
    frame(ctx, app);
    while (app.session.worker.busy || app.session.has_pending())
        && start.elapsed() < Duration::from_secs(3)
    {
        std::thread::sleep(Duration::from_millis(5));
        frame(ctx, app);
    }
}
fn demo() -> (egui::Context, Harbor) {
    let ctx = egui::Context::default();
    let mut app = Harbor::new(&ctx, true);
    app.store = None;
    wait_for_snapshot(&ctx, &mut app);
    (ctx, app)
}
#[test]
fn database_and_table_details_render_with_maintenance_preview() {
    let (ctx, mut app) = demo();
    app.page = DATABASES;
    assert!(app.session.request(Operation::Tables("commerce".into())));
    app.selected_db = Some("commerce".into());
    settle(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("bez primárního klíče"), "warnings shown");
    assert!(all.contains("audit_log (demo)"));
    assert!(app.session.request(Operation::Table {
        database: "commerce".into(),
        table: "orders (demo)".into(),
    }));
    app.selected_table = Some("orders (demo)".into());
    settle(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("customer_id") && all.contains("fk_customer"));
    app.confirm(Change::Maintenance {
        database: "commerce".into(),
        table: "orders".into(),
        action: Maintenance::Analyze,
    });
    // A modal is sized on its first frame and painted on the next.
    frame(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(
        all.contains("ANALYZE TABLE `commerce`.`orders`"),
        "SQL preview shown"
    );
}
#[test]
fn destructive_changes_need_the_typed_name() {
    let drop = Change::DropDatabase {
        name: "commerce".into(),
    };
    assert!(!unlocked(&drop, ""));
    assert!(!unlocked(&drop, "Commerce"));
    assert!(unlocked(&drop, "commerce"));
    assert!(unlocked(&Change::CreateRole { name: "r".into() }, ""));
}
#[test]
fn privilege_editor_reviews_only_the_difference() {
    let (ctx, mut app) = demo();
    let account = app.session.snapshot.as_ref().unwrap().accounts[0].clone();
    app.page = ACCOUNTS;
    app.selected_account = Some(account.clone());
    assert!(app.session.request(Operation::Grants(account.clone())));
    settle(&ctx, &mut app);
    let scope = GrantScope::Database("analytics".into());
    app.privilege_editor = Some(PrivilegeEditor {
        account: account.clone(),
        fixed: Some(scope.clone()),
        scope_text: String::new(),
        loaded_for: None,
        wanted: BTreeSet::new(),
    });
    frame(&ctx, &mut app);
    let editor = app.privilege_editor.as_mut().unwrap();
    assert_eq!(
        editor.wanted.iter().cloned().collect::<Vec<_>>(),
        ["SELECT", "SHOW VIEW"],
        "checkboxes start from the current grants"
    );
    editor.wanted.remove("SHOW VIEW");
    editor.wanted.insert("INSERT".into());
    let current = app
        .session
        .account
        .as_ref()
        .unwrap()
        .privileges_at(&scope)
        .unwrap()
        .privileges
        .clone();
    let (grant, revoke) = privilege_diff(&current, &app.privilege_editor.as_ref().unwrap().wanted);
    assert_eq!(
        (grant, revoke),
        (vec!["INSERT".to_string()], vec!["SHOW VIEW".to_string()])
    );
    // A typed scope that does not parse shows an error instead of a change.
    app.privilege_editor.as_mut().unwrap().fixed = None;
    app.privilege_editor.as_mut().unwrap().scope_text = "a.b.c".into();
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("Rozsah zadejte"));
}
#[test]
fn variables_and_health_pages_load_once_and_filter() {
    let (ctx, mut app) = demo();
    app.page = VARIABLES;
    settle(&ctx, &mut app);
    let variables = app.session.variables.clone().expect("autoloaded");
    app.key_only = true;
    app.changed_only = true;
    let visible: Vec<&Variable> = variables
        .iter()
        .filter(|v| app.variable_visible(v))
        .collect();
    assert!(visible
        .iter()
        .all(|v| v.changed() && v.key_description().is_some()));
    assert!(
        !visible.iter().any(|v| v.name == "wait_timeout"),
        "default value hidden"
    );
    app.selected_variable = Some("max_connections".into());
    app.variable_value = "300".into();
    let Some(Change::SetGlobal { value, .. }) = app.draft_change(Draft::SetVariable) else {
        panic!("set global draft")
    };
    assert_eq!(value, "300");
    app.page = HEALTH;
    settle(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("Tabulky bez primárního klíče"), "{all}");
    // Leaving and returning does not re-run the check automatically.
    let first = app.session.health.as_ref().unwrap().0;
    app.page = OVERVIEW;
    frame(&ctx, &mut app);
    app.page = HEALTH;
    settle(&ctx, &mut app);
    assert_eq!(app.session.health.as_ref().unwrap().0, first);
}
/// JobSpec carries secrets and has no Debug, so no unwrap_err.
fn refused(result: std::result::Result<JobSpec, String>) -> String {
    match result {
        Err(message) => message,
        Ok(_) => panic!("request was accepted"),
    }
}
#[test]
fn backups_page_validates_forms_and_lists_archives() {
    let (ctx, mut app) = demo();
    let dir = std::env::temp_dir().join(format!("harbor-desktop-backups-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cizi.zip"), b"not a zip").unwrap();
    app.settings.backup_dir = dir.display().to_string();
    app.backup_dir_text = dir.display().to_string();
    app.page = BACKUPS;
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(
        all.contains("Nová záloha"),
        "toolbar action shown (disabled in demo)"
    );
    assert!(
        all.contains("cizi.zip"),
        "foreign archive listed with an error"
    );
    assert!(app.backups[0].manifest.is_err());
    // Demo is refused before anything starts.
    app.backup_selection.insert("commerce".into());
    assert!(refused(app.backup_request()).contains("demo"));
    // Encryption password rules are checked in the form.
    let mut snapshot = (**app.session.snapshot.as_ref().unwrap()).clone();
    snapshot.demo = false;
    app.session.snapshot = Some(std::sync::Arc::new(snapshot));
    app.encrypt = true;
    app.encryption = Zeroizing::new("kratke".into());
    assert!(refused(app.backup_request()).contains("8 znaků"));
    app.encryption = Zeroizing::new("dostatečně dlouhé".into());
    app.encryption_again = Zeroizing::new("jiné".into());
    assert!(refused(app.backup_request()).contains("neshodují"));
    app.encryption_again = app.encryption.clone();
    let Ok(JobSpec::Backup(request)) = app.backup_request() else {
        panic!("valid request")
    };
    assert!(request.path.starts_with(&dir) && request.encryption.is_some());
    assert_eq!(request.databases, ["commerce"]);
    // A schedule needs a saved profile.
    assert!(app.schedule_form().unwrap_err().contains("uložený profil"));
    std::fs::remove_dir_all(dir).unwrap();
}
/// Screen position of the first painted text equal to `needle`.
fn text_position(shapes: &[egui::epaint::ClippedShape], needle: &str) -> Option<egui::Pos2> {
    fn walk(shape: &egui::Shape, needle: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| walk(s, needle)),
            _ => None,
        }
    }
    shapes.iter().find_map(|s| walk(&s.shape, needle))
}
fn click_at(ctx: &egui::Context, app: &mut Harbor, pos: egui::Pos2) {
    let screen = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        Vec2::new(1440.0, 960.0),
    ));
    let input = |events: Vec<egui::Event>| egui::RawInput {
        screen_rect: screen,
        events,
        ..Default::default()
    };
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = ctx.run(input(vec![egui::Event::PointerMoved(pos)]), |ctx| {
        app.render(ctx)
    });
    let _ = ctx.run(input(vec![button(true)]), |ctx| app.render(ctx));
    let _ = ctx.run(input(vec![button(false)]), |ctx| app.render(ctx));
}
#[test]
fn clicking_on_the_text_of_a_row_selects_the_row() {
    let (ctx, mut app) = demo();
    app.page = DATABASES;
    let screen = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1440.0, 960.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(screen.clone(), |ctx| app.render(ctx));
    let output = ctx.run(screen, |ctx| app.render(ctx));
    // Both the name and a value in another column of the row must select it.
    let pos = text_position(&output.shapes, "analytics").expect("row text painted");
    click_at(&ctx, &mut app, pos);
    assert_eq!(app.selected_db.as_deref(), Some("analytics"));
    let screen = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1440.0, 960.0),
        )),
        ..Default::default()
    };
    let output = ctx.run(screen, |ctx| app.render(ctx));
    let pos = text_position(&output.shapes, "identity").expect("row text painted");
    click_at(&ctx, &mut app, pos);
    assert_eq!(app.selected_db.as_deref(), Some("identity"));
}
#[test]
fn service_page_loads_from_the_host_and_guards_actions() {
    let (ctx, mut app) = demo();
    app.page = SERVICE;
    let start = std::time::Instant::now();
    while (app.session.host_logs.is_none() || app.session.host_busy())
        && start.elapsed() < Duration::from_secs(3)
    {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app
        .session
        .host_status
        .as_ref()
        .is_some_and(|s| s.running()));
    assert!(app.session.host_config.is_some());
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("běží") && all.contains("Konfigurace Harboru"));
    // Stopping needs the typed unit name; config values are validated first.
    app.ask = Some(Ask::Service(ServiceAction::Stop));
    frame(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(
        all.contains("Pro potvrzení napište název: mariadb"),
        "{all}"
    );
    app.ask = None;
    app.session.request(Operation::Variables);
    let start = std::time::Instant::now();
    while app.session.variables.is_none() && start.elapsed() < Duration::from_secs(3) {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        app.config_change("max_connections", "5").is_err(),
        "below minimum"
    );
    assert!(
        app.config_change("max_connections", "300 # x").is_err(),
        "comment injection"
    );
    let entries = app.config_change("max_connections", "300").unwrap();
    assert!(entries.contains(&("max_connections".into(), "300".into())));
    assert!(entries.iter().any(|(n, _)| n == "innodb_buffer_pool_size"));
}
#[test]
fn diagnostics_pages_and_console_render_in_demo() {
    let (ctx, mut app) = demo();
    for page in [PERFORMANCE, REPLICATION] {
        app.page = page;
        settle(&ctx, &mut app);
    }
    app.page = PROCESSES;
    app.locks_view = true;
    settle(&ctx, &mut app);
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("Kořenové blokující spojení: #1842"), "{all}");
    app.page = REPLICATION;
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("db-primary.demo") && all.contains("mariadb-bin.000042"));
    // A console write is only proposed; it runs after the modal confirmation.
    app.page = CONSOLE;
    app.console_sql = "DELETE FROM orders".into();
    assert_eq!(
        app.session
            .console("", &app.console_sql.clone(), false)
            .unwrap(),
        ConsoleKind::Write
    );
    assert!(app.session.console.is_none(), "nothing ran yet");
    app.console_sql = "SELECT 1".into();
    app.session.console("", "SELECT 1", false).unwrap();
    settle(&ctx, &mut app);
    let start = std::time::Instant::now();
    while app.session.console.as_ref().is_none_or(|c| c.running())
        && start.elapsed() < Duration::from_secs(3)
    {
        frame(&ctx, &mut app);
        std::thread::sleep(Duration::from_millis(5));
    }
    let all = texts(&frame(&ctx, &mut app).shapes).join("\n");
    assert!(all.contains("Zákazník 1 (demo)"), "{all}");
    // CSV export goes to a private file.
    let path = std::env::temp_dir().join(format!("harbor-console-{}.csv", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let csv = app
        .session
        .console
        .as_ref()
        .unwrap()
        .result()
        .unwrap()
        .unwrap()
        .csv();
    write_private(path.to_str().unwrap(), &csv).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        write_private(path.to_str().unwrap(), &csv).is_err(),
        "never overwrites"
    );
    std::fs::remove_file(path).unwrap();
}
