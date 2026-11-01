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
    assert_eq!(rate.sample(100, 20, t), None);
    assert_eq!(rate.sample(140, 22, t + Duration::from_secs(2)), Some(20.0));
    assert_eq!(rate.sample(2, 1, t + Duration::from_secs(4)), None);
    assert_eq!(rate.sample(10, 2, t + Duration::from_secs(5)), Some(8.0));
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
