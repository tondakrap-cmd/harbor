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
                for page in 0..PAGES.len() {
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
    app.menu = MENU.iter().position(|(_, a)| *a == Action::NextTheme);
    app.key(KeyEvent::new(K::Enter, M::NONE));
    assert!(app.dialog.is_some());
    app.confirm = Some(Confirmation::new(Pending::Server(Change::CreateDatabase {
        name: "test".into(),
        collation: "utf8mb4_unicode_ci".into(),
    })));
    app.key(KeyEvent::new(K::Enter, M::NONE));
    assert!(app.confirm.is_none());
    assert!(!app.session.worker.busy);
}
#[test]
fn menu_opens_with_f10_f9_alt_m_but_not_enter() {
    let mut app = App::new(&[]);
    app.store = None;
    app.page = SERVER;
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
    app.page = DATABASES;
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
    assert!(!app.session.tables().is_empty());
}
#[test]
fn selection_follows_the_row_when_a_refresh_reorders_the_list() {
    let mut app = App::new(&["--demo".to_string()]);
    app.store = None;
    wait_idle(&mut app);
    app.page = PROCESSES;
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
    app.page = PROCESSES;
    app.run(Action::Search);
    app.dialog.as_mut().unwrap().fields[0].value = Zeroizing::new("SELECT DATE".into());
    app.dialog.as_mut().unwrap().focus = 2; // "Zkontrolovat"
    key(&mut app, K::Enter);
    assert_eq!(
        app.processes().len(),
        1,
        "process query matched case-insensitively"
    );
    app.page = DATABASES;
    assert!(
        !app.databases().is_empty(),
        "the database list has its own filter"
    );
    app.page = ACCOUNTS;
    app.filters[ACCOUNTS] = "ANALYTICS".into();
    assert_eq!(app.accounts().len(), 1);
}
#[test]
fn cancelled_confirmation_returns_the_same_draft_with_its_password() {
    let mut app = App::new(&["--demo".to_string()]);
    app.store = None;
    wait_idle(&mut app);
    app.run(Action::CreateAccount);
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
    app.page = ACCOUNTS;
    app.selected = 1;
    app.run(Action::ChangePassword);
    assert!(matches!(
        app.dialog.as_ref().unwrap().kind,
        Kind::Password(_)
    ));
    key(&mut app, K::Esc); // the draft is kept as suspended
    assert!(app.suspended.is_some());
    app.tick();
    assert!(app.suspended.is_some(), "same server keeps the draft");
    let account = app.accounts()[1].clone();
    let mut confirmation = Confirmation::new(Pending::Server(Change::ChangePassword {
        account,
        password: Secret::new("x".into()),
    }));
    confirmation.yes = true;
    app.confirm = Some(confirmation);
    app.run(Action::Demo); // open DEMO again = new server session
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
    app.page = PROFILES;
    app.selected = 0;
    app.run(Action::DeleteProfile);
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
    app.run(Action::DeleteProfile);
    key(&mut app, K::Tab);
    key(&mut app, K::Enter);
    assert_eq!(app.settings.profiles.len(), 1);
    assert_eq!(app.settings.profiles[0].name, "B");
    std::fs::remove_dir_all(directory).unwrap();
}
fn settle(app: &mut App) {
    let start = std::time::Instant::now();
    app.tick();
    while (app.session.worker.busy || app.session.has_pending())
        && start.elapsed() < std::time::Duration::from_secs(3)
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.tick();
    }
}
fn demo() -> App {
    let mut app = App::new(&["--demo".to_string()]);
    app.store = None;
    wait_idle(&mut app);
    app
}
fn render(app: &App, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, K::Char(c));
    }
}
#[test]
fn number_keys_switch_pages_and_every_menu_action_is_safe_everywhere() {
    let mut app = demo();
    for (i, page) in PAGES.iter().enumerate() {
        key(&mut app, K::Char(char::from(b'1' + i as u8)));
        assert_eq!(app.page, i, "{page}");
    }
    for page in 0..PAGES.len() {
        for (label, action) in MENU {
            if *action == Action::Quit {
                continue;
            }
            app.page = page;
            app.selected = 0;
            app.run(*action);
            settle(&mut app);
            for (w, h) in [(80, 24), (120, 35)] {
                render(&app, w, h);
            }
            app.dialog = None;
            app.suspended = None;
            app.picker = None;
            app.confirm = None;
            app.table_overlay = false;
            app.help = false;
            assert!(!app.quit, "{label}");
        }
    }
}
#[test]
fn database_detail_table_overlay_and_maintenance_preview() {
    let mut app = demo();
    app.run(Action::Page(DATABASES));
    app.selected = app
        .databases()
        .iter()
        .position(|d| d.name == "commerce")
        .unwrap();
    key(&mut app, K::Enter);
    settle(&mut app);
    let screen = render(&app, 120, 35);
    assert!(
        screen.contains("bez primárního klíče"),
        "warnings in the detail"
    );
    key(&mut app, K::Tab);
    key(&mut app, K::Down);
    assert_eq!(app.table_cursor, 1);
    key(&mut app, K::Enter);
    settle(&mut app);
    assert!(app.table_overlay);
    let screen = render(&app, 120, 35);
    assert!(screen.contains("customer_id") && screen.contains("fk_customer"));
    app.run(Action::Maintenance);
    assert!(!app.table_overlay && app.dialog.is_some());
    let dialog = app.dialog.as_mut().unwrap();
    dialog.fields[1].value = Zeroizing::new("check".into());
    dialog.focus = dialog.fields.len() + 1;
    key(&mut app, K::Enter);
    assert!(app.confirm.is_some());
    let screen = render(&app, 120, 35);
    assert!(
        screen.contains("CHECK TABLE `commerce`.`order_items (demo)`"),
        "SQL preview"
    );
    // Invalid input stays in the form instead of opening a confirmation.
    key(&mut app, K::Esc);
    let dialog = app.dialog.as_mut().unwrap();
    dialog.fields[1].value = Zeroizing::new("VACUUM".into());
    dialog.focus = dialog.fields.len() + 1;
    key(&mut app, K::Enter);
    assert!(app.confirm.is_none());
    assert!(app.status_line().contains("ANALYZE"));
}
#[test]
fn dropping_a_database_needs_its_typed_name() {
    let mut app = demo();
    app.run(Action::Page(DATABASES));
    app.selected = app
        .databases()
        .iter()
        .position(|d| d.name == "identity")
        .unwrap();
    app.run(Action::DropDatabase);
    key(&mut app, K::Tab); // "Provést změnu"
    key(&mut app, K::Enter);
    assert!(app.confirm.is_some(), "not without the name");
    assert!(app.status_line().contains("opište"));
    type_text(&mut app, "identit");
    key(&mut app, K::Enter);
    assert!(app.confirm.is_some(), "a prefix is not enough");
    type_text(&mut app, "y");
    key(&mut app, K::Enter);
    assert!(app.confirm.is_none());
    settle(&mut app);
    assert!(!app.databases().iter().any(|d| d.name == "identity"));
}
#[test]
fn privilege_picker_toggles_and_reviews_the_difference() {
    let mut app = demo();
    app.run(Action::Page(ACCOUNTS));
    key(&mut app, K::Enter);
    settle(&mut app);
    app.run(Action::EditPrivileges);
    let picker = app.picker.as_ref().expect("picker");
    assert_eq!(picker.fixed, Some(GrantScope::Database("analytics".into())));
    assert!(picker.wanted.contains("SELECT") && picker.wanted.contains("SHOW VIEW"));
    let insert = picker
        .options()
        .iter()
        .position(|p| *p == "INSERT")
        .unwrap()
        + 1;
    for _ in 0..insert {
        key(&mut app, K::Down);
    }
    key(&mut app, K::Char(' '));
    assert!(app.picker.as_ref().unwrap().wanted.contains("INSERT"));
    // →: the next existing scope reloads the checkboxes from the server state.
    app.picker.as_mut().unwrap().cursor = 0;
    key(&mut app, K::Right);
    assert_eq!(
        app.picker.as_ref().unwrap().fixed,
        Some(GrantScope::Table("commerce".into(), "orders".into()))
    );
    key(&mut app, K::Left);
    let review = app.picker.as_ref().unwrap().options().len() + 2;
    app.picker.as_mut().unwrap().cursor = review;
    app.picker.as_mut().unwrap().wanted.insert("INSERT".into());
    key(&mut app, K::Enter);
    let Some(Confirmation {
        action: Pending::Server(Change::Privileges { grant, revoke, .. }),
        ..
    }) = &app.confirm
    else {
        panic!("privileges confirmation");
    };
    assert_eq!(
        (grant.as_slice(), revoke.len()),
        (["INSERT".to_string()].as_slice(), 0)
    );
    assert!(render(&app, 120, 35).contains("GRANT INSERT ON `analytics`.*"));
    // Typing a new scope replaces the fixed one.
    app.confirm = None;
    app.picker.as_mut().unwrap().cursor = 0;
    key(&mut app, K::Backspace);
    assert_eq!(app.picker.as_ref().unwrap().scope_text, "analytics.");
    type_text(&mut app, "x");
    assert!(app.picker.as_ref().unwrap().scope().is_ok());
}
#[test]
fn variables_load_on_first_visit_and_values_are_validated_in_the_form() {
    let mut app = demo();
    key(&mut app, K::Char('5'));
    settle(&mut app);
    assert!(app.session.variables.is_some());
    assert!(app
        .variables()
        .iter()
        .all(|v| v.key_description().is_some()));
    app.selected = app
        .variables()
        .iter()
        .position(|v| v.name == "max_connections")
        .unwrap();
    assert!(render(&app, 120, 35).contains("Maximální počet souběžných spojení"));
    app.run(Action::SetVariable);
    let dialog = app.dialog.as_mut().unwrap();
    dialog.fields[0].value = Zeroizing::new("5".into());
    dialog.focus = dialog.fields.len() + 1;
    key(&mut app, K::Enter);
    assert!(app.confirm.is_none(), "below the minimum");
    let dialog = app.dialog.as_mut().unwrap();
    dialog.fields[0].value = Zeroizing::new("300".into());
    key(&mut app, K::Enter);
    assert!(render(&app, 120, 35).contains("SET GLOBAL max_connections = 300"));
    key(&mut app, K::Tab);
    key(&mut app, K::Enter);
    settle(&mut app);
    assert_eq!(app.session.snapshot.as_ref().unwrap().max_connections, 300);
    key(&mut app, K::Char('6'));
    settle(&mut app);
    assert!(app
        .findings()
        .iter()
        .any(|f| f.title == "Tabulky bez primárního klíče"));
}
