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
    let snapshot = backend.snapshot().await.unwrap();
    assert!(!snapshot.demo);
    assert!(snapshot.version.contains("MariaDB"));
    sqlx::query("DROP DATABASE IF EXISTS `harborXtest``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::query("CREATE DATABASE `harborXtest``db`")
        .execute(&mut admin)
        .await
        .unwrap();
    backend
        .apply(Change::CreateDatabase {
            name: "harbor_test`db".into(),
            collation: "utf8mb4_unicode_ci".into(),
        })
        .await
        .unwrap();
    assert!(backend
        .snapshot()
        .await
        .unwrap()
        .databases
        .iter()
        .any(|d| d.name == "harbor_test`db"));
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
    backend.apply(Change::KillQuery { target }).await.unwrap();
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
