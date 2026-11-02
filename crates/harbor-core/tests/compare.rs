//! Comparison of two isolated instances (10.11 and 11.4) with deliberate differences.
use harbor_core::*;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlSslMode},
    Connection, MySqlConnection,
};
use std::time::{Duration, Instant};

fn socket(var: &str) -> String {
    let path = std::fs::canonicalize(std::env::var(var).expect("explicit test socket")).unwrap();
    let path = path.to_str().unwrap().to_string();
    assert!(path.contains("/.test-data/"), "only project test instances");
    path
}
async fn admin(path: &str) -> MySqlConnection {
    let options = MySqlConnectOptions::new()
        .socket(path)
        .username("root")
        .ssl_mode(MySqlSslMode::Disabled);
    let mut conn = MySqlConnection::connect_with(&options).await.unwrap();
    let marker: String =
        sqlx::query_scalar("SELECT marker FROM harbor_test_guard.instance LIMIT 1")
            .fetch_one(&mut conn)
            .await
            .expect("test sentinel");
    assert_eq!(marker, "isolated-harbor-integration");
    conn
}
fn profile(name: &str, path: &str) -> Profile {
    Profile {
        id: name.into(),
        name: name.into(),
        user: "root".into(),
        transport: Transport::Unix { path: path.into() },
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "vyžaduje dvě izolované instance (HARBOR_TEST_SOCKET_A a _B)"]
async fn two_servers_compare_variables_accounts_and_schema() {
    let (a, b) = (
        socket("HARBOR_TEST_SOCKET_A"),
        socket("HARBOR_TEST_SOCKET_B"),
    );
    let (mut left, mut right) = (admin(&a).await, admin(&b).await);
    for conn in [&mut left, &mut right] {
        sqlx::raw_sql(
            "DROP DATABASE IF EXISTS harbor_cmp; DROP USER IF EXISTS 'harbor_cmp_only'@'localhost'; \
             DROP USER IF EXISTS 'harbor_cmp_both'@'localhost'; DROP USER IF EXISTS 'harbor_cmp_limited'@'localhost'",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
    }
    sqlx::raw_sql("CREATE DATABASE harbor_cmp COLLATE utf8mb4_unicode_ci; CREATE TABLE harbor_cmp.t (id INT PRIMARY KEY, total DECIMAL(10,2) NOT NULL); CREATE TABLE harbor_cmp.same (id INT PRIMARY KEY)")
        .execute(&mut left)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE DATABASE harbor_cmp COLLATE utf8mb4_unicode_ci; CREATE TABLE harbor_cmp.t (id INT PRIMARY KEY, total DECIMAL(12,2) NOT NULL, note VARCHAR(20), KEY by_total (total)); CREATE TABLE harbor_cmp.same (id INT PRIMARY KEY); CREATE USER 'harbor_cmp_only'@'localhost'; GRANT SELECT ON harbor_cmp.* TO 'harbor_cmp_only'@'localhost'")
        .execute(&mut right)
        .await
        .unwrap();
    // Collation, generated column, index prefix and type, routine grant (issues #3–#5).
    let shape = |collation: &str, plus: u8, prefix: u8, fulltext: &str| {
        format!(
            "CREATE TABLE harbor_cmp.x (id INT PRIMARY KEY, name VARCHAR(40) COLLATE {collation}, \
             g INT AS (id+{plus}) VIRTUAL, UNIQUE KEY by_name (name({prefix})), {fulltext} ft (name)); \
             CREATE PROCEDURE harbor_cmp.p() SELECT 1; \
             CREATE USER 'harbor_cmp_both'@'localhost'; GRANT SELECT ON harbor_cmp.* TO 'harbor_cmp_both'@'localhost'"
        )
    };
    sqlx::raw_sql(&shape("utf8mb4_unicode_ci", 1, 10, "KEY"))
        .execute(&mut left)
        .await
        .unwrap();
    sqlx::raw_sql(&format!(
        "{}; GRANT EXECUTE ON PROCEDURE harbor_cmp.p TO 'harbor_cmp_both'@'localhost'; \
         CREATE USER 'harbor_cmp_limited'@'localhost'",
        shape("utf8mb4_bin", 2, 20, "FULLTEXT")
    ))
    .execute(&mut right)
    .await
    .unwrap();
    let mut session = Session::default();
    session.connect(profile("A", &a), Secret::default(), false);
    let wait = |session: &mut Session| {
        let start = Instant::now();
        while (session.worker.busy || session.compare.as_ref().is_some_and(|c| c.running()))
            && start.elapsed() < Duration::from_secs(60)
        {
            std::thread::sleep(Duration::from_millis(20));
            session.tick(300);
        }
        session.tick(300);
    };
    wait(&mut session);
    session
        .compare_with(profile("B", &b), Secret::default(), Areas::default())
        .unwrap();
    wait(&mut session);
    let result = session.compare.as_ref().unwrap().result().unwrap().unwrap();
    let find = |object: &str| result.differences.iter().find(|d| d.object == object);
    let total = find("harbor_cmp.t.total").expect("column type differs");
    assert_eq!(total.kind, DiffKind::Different);
    assert_eq!(
        (total.left.as_str(), total.right.as_str()),
        ("decimal(10,2) NOT NULL", "decimal(12,2) NOT NULL")
    );
    assert_eq!(find("harbor_cmp.t.note").unwrap().kind, DiffKind::OnlyRight);
    assert_eq!(
        find("harbor_cmp.t [index by_total]").unwrap().kind,
        DiffKind::OnlyRight
    );
    assert!(
        find("harbor_cmp.same").is_none(),
        "identical objects are not listed"
    );
    assert!(find("harbor_cmp.same.id").is_none());
    assert_eq!(
        find("harbor_cmp_only@localhost • existuje").unwrap().kind,
        DiffKind::OnlyRight
    );
    let grant = result
        .differences
        .iter()
        .find(|d| {
            d.object
                .starts_with("harbor_cmp_only@localhost • oprávnění harbor_cmp")
        })
        .expect("privileges of the extra account");
    assert_eq!(
        (grant.kind, grant.right.as_str()),
        (DiffKind::OnlyRight, "SELECT")
    );
    assert!(find("version").is_some(), "versions differ");
    assert!(
        find("server_id").is_none() && find("hostname").is_none(),
        "expected differences skipped"
    );
    assert!(result.ignored > 0);
    assert!(result.left.contains("A (") && result.right.contains("B ("));
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let differs = |object: &str| {
        let d = find(object).unwrap_or_else(|| panic!("{object} differs"));
        assert_eq!(d.kind, DiffKind::Different, "{d:?}");
        (d.left.clone(), d.right.clone())
    };
    let (l, r) = differs("harbor_cmp.x.name");
    assert!(
        l.contains("utf8mb4_unicode_ci") && r.contains("utf8mb4_bin"),
        "{l} / {r}"
    );
    let (l, r) = differs("harbor_cmp.x.g");
    assert!(l.contains("+ 1") || l.contains("+1"), "{l}");
    assert!(r.contains("+ 2") || r.contains("+2"), "{r}");
    let (l, r) = differs("harbor_cmp.x [index by_name]");
    assert!(
        l.contains("name(10)") && r.contains("name(20)"),
        "{l} / {r}"
    );
    let (l, r) = differs("harbor_cmp.x [index ft]");
    assert!(l.contains("BTREE") && r.contains("FULLTEXT"), "{l} / {r}");
    let routine = find("harbor_cmp_both@localhost • oprávnění PROCEDURE harbor_cmp.p")
        .expect("routine grant differs");
    assert_eq!(
        (routine.kind, routine.right.as_str()),
        (DiffKind::OnlyRight, "EXECUTE")
    );
    // An account that cannot read mysql.user sees only itself: the result says so (#2).
    let limited = Profile {
        user: "harbor_cmp_limited".into(),
        ..profile("B-limited", &b)
    };
    session
        .compare_with(
            limited,
            Secret::default(),
            Areas {
                variables: false,
                accounts: true,
                schema: false,
            },
        )
        .unwrap();
    wait(&mut session);
    let result = session.compare.as_ref().unwrap().result().unwrap().unwrap();
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert!(
        result.warnings[0].contains("vpravo"),
        "{:?}",
        result.warnings
    );
    for conn in [&mut left, &mut right] {
        sqlx::raw_sql(
            "DROP DATABASE harbor_cmp; DROP USER IF EXISTS 'harbor_cmp_only'@'localhost'; \
             DROP USER IF EXISTS 'harbor_cmp_both'@'localhost'; DROP USER IF EXISTS 'harbor_cmp_limited'@'localhost'",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
    }
}
