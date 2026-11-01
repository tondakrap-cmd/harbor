//! Secret service storage against a private gnome-keyring in its own D-Bus session
//! (`bash scripts/test-keyring.sh`); never touches the user's real keyring.
use harbor_core::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "vyžaduje izolovanou relaci D-Bus s gnome-keyring (scripts/test-keyring.sh)"]
fn passwords_are_stored_loaded_and_used_for_connecting() {
    assert!(
        std::env::var("HARBOR_KEYRING_TEST").as_deref() == Ok("isolated"),
        "only inside scripts/test-keyring.sh"
    );
    let id = "keyring-test";
    secrets::delete_password(id).unwrap();
    assert!(secrets::load_password(id).unwrap().is_none());
    secrets::store_password(id, &Secret::new("první".into())).unwrap();
    secrets::store_password(id, &Secret::new("host-test-heslo".into())).unwrap();
    assert_eq!(
        secrets::load_password(id).unwrap().unwrap().0.as_str(),
        "host-test-heslo"
    );
    assert!(
        secrets::store_password("", &Secret::new("x".into())).is_err(),
        "unsaved profile"
    );
    let mut profile = Profile {
        id: id.into(),
        name: "keyring".into(),
        user: "harbor".into(),
        transport: Transport::Tcp {
            host: "127.0.0.1".into(),
            port: std::env::var("HARBOR_TEST_HOST_DB_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(33318),
        },
        tls: TlsMode::InsecureTestOnly,
        ..Default::default()
    };
    // Without the flag the stored password is not used.
    assert!(secrets::password_for(&profile, Secret::default())
        .unwrap()
        .0
        .is_empty());
    profile.remember_password = true;
    assert_eq!(
        secrets::password_for(&profile, Secret::default())
            .unwrap()
            .0
            .as_str(),
        "host-test-heslo"
    );
    assert_eq!(
        secrets::password_for(&profile, Secret::new("typed".into()))
            .unwrap()
            .0
            .as_str(),
        "typed",
        "a typed password wins"
    );
    // A session connects with an empty password field using the stored one.
    if std::env::var("HARBOR_TEST_HOST_DB_PORT").is_ok() {
        let mut session = Session::default();
        session.connect(profile.clone(), Secret::default(), false);
        let start = Instant::now();
        while session.worker.busy && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(10));
            session.tick(300);
        }
        assert!(session.snapshot.is_some(), "{:?}", session.error);
        // A typed password that works replaces the stored one.
        secrets::store_password(id, &Secret::new("stare".into())).unwrap();
        let mut session = Session::default();
        session.connect(
            profile.clone(),
            Secret::new("host-test-heslo".into()),
            false,
        );
        let start = Instant::now();
        while session.worker.busy && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(10));
            session.tick(300);
        }
        let snapshot = session.snapshot.as_ref().expect("connected");
        assert!(
            snapshot.notices[0].contains("uložené"),
            "{:?}",
            snapshot.notices
        );
        assert_eq!(
            secrets::load_password(id).unwrap().unwrap().0.as_str(),
            "host-test-heslo"
        );
    }
    secrets::delete_password(id).unwrap();
    secrets::delete_password(id).unwrap();
    assert!(secrets::load_password(id).unwrap().is_none());
}
