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
        .apply(Change::Privileges {
            account: account.clone(),
            scope: GrantScope::Database("harbor_test`db".into()),
            grant: vec!["SELECT".into()],
            revoke: vec![],
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
        .apply(Change::Privileges {
            account: account.clone(),
            scope: GrantScope::Database("harbor_test`db".into()),
            grant: vec![],
            revoke: vec!["SELECT".into()],
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

/// Admin connection to the isolated instance (sentinel verified) and a Harbor profile.
async fn isolated() -> (MySqlConnection, Profile) {
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
    let profile = Profile {
        name: "Izolovaný integrační test".into(),
        user: "root".into(),
        transport: Transport::Unix {
            path: socket.to_str().unwrap().into(),
        },
        ..Default::default()
    };
    (admin, profile)
}
async fn run(admin: &mut MySqlConnection, sql: &str) {
    sqlx::raw_sql(sql).execute(&mut *admin).await.unwrap();
}

#[tokio::test]
#[ignore = "vyžaduje izolovanou instanci a HARBOR_TEST_SOCKET"]
async fn isolated_mariadb_administration() {
    let (mut admin, profile) = isolated().await;
    run(&mut admin, "DROP USER IF EXISTS 'harbor_adm''in'@'localhost'; DROP ROLE IF EXISTS harbor_role; DROP DATABASE IF EXISTS harbor_adm_db; DROP DATABASE IF EXISTS harbor_drop_db").await;
    run(&mut admin, "CREATE DATABASE harbor_adm_db COLLATE utf8mb4_unicode_ci; \
        CREATE TABLE harbor_adm_db.customers (id INT PRIMARY KEY, name VARCHAR(40) COLLATE utf8mb4_bin COMMENT 'jméno') ENGINE=InnoDB; \
        CREATE TABLE harbor_adm_db.orders (id INT AUTO_INCREMENT PRIMARY KEY, customer_id INT NOT NULL, total DECIMAL(10,2) DEFAULT 0, \
          KEY by_customer (customer_id, total), CONSTRAINT fk_customer FOREIGN KEY (customer_id) REFERENCES harbor_adm_db.customers(id) ON DELETE CASCADE) ENGINE=InnoDB; \
        CREATE TABLE harbor_adm_db.legacy (a INT) ENGINE=MyISAM; \
        CREATE VIEW harbor_adm_db.big_orders AS SELECT * FROM harbor_adm_db.orders WHERE total > 100; \
        CREATE PROCEDURE harbor_adm_db.noop() SELECT 1; \
        CREATE TRIGGER harbor_adm_db.t_orders BEFORE INSERT ON harbor_adm_db.orders FOR EACH ROW SET NEW.total = NEW.total").await;
    let mut backend = Backend::connect(&profile, Secret::default()).await.unwrap();

    // --- database detail
    let detail = backend.database_detail("harbor_adm_db").await.unwrap();
    assert_eq!(detail.collation, "utf8mb4_unicode_ci");
    assert_eq!((detail.views, detail.routines, detail.triggers), (1, 1, 1));
    let table = |name: &str| detail.tables.iter().find(|t| t.name == name).unwrap();
    assert!(table("orders").primary_key && table("customers").primary_key);
    assert!(!table("legacy").primary_key);
    assert_eq!(table("legacy").engine, "MyISAM");
    assert!(table("big_orders").view());
    assert_eq!(table("orders").auto_increment, Some(1));
    let warnings = detail.warnings().join("\n");
    assert!(
        warnings.contains("bez primárního klíče") && warnings.contains("MyISAM"),
        "{warnings}"
    );
    assert!(backend.database_detail("neexistuje_xyz").await.is_err());

    // --- table detail
    let orders = backend
        .table_detail("harbor_adm_db", "orders")
        .await
        .unwrap();
    assert_eq!(
        orders
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "customer_id", "total"]
    );
    assert_eq!(orders.columns[0].key, "PRI");
    assert!(orders.columns[0].extra.contains("auto_increment"));
    assert_eq!(orders.indexes[0].name, "PRIMARY");
    let by_customer = orders
        .indexes
        .iter()
        .find(|i| i.name == "by_customer")
        .unwrap();
    assert_eq!(by_customer.columns, ["customer_id", "total"]);
    assert!(!by_customer.unique);
    assert_eq!(orders.foreign_keys.len(), 1);
    assert_eq!(
        orders.foreign_keys[0].referenced_table,
        "harbor_adm_db.customers"
    );
    assert_eq!(orders.foreign_keys[0].on_delete, "CASCADE");
    assert!(orders.create_sql.starts_with("CREATE TABLE `orders`"));
    let customers = backend
        .table_detail("harbor_adm_db", "customers")
        .await
        .unwrap();
    assert_eq!(customers.columns[1].collation, "utf8mb4_bin");
    assert_eq!(customers.columns[1].comment, "jméno");
    let view = backend
        .table_detail("harbor_adm_db", "big_orders")
        .await
        .unwrap();
    assert!(view.create_sql.contains("VIEW"), "{}", view.create_sql);

    // --- maintenance returns the server's messages
    let output = backend
        .apply(Change::Maintenance {
            database: "harbor_adm_db".into(),
            table: "orders".into(),
            action: Maintenance::Check,
        })
        .await
        .unwrap();
    assert!(
        output
            .iter()
            .any(|l| l.contains("status") && l.contains("OK")),
        "{output:?}"
    );
    let output = backend
        .apply(Change::Maintenance {
            database: "harbor_adm_db".into(),
            table: "legacy".into(),
            action: Maintenance::Repair,
        })
        .await
        .unwrap();
    assert!(!output.is_empty());

    // --- accounts, structured privileges, GRANT OPTION, roles, properties
    backend
        .apply(Change::CreateAccount {
            user: "harbor_adm'in".into(),
            host: "localhost".into(),
            password: Secret::new("x'y".into()),
        })
        .await
        .unwrap();
    let account = Account {
        user: "harbor_adm'in".into(),
        host: "localhost".into(),
        plugin: "mysql_native_password".into(),
    };
    let db_scope = GrantScope::Database("harbor_adm_db".into());
    backend
        .apply(Change::Privileges {
            account: account.clone(),
            scope: db_scope.clone(),
            grant: vec!["SELECT".into(), "INSERT".into(), GRANT_OPTION.into()],
            revoke: vec![],
        })
        .await
        .unwrap();
    backend
        .apply(Change::Privileges {
            account: account.clone(),
            scope: GrantScope::Table("harbor_adm_db".into(), "orders".into()),
            grant: vec!["UPDATE".into()],
            revoke: vec![],
        })
        .await
        .unwrap();
    backend
        .apply(Change::Privileges {
            account: account.clone(),
            scope: GrantScope::Global,
            grant: vec!["PROCESS".into()],
            revoke: vec![],
        })
        .await
        .unwrap();
    run(
        &mut admin,
        "GRANT SELECT (name) ON harbor_adm_db.customers TO 'harbor_adm''in'@'localhost'",
    )
    .await;
    let detail = backend.account_detail(&account).await.unwrap();
    let at = |scope: &GrantScope| detail.privileges_at(scope).cloned().unwrap();
    let db = at(&db_scope);
    assert!(db.grantable);
    assert_eq!(
        db.privileges.iter().cloned().collect::<Vec<_>>(),
        ["INSERT", "SELECT"]
    );
    assert!(
        at(&GrantScope::Table("harbor_adm_db".into(), "orders".into()))
            .privileges
            .contains("UPDATE")
    );
    assert!(at(&GrantScope::Global).privileges.contains("PROCESS"));
    assert_eq!(detail.columns, ["SELECT (name) na harbor_adm_db.customers"]);
    let properties = detail.properties.clone().unwrap();
    assert_eq!(properties.plugin, "mysql_native_password");
    assert!(!properties.locked && !properties.password_expired);
    // Diff-based edit: drop INSERT and the grant option, keep SELECT.
    let wanted: std::collections::BTreeSet<String> = ["SELECT".to_string()].into();
    let mut current = db.privileges.clone();
    current.insert(GRANT_OPTION.into());
    let (grant, revoke) = privilege_diff(&current, &wanted);
    backend
        .apply(Change::Privileges {
            account: account.clone(),
            scope: db_scope.clone(),
            grant,
            revoke,
        })
        .await
        .unwrap();
    let db = backend
        .account_detail(&account)
        .await
        .unwrap()
        .privileges_at(&db_scope)
        .cloned()
        .unwrap();
    assert!(!db.grantable);
    assert_eq!(
        db.privileges.iter().cloned().collect::<Vec<_>>(),
        ["SELECT"]
    );
    // A grant made with an unescaped wildcard is shown as a pattern and can be revoked.
    run(
        &mut admin,
        "GRANT SELECT ON `harbor_adm_%`.* TO 'harbor_adm''in'@'localhost'",
    )
    .await;
    let pattern = GrantScope::DatabasePattern("harbor_adm_%".into());
    assert!(backend
        .account_detail(&account)
        .await
        .unwrap()
        .privileges_at(&pattern)
        .is_some());
    backend
        .apply(Change::Privileges {
            account: account.clone(),
            scope: pattern.clone(),
            grant: vec![],
            revoke: vec!["SELECT".into()],
        })
        .await
        .unwrap();
    assert!(backend
        .account_detail(&account)
        .await
        .unwrap()
        .privileges_at(&pattern)
        .is_none());
    // Roles.
    backend
        .apply(Change::CreateRole {
            name: "harbor_role".into(),
        })
        .await
        .unwrap();
    let role = Account {
        user: "harbor_role".into(),
        host: String::new(),
        plugin: String::new(),
    };
    backend
        .apply(Change::Privileges {
            account: role.clone(),
            scope: db_scope.clone(),
            grant: vec!["SHOW VIEW".into()],
            revoke: vec![],
        })
        .await
        .unwrap();
    assert!(backend.snapshot().await.unwrap().accounts.contains(&role));
    assert!(backend
        .account_detail(&role)
        .await
        .unwrap()
        .privileges_at(&db_scope)
        .unwrap()
        .privileges
        .contains("SHOW VIEW"));
    backend
        .apply(Change::RoleMembership {
            role: "harbor_role".into(),
            account: account.clone(),
            grant: true,
        })
        .await
        .unwrap();
    backend
        .apply(Change::DefaultRole {
            account: account.clone(),
            role: Some("harbor_role".into()),
        })
        .await
        .unwrap();
    let detail = backend.account_detail(&account).await.unwrap();
    assert_eq!(detail.roles, ["harbor_role"]);
    assert_eq!(detail.default_role.as_deref(), Some("harbor_role"));
    // Login properties in one ALTER USER.
    backend
        .apply(Change::AlterAccount {
            account: account.clone(),
            locked: true,
            expire_password: true,
            tls: Some(TlsRequirement::Ssl),
            limits: Limits {
                queries_per_hour: 100,
                user_connections: 3,
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let properties = backend
        .account_detail(&account)
        .await
        .unwrap()
        .properties
        .unwrap();
    assert!(properties.locked && properties.password_expired);
    assert_eq!(properties.tls, TlsRequirement::Ssl);
    assert_eq!(properties.limits.queries_per_hour, 100);
    assert_eq!(properties.limits.user_connections, 3);
    assert!(
        Backend::connect(
            &Profile {
                user: account.user.clone(),
                ..profile.clone()
            },
            Secret::new("x'y".into())
        )
        .await
        .is_err(),
        "locked account cannot log in"
    );
    // Harbor refuses to drop or lock the account it is connected with.
    let me = Account {
        user: "root".into(),
        host: "localhost".into(),
        plugin: String::new(),
    };
    assert!(matches!(
        backend
            .apply(Change::DropAccount {
                account: me.clone()
            })
            .await,
        Err(Error::Validation(_))
    ));
    assert!(matches!(
        backend
            .apply(Change::AlterAccount {
                account: me,
                locked: true,
                expire_password: false,
                tls: None,
                limits: Limits::default(),
            })
            .await,
        Err(Error::Validation(_))
    ));
    backend
        .apply(Change::DropAccount {
            account: account.clone(),
        })
        .await
        .unwrap();
    backend
        .apply(Change::DropAccount {
            account: role.clone(),
        })
        .await
        .unwrap();
    let accounts = backend.snapshot().await.unwrap().accounts;
    assert!(!accounts.contains(&account) && !accounts.contains(&role));

    // --- variables
    let variables = backend.variables().await.unwrap();
    let max = variables
        .iter()
        .find(|v| v.name == "max_connections")
        .unwrap()
        .clone();
    assert!(max.settable() && max.min.is_some());
    assert!(variables
        .iter()
        .any(|v| v.name == "log_bin" && !v.settable()));
    assert!(
        variables
            .iter()
            .filter(|v| v.key_description().is_some())
            .count()
            >= 25
    );
    backend
        .apply(Change::SetGlobal {
            variable: max.clone(),
            value: "222".into(),
        })
        .await
        .unwrap();
    let changed = backend
        .variables()
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.name == "max_connections")
        .unwrap();
    assert_eq!(changed.value, "222");
    assert!(changed.runtime_only());
    assert!(
        backend
            .apply(Change::SetGlobal {
                variable: max.clone(),
                value: "5".into(),
            })
            .await
            .is_err(),
        "below the server's minimum"
    );
    let mut stale = max.clone();
    stale.var_type = "VARCHAR".into();
    assert!(
        backend
            .apply(Change::SetGlobal {
                variable: stale,
                value: "300".into(),
            })
            .await
            .is_err(),
        "metadata the user saw no longer matches"
    );
    run(
        &mut admin,
        &format!("SET GLOBAL max_connections = {}", max.value),
    )
    .await;

    // --- health: the test instance's root has no password
    let findings = backend.health().await.unwrap();
    let titles: Vec<&str> = findings.iter().map(|f| f.title.as_str()).collect();
    assert!(titles.contains(&"Účty bez hesla"), "{titles:?}");
    assert!(titles.contains(&"Tabulky bez primárního klíče"));
    assert!(findings
        .iter()
        .any(|f| f.detail.contains("harbor_adm_db.legacy")));

    // --- drop database
    run(&mut admin, "CREATE DATABASE harbor_drop_db").await;
    backend
        .apply(Change::DropDatabase {
            name: "harbor_drop_db".into(),
        })
        .await
        .unwrap();
    assert!(!backend
        .snapshot()
        .await
        .unwrap()
        .databases
        .iter()
        .any(|d| d.name == "harbor_drop_db"));
    run(&mut admin, "DROP DATABASE harbor_adm_db").await;
}

async fn table_checksum(admin: &mut MySqlConnection, db: &str, table: &str) -> Option<i64> {
    let row = sqlx::raw_sql(&format!("CHECKSUM TABLE `{db}`.`{table}` EXTENDED"))
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    use sqlx::Row;
    row.try_get::<Option<i64>, _>(1).unwrap()
}

#[tokio::test]
#[ignore = "vyžaduje izolovanou instanci a HARBOR_TEST_SOCKET"]
async fn isolated_mariadb_backup_and_restore() {
    let (mut admin, profile) = isolated().await;
    run(
        &mut admin,
        "DROP DATABASE IF EXISTS harbor_bk; DROP DATABASE IF EXISTS harbor_bk_copy",
    )
    .await;
    run(&mut admin, r#"CREATE DATABASE harbor_bk CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
        USE harbor_bk;
        CREATE TABLE customers (id INT PRIMARY KEY, name VARCHAR(80), note TEXT, avatar BLOB, flags BIT(5),
            price DECIMAL(12,4), ratio DOUBLE, born DATE, seen DATETIME(6), kind ENUM('a','b'), tags SET('x','y','z'),
            doc JSON, place POINT, upper_name VARCHAR(80) AS (UPPER(name)) VIRTUAL, secret INT INVISIBLE DEFAULT 7) ENGINE=InnoDB;
        INSERT INTO customers (id,name,note,avatar,flags,price,ratio,born,seen,kind,tags,doc,place,secret) VALUES
          (1,'Žluťoučký kůň','it''s "quoted"\\ back\\slash; semi\nnew line',0x00FF00DE,b'10101',1234.5678,1.0E-300,'2024-02-29','2026-11-01 12:34:56.789012','b','x,z','{"a": [1, "č"]}',ST_GeomFromText('POINT(14.42 50.08)'),99),
          (2,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,DEFAULT),
          (3,'',  '', '', b'0', -0.0001, -2.5, '1000-01-01', '1970-01-01 00:00:01', 'a', '', '[]', NULL, 0);
        CREATE TABLE orders (id INT AUTO_INCREMENT PRIMARY KEY, customer_id INT NOT NULL, total INT,
            FOREIGN KEY (customer_id) REFERENCES customers(id)) ENGINE=InnoDB;
        INSERT INTO orders (customer_id,total) VALUES (1,10),(1,20),(3,30);
        CREATE TABLE legacy (a INT, b VARCHAR(10)) ENGINE=MyISAM;
        INSERT INTO legacy VALUES (1,'x');
        CREATE TABLE latin (t VARCHAR(20)) CHARACTER SET latin1;
        INSERT INTO latin VALUES ('café'),('naïve');
        CREATE TABLE history (id INT PRIMARY KEY, v INT) WITH SYSTEM VERSIONING;
        INSERT INTO history VALUES (1,1); UPDATE history SET v=2 WHERE id=1;
        CREATE SEQUENCE ticket START WITH 100;
        DO NEXTVAL(ticket); DO NEXTVAL(ticket);
        "#).await;
    // a_big_orders sorts before the view it depends on.
    run(&mut admin, r#"USE harbor_bk;
        CREATE VIEW z_orders_view AS SELECT id, total FROM orders;
        CREATE VIEW a_big_orders AS SELECT * FROM z_orders_view WHERE total > 15;
        CREATE FUNCTION add_one(x INT) RETURNS INT DETERMINISTIC RETURN x + 1;
        CREATE PROCEDURE touch() BEGIN UPDATE orders SET total = total; SELECT 1; END;
        CREATE TRIGGER orders_bi BEFORE INSERT ON orders FOR EACH ROW SET NEW.total = COALESCE(NEW.total, 0);
        CREATE EVENT nightly ON SCHEDULE EVERY 1 DAY DISABLE DO DELETE FROM legacy WHERE a < 0;
        "#).await;
    let dir = std::env::temp_dir().join(format!("harbor-backup-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("bk.zip");
    let job = start_job(
        JobSpec::Backup(BackupRequest {
            profile: "test".into(),
            databases: vec!["harbor_bk".into()],
            path: path.clone(),
            encryption: None,
        }),
        Some((&profile, Secret::default())),
    )
    .unwrap();
    let summary = job.wait(|_| ()).unwrap();
    assert!(summary.contains("Netransakční"), "{summary}");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let manifest = read_manifest(&path).unwrap();
    assert_eq!(manifest.databases[0].rows, 3 + 3 + 1 + 2 + 1);
    assert!(
        !manifest.consistent
            && manifest.databases[0]
                .non_transactional
                .contains(&"legacy".to_string())
    );
    assert!(manifest.warnings.is_empty(), "{:?}", manifest.warnings);
    let verified = start_job(
        JobSpec::Verify {
            path: path.clone(),
            encryption: None,
        },
        None,
    )
    .unwrap()
    .wait(|_| ())
    .unwrap();
    assert!(verified.contains("v pořádku"), "{verified}");

    // Restore under another name and compare contents table by table.
    let restore = |target: &str, replace: bool| {
        start_job(
            JobSpec::Restore(RestoreRequest {
                path: path.clone(),
                encryption: None,
                database: "harbor_bk".into(),
                target: target.into(),
                replace,
            }),
            Some((&profile, Secret::default())),
        )
        .unwrap()
        .wait(|_| ())
    };
    let summary = restore("harbor_bk_copy", false).unwrap();
    assert!(summary.contains("harbor_bk_copy"), "{summary}");
    for table in ["customers", "orders", "legacy", "latin"] {
        assert_eq!(
            table_checksum(&mut admin, "harbor_bk", table).await,
            table_checksum(&mut admin, "harbor_bk_copy", table).await,
            "{table}"
        );
    }
    let invisible: (i64, String) =
        sqlx::query_as("SELECT secret, upper_name FROM harbor_bk_copy.customers WHERE id=1")
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!(invisible, (99, "ŽLUŤOUČKÝ KŮŇ".to_string()));
    let current: i64 = sqlx::query_scalar("SELECT v FROM harbor_bk_copy.history WHERE id=1")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(current, 2);
    let next: i64 = sqlx::query_scalar("SELECT NEXTVAL(harbor_bk_copy.ticket)")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert!(next >= 102, "sequence continues, got {next}");
    let big: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM harbor_bk_copy.a_big_orders")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(big, 2, "view depending on a later view was restored");
    let one: i32 = sqlx::query_scalar("SELECT harbor_bk_copy.add_one(1)")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(one, 2);
    let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA='harbor_bk_copy'), (SELECT COUNT(*) FROM information_schema.EVENTS WHERE EVENT_SCHEMA='harbor_bk_copy')")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(counts, (1, 1));
    let routines: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA='harbor_bk_copy'",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(routines, 2);
    // The original database was not touched by the renamed restore.
    assert!(table_checksum(&mut admin, "harbor_bk", "orders")
        .await
        .is_some());

    // Existing target: refused without replace, replaced on request.
    let refused = restore("harbor_bk_copy", false).unwrap_err().to_string();
    assert!(refused.contains("už existuje"), "{refused}");
    run(
        &mut admin,
        "INSERT INTO harbor_bk_copy.orders (customer_id,total) VALUES (1,99)",
    )
    .await;
    restore("harbor_bk_copy", true).unwrap();
    let orders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM harbor_bk_copy.orders")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(orders, 3, "replace drops rows added after the backup");

    // Encrypted backup: wrong or missing password is refused, the right one works.
    let secret_path = dir.join("secret.zip");
    start_job(
        JobSpec::Backup(BackupRequest {
            profile: "test".into(),
            databases: vec!["harbor_bk".into()],
            path: secret_path.clone(),
            encryption: Some(Secret::new("dlouhé heslo".into())),
        }),
        Some((&profile, Secret::default())),
    )
    .unwrap()
    .wait(|_| ())
    .unwrap();
    assert!(read_manifest(&secret_path).unwrap().encrypted);
    for wrong in [None, Some(Secret::new("špatné heslo".into()))] {
        assert!(start_job(
            JobSpec::Verify {
                path: secret_path.clone(),
                encryption: wrong
            },
            None
        )
        .unwrap()
        .wait(|_| ())
        .is_err());
    }
    assert!(start_job(
        JobSpec::Verify {
            path: secret_path.clone(),
            encryption: Some(Secret::new("dlouhé heslo".into()))
        },
        None
    )
    .unwrap()
    .wait(|_| ())
    .is_ok());
    // A plaintext search of the encrypted archive finds no row data.
    let raw = std::fs::read(&secret_path).unwrap();
    assert!(!raw.windows(5).any(|w| w == b"quoted"));

    // Cancelled backup leaves no file behind; existing files are never overwritten.
    let cancelled_path = dir.join("cancelled.zip");
    let job = start_job(
        JobSpec::Backup(BackupRequest {
            profile: "test".into(),
            databases: vec!["harbor_bk".into()],
            path: cancelled_path.clone(),
            encryption: None,
        }),
        Some((&profile, Secret::default())),
    )
    .unwrap();
    job.cancel();
    assert!(job.wait(|_| ()).is_err());
    assert!(!cancelled_path.exists());
    assert!(std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .all(|e| !e.path().to_string_lossy().ends_with(".part")));
    let existing = start_job(
        JobSpec::Backup(BackupRequest {
            profile: "test".into(),
            databases: vec!["harbor_bk".into()],
            path: path.clone(),
            encryption: None,
        }),
        Some((&profile, Secret::default())),
    )
    .unwrap()
    .wait(|_| ());
    assert!(existing.unwrap_err().to_string().contains("už existuje"));

    // A damaged archive fails verification and is not restored.
    let mut damaged = std::fs::read(&path).unwrap();
    let middle = damaged.len() / 3;
    damaged[middle] ^= 0xFF;
    let damaged_path = dir.join("damaged.zip");
    std::fs::write(&damaged_path, damaged).unwrap();
    assert!(start_job(
        JobSpec::Verify {
            path: damaged_path,
            encryption: None
        },
        None
    )
    .unwrap()
    .wait(|_| ())
    .is_err());

    // The listing reads manifests.
    let listed = list_backups(&dir);
    assert!(listed.iter().any(|b| b.path == path && b.manifest.is_ok()));
    run(
        &mut admin,
        "DROP DATABASE harbor_bk; DROP DATABASE harbor_bk_copy",
    )
    .await;
    std::fs::remove_dir_all(dir).unwrap();
}
