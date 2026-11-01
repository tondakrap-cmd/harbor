//! Explicit opt-in; only a socket under a private test directory and a sentinel allow writes.
use harbor_core::*;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlSslMode},
    Connection, MySqlConnection,
};

#[tokio::test]
#[ignore = "vyžaduje izolovanou instanci a HARBOR_TEST_SOCKET"]
async fn isolated_mariadb_round_trip() {
    let socket = std::env::var("HARBOR_TEST_SOCKET").expect("explicit test socket");
    let socket = std::fs::canonicalize(socket).unwrap();
    assert!(
        socket.to_str().unwrap().contains("/.test-data/"),
        "Only project test instances are allowed"
    );
    let options = MySqlConnectOptions::new()
        .socket(&socket)
        .username("root")
        .ssl_mode(MySqlSslMode::Disabled);
    let mut admin = MySqlConnection::connect_with(&options).await.unwrap();
    let sentinel: String =
        sqlx::query_scalar("SELECT marker FROM harbor_test_guard.instance LIMIT 1")
            .fetch_one(&mut admin)
            .await
            .expect("test sentinel");
    assert_eq!(sentinel, "isolated-harbor-integration");
    // Reset only the test's own objects, after validating the isolated-instance sentinel.
    sqlx::query("DROP USER IF EXISTS 'harbor_test''user'@'localhost'")
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::query("DROP DATABASE IF EXISTS `harbor_test``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    let profile = Profile {
        // The neighboring database below would match an unescaped GRANT underscore.
        name: "Izolovaný integrační test".into(),
        user: "root".into(),
        transport: Transport::Unix {
            path: socket.to_str().unwrap().into(),
        },
        ..Default::default()
    };
    let mut backend = Backend::connect(&profile, Secret::default()).await.unwrap();
    backend.set_size_refresh_interval(std::time::Duration::from_secs(3600));
    let snapshot = backend.snapshot().await.unwrap();
    assert!(!snapshot.demo);
    assert!(snapshot.version.contains("MariaDB"));
    // Questions/s must not count Harbor's own monitoring statements: with no other
    // client active, the external counter does not move between two snapshots.
    let again = backend.snapshot().await.unwrap();
    assert_eq!(again.source, snapshot.source);
    assert_eq!(again.questions, snapshot.questions);
    // The overview must report the server's global time zone, not sqlx's session +00:00.
    let previous_zone: String = sqlx::query_scalar("SELECT @@global.time_zone")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    sqlx::query("SET GLOBAL time_zone = '+02:00'")
        .execute(&mut admin)
        .await
        .unwrap();
    let zone = backend.snapshot().await.unwrap().timezone;
    sqlx::query("SET GLOBAL time_zone = ?")
        .bind(&previous_zone)
        .execute(&mut admin)
        .await
        .unwrap();
    assert_eq!(zone, "+02:00");
    let system_zone: String = sqlx::query_scalar("SELECT @@system_time_zone")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    if previous_zone == "SYSTEM" {
        assert_eq!(
            backend.snapshot().await.unwrap().timezone,
            format!("SYSTEM ({system_zone})")
        );
    }
    sqlx::query("DROP DATABASE IF EXISTS `harborXtest``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::query("CREATE DATABASE `harborXtest``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    // Sizes/table counts are measured once per interval, not on every refresh...
    backend.set_size_refresh_interval(std::time::Duration::ZERO);
    backend.snapshot().await.unwrap();
    backend.set_size_refresh_interval(std::time::Duration::from_secs(3600));
    sqlx::query("CREATE TABLE `harborXtest``db`.`size_probe` (id INT)")
        .execute(&mut admin)
        .await
        .unwrap();
    let tables_of = |snapshot: &Snapshot, name: &str| {
        snapshot
            .databases
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.tables)
    };
    assert_eq!(
        tables_of(&backend.snapshot().await.unwrap(), "harborXtest`db"),
        Some(0),
        "cached measurement is reused within the interval"
    );
    backend
        .apply(Change::CreateDatabase {
            name: "harbor_test`db".into(),
            collation: "utf8mb4_unicode_ci".into(),
        })
        .await
        .unwrap();
    let after_write = backend.snapshot().await.unwrap();
    assert!(after_write
        .databases
        .iter()
        .any(|d| d.name == "harbor_test`db"));
    // ...but Harbor's own write invalidates the cache.
    assert_eq!(tables_of(&after_write, "harborXtest`db"), Some(1));
    sqlx::query("CREATE TABLE `harbor_test``db`.`česká_tabulka` (id INT PRIMARY KEY)")
        .execute(&mut admin)
        .await
        .unwrap();
    assert!(backend
        .tables("harbor_test`db")
        .await
        .unwrap()
        .iter()
        .any(|t| t.name == "česká_tabulka" && t.engine == "InnoDB"));
    backend
        .apply(Change::CreateAccount {
            user: "harbor_test'user".into(),
            host: "localhost".into(),
            password: Secret::new("test'p\\ass".into()),
        })
        .await
        .unwrap();
    let account = Account {
        user: "harbor_test'user".into(),
        host: "localhost".into(),
        plugin: "mysql_native_password".into(),
    };
    // Harbor sends only the client-side hash; the server must store exactly PASSWORD(plaintext).
    let same_hash: i64 = sqlx::query_scalar(
        "SELECT PASSWORD(?) = (SELECT authentication_string FROM mysql.user WHERE User=? AND Host=?)",
    )
    .bind("test'p\\ass")
    .bind(&account.user)
    .bind(&account.host)
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(same_hash, 1);
    Backend::connect(
        &Profile {
            user: account.user.clone(),
            ..profile.clone()
        },
        Secret::new("test'p\\ass".into()),
    )
    .await
    .unwrap();
    backend
        .apply(Change::DatabaseGrant {
            account: account.clone(),
            database: "harbor_test`db".into(),
            privilege: Privilege::Select,
            grant: true,
        })
        .await
        .unwrap();
    assert!(backend
        .grants(&account)
        .await
        .unwrap()
        .iter()
        .any(|g| g.contains("SELECT")));
    backend
        .apply(Change::ChangePassword {
            account: account.clone(),
            password: Secret::new("changed'p\\ass".into()),
        })
        .await
        .unwrap();
    let limited = Profile {
        user: account.user.clone(),
        ..profile.clone()
    };
    assert!(matches!(
        Backend::connect(&limited, Secret::new("wrong".into())).await,
        Err(Error::Authentication)
    ));
    let mut limited_backend = Backend::connect(&limited, Secret::new("changed'p\\ass".into()))
        .await
        .unwrap();
    let limited_snapshot = limited_backend.snapshot().await.unwrap();
    assert!(!limited_snapshot.notices.is_empty());
    assert!(limited_snapshot
        .databases
        .iter()
        .any(|d| d.name == "harbor_test`db"));
    assert!(!limited_snapshot
        .databases
        .iter()
        .any(|d| d.name == "harborXtest`db"));
    assert!(matches!(
        limited_backend
            .apply(Change::CreateDatabase {
                name: "harbor_denied".into(),
                collation: "utf8mb4_unicode_ci".into()
            })
            .await,
        Err(Error::Permission)
    ));
    backend
        .apply(Change::DatabaseGrant {
            account: account.clone(),
            database: "harbor_test`db".into(),
            privilege: Privilege::Select,
            grant: false,
        })
        .await
        .unwrap();
    let process_options = options.clone();
    let mut sleeper = MySqlConnection::connect_with(&process_options)
        .await
        .unwrap();
    let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
        .fetch_one(&mut sleeper)
        .await
        .unwrap();
    let task = tokio::spawn(async move {
        let result = sqlx::query("SELECT SLEEP(60)").execute(&mut sleeper).await;
        (result, sleeper)
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let target = backend
        .snapshot()
        .await
        .unwrap()
        .processes
        .into_iter()
        .find(|p| p.id == id && p.running_query())
        .unwrap();
    let mut changed_target = target.clone();
    changed_target.user = "jiný_účet".into();
    assert!(matches!(
        backend
            .apply(Change::KillQuery {
                target: changed_target
            })
            .await,
        Err(Error::Validation(_))
    ));
    let mut other_statement = target.clone();
    other_statement.query_id += 1;
    assert!(matches!(
        backend
            .apply(Change::KillQuery {
                target: other_statement
            })
            .await,
        Err(Error::Validation(_))
    ));
    assert_ne!(target.query_id, 0);
    backend
        .apply(Change::KillQuery {
            target: target.clone(),
        })
        .await
        .unwrap();
    // The same statement cannot be killed twice (it no longer runs).
    assert!(matches!(
        backend.apply(Change::KillQuery { target }).await,
        Err(Error::Validation(_))
    ));
    let (_, mut sleeper) = task.await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&mut sleeper)
            .await
            .unwrap(),
        1
    );
    sqlx::query("DROP USER 'harbor_test''user'@'localhost'")
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::query("DROP DATABASE `harbor_test``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::query("DROP DATABASE `harborXtest``db`")
        .execute(&mut admin)
        .await
        .unwrap();
}
