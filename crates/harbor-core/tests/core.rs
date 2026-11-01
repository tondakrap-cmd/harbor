use harbor_core::*;
use std::time::{Duration, Instant};

#[test]
fn identifiers_and_terminal_sequences_are_safe() {
    assert_eq!(
        identifier("x` ; DROP USER root").unwrap(),
        "`x`` ; DROP USER root`"
    );
    for bad in ["", "a.b", "a/b", "a\\b", "a\0b", "x "] {
        assert!(identifier(bad).is_err());
    }
    assert!(identifier(&"a".repeat(65)).is_err());
    assert_eq!(safe_text("Český\x1b[2J\r\n\u{202e}"), "Český�[2J���");
}
#[test]
fn rustls_certificate_errors_wrapped_by_sqlx_io_keep_the_tls_category() {
    let io = std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer),
    );
    assert!(matches!(Error::from(sqlx::Error::Io(io)), Error::Tls));
}
#[test]
fn rate_uses_elapsed_time_and_resets_after_restart() {
    let mut rate = Rate::default();
    let t = Instant::now();
    assert_eq!(rate.sample(100, 20, 7, t), None);
    assert_eq!(
        rate.sample(140, 22, 7, t + Duration::from_secs(2)),
        Some(20.0)
    );
    assert_eq!(rate.sample(2, 1, 7, t + Duration::from_secs(4)), None);
    assert_eq!(rate.sample(10, 2, 7, t + Duration::from_secs(5)), Some(8.0));
    // A reconnect (new server connection) restarts sampling even if counters grew.
    assert_eq!(rate.sample(500, 3, 8, t + Duration::from_secs(6)), None);
    assert_eq!(
        rate.sample(510, 4, 8, t + Duration::from_secs(7)),
        Some(10.0)
    );
}
#[test]
fn profile_storage_preserves_other_clients_changes_and_contains_no_password() {
    let directory = std::env::temp_dir().join(format!("harbor-config-test-{}", std::process::id()));
    let a = ConfigStore::at(directory.clone());
    let b = a.clone();
    a.update(|s| {
        s.profiles = vec![
            Profile::default(),
            Profile {
                name: "Druhý server".into(),
                ..Default::default()
            },
        ];
        s.desktop_theme = "terra".into();
    })
    .unwrap();
    b.update(|s| s.tui_theme = "nocturne".into()).unwrap();
    let loaded = a.load().unwrap();
    assert_eq!(loaded.desktop_theme, "terra");
    assert_eq!(loaded.tui_theme, "nocturne");
    assert_eq!(loaded.profiles.len(), 2);
    assert!(!std::fs::read_to_string(directory.join("settings.json"))
        .unwrap()
        .contains("password"));
    std::fs::write(directory.join("settings.json"), "broken").unwrap();
    assert!(a.update(|s| s.desktop_theme = "original".into()).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.join("settings.json")).unwrap(),
        "broken"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn sleep_is_not_a_long_query_and_confirmation_hides_password() {
    let p = Process {
        id: 1,
        query_id: 11,
        user: "u".into(),
        host: "h".into(),
        database: "d".into(),
        command: "Sleep".into(),
        seconds: 999,
        state: String::new(),
        query: String::new(),
    };
    assert!(!p.long_query());
    let prepared = Process {
        command: "Execute".into(),
        query: "SELECT SLEEP(60)".into(),
        ..p
    };
    assert!(prepared.long_query());
    let change = Change::CreateAccount {
        user: "u".into(),
        host: "h".into(),
        password: Secret::new("supersecret".into()),
    };
    assert!(!change.summary().contains("supersecret"));
}
#[tokio::test]
async fn demo_changes_use_the_shared_typed_backend() {
    let mut backend = Backend::demo();
    let initial = backend.snapshot().await.unwrap();
    backend
        .apply(Change::CreateDatabase {
            name: "test`db".into(),
            collation: "utf8mb4_unicode_ci".into(),
        })
        .await
        .unwrap();
    let s = backend.snapshot().await.unwrap();
    assert!(s.demo);
    assert_eq!(s.databases.len(), initial.databases.len() + 1);
    let target = s
        .processes
        .into_iter()
        .find(|p| p.command == "Query")
        .unwrap();
    backend.apply(Change::KillQuery { target }).await.unwrap();
    assert_eq!(backend.snapshot().await.unwrap().long_queries(), 0);
}
#[test]
fn switching_profiles_cannot_accept_old_worker_results() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    session.disconnect();
    let start = Instant::now();
    while session.worker.busy && start.elapsed() < Duration::from_secs(3) {
        session.tick(300);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!session.worker.busy);
    assert!(session.snapshot.is_none());
    assert_eq!(session.status, "Odpojeno");
}
fn wait_idle(session: &mut Session) {
    let start = Instant::now();
    session.tick(300);
    while session.worker.busy && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(5));
        session.tick(300);
    }
    assert!(!session.worker.busy, "worker did not answer");
}
#[test]
fn failed_write_does_not_mark_data_stale_or_reset_the_traffic_graph() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    wait_idle(&mut session);
    std::thread::sleep(Duration::from_millis(5));
    assert!(session.worker.send(Operation::Refresh));
    wait_idle(&mut session);
    assert_eq!(session.history.len(), 1);
    // The demo already has "commerce": a validation error from a write.
    assert!(session
        .worker
        .send(Operation::Apply(Change::CreateDatabase {
            name: "commerce".into(),
            collation: "utf8mb4_unicode_ci".into(),
        })));
    wait_idle(&mut session);
    assert!(session
        .error
        .as_deref()
        .unwrap()
        .contains("Databáze už existuje"));
    assert!(!session.stale);
    assert!(session.snapshot.is_some());
    assert!(session.qps.is_some());
    assert_eq!(session.history.len(), 1);
    std::thread::sleep(Duration::from_millis(5));
    assert!(session.worker.send(Operation::Refresh));
    wait_idle(&mut session);
    assert_eq!(session.history.len(), 2, "rate continued without a reset");
    // A refused write stays visible across periodic refreshes until the user acts again.
    assert!(session.error.is_some());
    assert!(session.request(Operation::Refresh));
    assert!(session.error.is_none());
    wait_idle(&mut session);
}
#[test]
fn reads_requested_while_busy_are_queued_not_dropped() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    assert!(session.worker.busy);
    assert!(
        !session.worker.send(Operation::Tables("commerce".into())),
        "raw send refuses"
    );
    assert!(session.request(Operation::Tables("commerce".into())));
    assert!(session.has_pending());
    // A later refresh does not replace the user's queued detail request.
    assert!(session.request(Operation::Refresh));
    let start = Instant::now();
    while (session.worker.busy || session.has_pending()) && start.elapsed() < Duration::from_secs(3)
    {
        std::thread::sleep(Duration::from_millis(5));
        session.tick(300);
    }
    assert!(!session.has_pending());
    assert!(session.snapshot.is_some());
    assert!(
        !session.tables().is_empty(),
        "queued Tables request was executed"
    );
}
#[test]
fn writes_are_never_queued_and_queued_reads_die_with_the_server_switch() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    assert!(!session.request(Operation::Apply(Change::CreateDatabase {
        name: "x".into(),
        collation: "utf8mb4_unicode_ci".into(),
    })));
    assert!(!session.has_pending());
    assert!(session.request(Operation::Tables("commerce".into())));
    assert!(session.has_pending());
    session.connect(Profile::default(), Secret::default(), true);
    assert!(!session.has_pending());
    wait_idle(&mut session);
    session.tick(300);
    assert!(!session.worker.busy);
    assert!(session.tables().is_empty());
}
#[test]
fn messages_carry_the_time_they_were_produced() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    wait_idle(&mut session);
    let before = session.error_at;
    std::thread::sleep(Duration::from_millis(2));
    assert!(session.request(Operation::Apply(Change::CreateDatabase {
        name: "commerce".into(),
        collation: "utf8mb4_unicode_ci".into(),
    })));
    wait_idle(&mut session);
    assert!(session.error.is_some());
    assert!(session.error_at > before);
}
#[test]
fn failed_refresh_still_marks_data_stale() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    wait_idle(&mut session);
    assert!(session.worker.send(Operation::Disconnect));
    wait_idle(&mut session);
    assert!(session.worker.send(Operation::Refresh));
    wait_idle(&mut session);
    assert!(session.stale);
    assert!(session.qps.is_none());
}
#[test]
fn server_switch_changes_the_worker_generation() {
    let mut session = Session::default();
    let before = session.worker.generation();
    session.connect(Profile::default(), Secret::default(), true);
    let connected = session.worker.generation();
    assert_ne!(before, connected);
    wait_idle(&mut session);
    assert!(session.worker.send(Operation::Refresh));
    assert_eq!(
        session.worker.generation(),
        connected,
        "reads keep the generation"
    );
    wait_idle(&mut session);
    session.disconnect();
    assert_ne!(session.worker.generation(), connected);
}
fn temp_store(tag: &str) -> (std::path::PathBuf, ConfigStore) {
    let directory =
        std::env::temp_dir().join(format!("harbor-config-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    (directory.clone(), ConfigStore::at(directory))
}
#[test]
fn renaming_onto_another_profile_name_is_rejected_and_nothing_is_written() {
    let (directory, store) = temp_store("rename");
    let (_, a) = store
        .try_update(|s| {
            s.upsert_profile(Profile {
                name: "A".into(),
                ..Default::default()
            })
        })
        .unwrap();
    let (_, b) = store
        .try_update(|s| {
            s.upsert_profile(Profile {
                name: "B".into(),
                ..Default::default()
            })
        })
        .unwrap();
    assert_ne!(a, b);
    let before = std::fs::read(directory.join("settings.json")).unwrap();
    let renamed = Profile {
        id: a.clone(),
        name: " b ".into(),
        ..Default::default()
    };
    assert!(matches!(
        store.try_update(|s| s.upsert_profile(renamed)),
        Err(Error::Validation(_))
    ));
    assert_eq!(
        std::fs::read(directory.join("settings.json")).unwrap(),
        before
    );
    // Renaming to a free name keeps the same profile (same id), no duplicate.
    let renamed = Profile {
        id: a.clone(),
        name: "A2".into(),
        ..Default::default()
    };
    let (settings, id) = store.try_update(|s| s.upsert_profile(renamed)).unwrap();
    assert_eq!(id, a);
    assert_eq!(settings.profiles.len(), 2);
    assert_eq!(settings.profile(&a).unwrap().name, "A2");
    assert_eq!(settings.profile(&b).unwrap().name, "B");
    // Removal is by id, so the edited name in a form cannot hit another profile.
    let (settings, removed) = store.try_update(|s| s.remove_profile(&b)).unwrap();
    assert_eq!(removed.name, "B");
    assert_eq!(settings.profiles.len(), 1);
    assert!(store.try_update(|s| s.remove_profile(&b)).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn legacy_profiles_without_id_get_a_stable_id_that_is_persisted() {
    let (directory, store) = temp_store("legacy");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("settings.json"),
        r#"{"profiles":[{"name":"Starý","transport":{"Tcp":{"host":"db","port":3306}},"user":"u","tls":"VerifyIdentity","ca":null}]}"#,
    )
    .unwrap();
    let first = store.load().unwrap().profiles[0].id.clone();
    assert!(!first.is_empty());
    assert_eq!(store.load().unwrap().profiles[0].id, first);
    // An edit made with the id from a load updates the profile instead of adding one.
    let mut edited = store.load().unwrap().profiles[0].clone();
    edited.user = "novy".into();
    let (settings, _) = store.try_update(|s| s.upsert_profile(edited)).unwrap();
    assert_eq!(settings.profiles.len(), 1);
    assert!(std::fs::read_to_string(directory.join("settings.json"))
        .unwrap()
        .contains(&first));
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn filters_are_case_insensitive_and_search_all_given_fields() {
    assert!(matches_filter("", &["cokoliv"]));
    assert!(matches_filter("  ", &["cokoliv"]));
    assert!(matches_filter("ČeS", &["xx", "česká_db"]));
    assert!(matches_filter("select", &["app", "SELECT 1"]));
    assert!(!matches_filter("zzz", &["app", "SELECT 1"]));
}
#[test]
fn driver_configuration_and_protocol_errors_have_their_own_messages() {
    let config = Error::from(sqlx::Error::Configuration("bad".into()));
    assert!(matches!(config, Error::Config(_)));
    let protocol = Error::from(sqlx::Error::Protocol("garbage".into()));
    assert!(matches!(protocol, Error::Protocol));
    assert!(protocol.to_string().contains("port"));
}
#[tokio::test]
async fn unreadable_or_non_pem_ca_is_reported_as_configuration() {
    let missing = Profile {
        ca: Some("/neexistuje/harbor-ca.pem".into()),
        ..Default::default()
    };
    let error = Backend::connect(&missing, Secret::default())
        .await
        .err()
        .unwrap();
    assert!(
        matches!(&error, Error::Config(m) if m.contains("nelze přečíst")),
        "{error}"
    );
    let path = std::env::temp_dir().join(format!("harbor-not-a-ca-{}", std::process::id()));
    std::fs::write(&path, "není certifikát").unwrap();
    let not_pem = Profile {
        ca: Some(path.to_str().unwrap().into()),
        ..Default::default()
    };
    let error = Backend::connect(&not_pem, Secret::default())
        .await
        .err()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        matches!(&error, Error::Config(m) if m.contains("PEM")),
        "{error}"
    );
}
#[test]
fn unknown_settings_and_profile_fields_survive_a_write() {
    let (directory, store) = temp_store("unknown");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("settings.json"),
        r#"{"future_option":{"a":1},"profiles":[{"id":"p1","name":"S","transport":{"Tcp":{"host":"db","port":3306}},"user":"u","tls":"VerifyIdentity","ca":null,"ssh_tunnel":"bastion"}]}"#,
    )
    .unwrap();
    // A form only knows today's fields: it saves a profile without `ssh_tunnel`.
    let edited = Profile {
        id: "p1".into(),
        name: "S2".into(),
        user: "u".into(),
        ..Default::default()
    };
    store.try_update(|s| s.upsert_profile(edited)).unwrap();
    store.update(|s| s.ascii = true).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("settings.json")).unwrap())
            .unwrap();
    assert_eq!(raw["future_option"]["a"], 1);
    assert_eq!(raw["profiles"][0]["ssh_tunnel"], "bastion");
    assert_eq!(raw["profiles"][0]["name"], "S2");
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn rapid_server_switches_are_never_refused() {
    let mut session = Session::default();
    for _ in 0..50 {
        session.connect(Profile::default(), Secret::default(), true);
        assert!(session.error.is_none(), "{:?}", session.error);
        session.disconnect();
        assert!(session.error.is_none(), "{:?}", session.error);
    }
    session.connect(Profile::default(), Secret::default(), true);
    wait_idle(&mut session);
    assert!(session.snapshot.is_some());
}
#[test]
fn open_details_reload_after_a_change_and_clear_on_disconnect() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    wait_idle(&mut session);
    let account = session.snapshot.as_ref().unwrap().accounts[0].clone();
    assert!(session.request(Operation::Grants(account.clone())));
    wait_idle(&mut session);
    assert_eq!(session.grants_for.as_ref(), Some(&account));
    session.account = Some(AccountDetail {
        account: Some(account.clone()),
        grants: vec!["stav před změnou".into()],
        ..Default::default()
    });
    assert!(session.request(Operation::Apply(Change::CreateAccount {
        user: "novy".into(),
        host: "localhost".into(),
        password: Secret::new("x".into()),
    })));
    let start = Instant::now();
    while (session.worker.busy || session.has_pending()) && start.elapsed() < Duration::from_secs(3)
    {
        std::thread::sleep(Duration::from_millis(5));
        session.tick(300);
    }
    assert_ne!(session.grants(), ["stav před změnou".to_string()]);
    assert!(session.grants()[0].contains(&account.label()));
    session.disconnect();
    assert!(session.grants().is_empty() && session.grants_for.is_none());
    assert!(session.tables().is_empty() && session.tables_for.is_none());
}
#[test]
fn wakeup_is_fast_only_while_work_is_in_flight() {
    let mut session = Session::default();
    assert_eq!(session.next_wakeup(5), Duration::from_secs(1));
    session.connect(Profile::default(), Secret::default(), true);
    assert_eq!(session.next_wakeup(5), Duration::from_millis(100));
    wait_idle(&mut session);
    let idle = session.next_wakeup(5);
    assert!(idle >= Duration::from_millis(100) && idle <= Duration::from_secs(1));
}
fn wait_all(session: &mut Session) {
    let start = Instant::now();
    session.tick(300);
    while (session.worker.busy || session.has_pending()) && start.elapsed() < Duration::from_secs(3)
    {
        std::thread::sleep(Duration::from_millis(5));
        session.tick(300);
    }
}
#[test]
fn a_late_detail_for_a_database_the_user_left_is_not_shown() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    wait_all(&mut session);
    assert!(session.request(Operation::Tables("commerce".into())));
    // The user moves on before the first answer arrives; only the newer one is shown.
    assert!(session.request(Operation::Tables("analytics".into())));
    wait_all(&mut session);
    assert_eq!(session.database.as_ref().unwrap().name, "analytics");
    assert!(session.request(Operation::Table {
        database: "analytics".into(),
        table: "orders (demo)".into(),
    }));
    wait_all(&mut session);
    assert!(session.table.is_some());
    // Opening another database closes the table detail of the previous one.
    assert!(session.request(Operation::Tables("commerce".into())));
    assert!(session.table.is_none() && session.table_for.is_none());
    wait_all(&mut session);
}
#[test]
fn reads_of_different_kinds_queue_together_and_a_change_reloads_open_details() {
    let mut session = Session::default();
    session.connect(Profile::default(), Secret::default(), true);
    assert!(session.request(Operation::Variables));
    assert!(session.request(Operation::Health));
    assert!(session.request(Operation::Tables("commerce".into())));
    wait_all(&mut session);
    assert!(session.variables.is_some() && session.health.is_some() && session.database.is_some());
    let variable = session
        .variables
        .as_ref()
        .unwrap()
        .iter()
        .find(|v| v.name == "max_connections")
        .unwrap()
        .clone();
    assert!(session.request(Operation::Apply(Change::SetGlobal {
        variable,
        value: "250".into(),
    })));
    wait_all(&mut session);
    assert_eq!(session.snapshot.as_ref().unwrap().max_connections, 250);
    let reloaded = session
        .variables
        .as_ref()
        .unwrap()
        .iter()
        .find(|v| v.name == "max_connections")
        .unwrap();
    assert_eq!(reloaded.value, "250", "variables reloaded after the change");
    // Maintenance output is kept for the UI.
    assert!(session.request(Operation::Apply(Change::Maintenance {
        database: "commerce".into(),
        table: "orders".into(),
        action: Maintenance::Analyze,
    })));
    wait_all(&mut session);
    assert!(session.change_output[0].contains("ANALYZE"));
    session.disconnect();
    assert!(session.variables.is_none() && session.health.is_none());
}
