//! Replication, binary logs, locks, statement statistics and the console against the
//! isolated primary/replica pair (`bash scripts/test-replication.sh`).
use harbor_core::*;
use sqlx::{
    mysql::{MySqlConnectOptions, MySqlSslMode},
    Connection, MySqlConnection,
};
use std::time::Duration;

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
fn profile(path: &str) -> Profile {
    Profile {
        name: "replikace test".into(),
        user: "root".into(),
        transport: Transport::Unix { path: path.into() },
        ..Default::default()
    }
}
async fn run(conn: &mut MySqlConnection, sql: &str) {
    sqlx::raw_sql(sql).execute(&mut *conn).await.unwrap();
}
async fn wait_for<F: Fn(&Replication) -> bool>(backend: &Backend, ok: F) -> Replication {
    for _ in 0..50 {
        let state = backend.replication().await.unwrap();
        if ok(&state) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("replication did not reach the expected state");
}

#[tokio::test]
#[ignore = "vyžaduje izolovaný pár primár/replika (scripts/test-replication.sh)"]
async fn replication_binlogs_locks_statistics_and_console() {
    let primary_socket = socket("HARBOR_TEST_PRIMARY_SOCKET");
    let replica_socket = socket("HARBOR_TEST_REPLICA_SOCKET");
    let mut primary = admin(&primary_socket).await;
    let _replica_admin = admin(&replica_socket).await;
    let mut replica = Backend::connect(&profile(&replica_socket), Secret::default())
        .await
        .unwrap();
    let mut source = Backend::connect(&profile(&primary_socket), Secret::default())
        .await
        .unwrap();

    // --- replication state, stop/start
    let state = wait_for(&replica, |r| {
        r.channels.first().is_some_and(|c| c.healthy())
    })
    .await;
    assert!(state.read_only && state.server_id == 2);
    assert_eq!(state.channels[0].master, "harbor-repl-primary:3306");
    assert_eq!(state.channels[0].using_gtid, "Slave_Pos");
    assert!(state.channels[0].seconds_behind.is_some());
    replica
        .apply(Change::ReplicaControl {
            connection: String::new(),
            start: false,
        })
        .await
        .unwrap();
    let stopped = wait_for(&replica, |r| r.channels[0].io_running == "No").await;
    assert!(!stopped.channels[0].healthy());
    let findings = replica.health().await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.title == "Replikace neběží" && f.severity == Severity::Critical));
    replica
        .apply(Change::ReplicaControl {
            connection: String::new(),
            start: true,
        })
        .await
        .unwrap();
    wait_for(&replica, |r| r.channels[0].healthy()).await;
    // Data written on the primary arrives on the replica.
    run(&mut primary, "DROP DATABASE IF EXISTS harbor_repl; CREATE DATABASE harbor_repl; CREATE TABLE harbor_repl.t (id INT PRIMARY KEY, v INT) ENGINE=InnoDB; INSERT INTO harbor_repl.t VALUES (1,1),(2,2)").await;
    let mut arrived = false;
    for _ in 0..50 {
        if replica
            .database_detail("harbor_repl")
            .await
            .is_ok_and(|d| d.tables.len() == 1)
        {
            arrived = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(arrived, "replicated table");

    // --- primary: binary logs and purge
    let state = source.replication().await.unwrap();
    assert!(state.log_bin && state.channels.is_empty());
    assert!(
        state.replicas.iter().any(|r| r.contains("server_id 2")),
        "{:?}",
        state.replicas
    );
    run(&mut primary, "FLUSH BINARY LOGS; FLUSH BINARY LOGS").await;
    let logs = source.replication().await.unwrap().binlogs;
    assert!(logs.len() >= 3);
    let oldest = logs[0].name.clone();
    assert!(
        matches!(
            source
                .apply(Change::PurgeBinlogs { to: oldest.clone() })
                .await,
            Err(Error::Validation(_))
        ),
        "nothing older than the oldest"
    );
    assert!(matches!(
        source
            .apply(Change::PurgeBinlogs {
                to: "neexistuje.000001".into()
            })
            .await,
        Err(Error::Validation(_))
    ));
    let keep = logs[logs.len() - 1].name.clone();
    let output = source
        .apply(Change::PurgeBinlogs { to: keep.clone() })
        .await
        .unwrap();
    let after = source.replication().await.unwrap().binlogs;
    // Logs a connected replica still reads are kept, and the user is told so.
    match after.first().map(|l| l.name.clone()) {
        Some(first) if first == keep => assert!(output[0].contains("Smazány"), "{output:?}"),
        Some(first) if first == oldest => {
            assert!(output[0].contains("Nic nebylo smazáno"), "{output:?}")
        }
        _ => assert!(output[0].contains("čte připojená replika"), "{output:?}"),
    }
    // The replica keeps replicating after the purge (it had read the old logs).
    run(&mut primary, "INSERT INTO harbor_repl.t VALUES (3,3)").await;
    wait_for(&replica, |r| r.channels[0].healthy()).await;

    // --- locks: an idle transaction blocks an update
    let options = MySqlConnectOptions::new()
        .socket(&primary_socket)
        .username("root")
        .ssl_mode(MySqlSslMode::Disabled);
    let mut holder = MySqlConnection::connect_with(&options).await.unwrap();
    let holder_id: u64 = sqlx::query_scalar("SELECT CAST(CONNECTION_ID() AS UNSIGNED)")
        .fetch_one(&mut holder)
        .await
        .unwrap();
    run(
        &mut holder,
        "START TRANSACTION; UPDATE harbor_repl.t SET v = v + 1 WHERE id = 1",
    )
    .await;
    let mut waiter = MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 20")
        .execute(&mut waiter)
        .await
        .unwrap();
    let waiting = tokio::spawn(async move {
        let result = sqlx::query("UPDATE harbor_repl.t SET v = 0 WHERE id = 1")
            .execute(&mut waiter)
            .await;
        (result, waiter)
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let report = source.locks().await.unwrap();
    assert!(
        report.waits.iter().any(|w| w.blocking_thread == holder_id),
        "{:?}",
        report.waits
    );
    assert_eq!(report.root_blockers, vec![holder_id]);
    let blocker = report
        .transactions
        .iter()
        .find(|t| t.thread == holder_id)
        .unwrap();
    assert!(blocker.idle() && blocker.rows_locked >= 1);
    assert!(report.waits[0].lock_table.contains("harbor_repl"));
    // Kill the idle blocker: its transaction rolls back, the waiter proceeds.
    let target = source
        .snapshot()
        .await
        .unwrap()
        .processes
        .into_iter()
        .find(|p| p.id == holder_id)
        .unwrap();
    let mut wrong = target.clone();
    wrong.user = "jiny".into();
    assert!(matches!(
        source.apply(Change::KillConnection { target: wrong }).await,
        Err(Error::Validation(_))
    ));
    source
        .apply(Change::KillConnection { target })
        .await
        .unwrap();
    let (result, mut waiter) = waiting.await.unwrap();
    let waiter_id: u64 = sqlx::query_scalar("SELECT CAST(CONNECTION_ID() AS UNSIGNED)")
        .fetch_one(&mut waiter)
        .await
        .unwrap();
    assert_ne!(waiter_id, holder_id);
    assert!(
        result.is_ok(),
        "waiter continued after the blocker was killed"
    );
    // information_schema InnoDB tables are a cache refreshed at most every 100 ms.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let left = source.locks().await.unwrap();
    assert!(
        left.waits.is_empty(),
        "{:?} {:?}",
        left.waits,
        left.transactions
    );

    // --- statement statistics from mysql.slow_log
    run(&mut primary, "SET GLOBAL log_output = 'TABLE'; SET GLOBAL long_query_time = 0; SET GLOBAL slow_query_log = ON; TRUNCATE mysql.slow_log").await;
    let mut client = MySqlConnection::connect_with(&options).await.unwrap();
    for id in [1, 2, 3, 1] {
        sqlx::raw_sql(&format!("SELECT * FROM harbor_repl.t WHERE id = {id}"))
            .execute(&mut client)
            .await
            .unwrap();
    }
    let performance = source.performance().await.unwrap();
    assert_eq!(performance.source, Some(StatsSource::SlowLogTable));
    let stat = performance
        .statements
        .iter()
        .find(|s| s.query == "SELECT * FROM harbor_repl.t WHERE id = ?")
        .expect("grouped statement");
    assert_eq!(stat.count, 4);
    run(&mut primary, "SET GLOBAL slow_query_log = OFF; SET GLOBAL log_output = 'FILE'; SET GLOBAL long_query_time = 10").await;
    assert_eq!(
        source.performance().await.unwrap().source,
        Some(StatsSource::Off)
    );

    // --- console through the session (own connection)
    let mut session = Session::default();
    session.connect(profile(&primary_socket), Secret::default(), false);
    let wait_idle = |session: &mut Session| {
        let start = std::time::Instant::now();
        while (session.worker.busy || session.console.as_ref().is_some_and(|c| c.running()))
            && start.elapsed() < Duration::from_secs(20)
        {
            std::thread::sleep(Duration::from_millis(10));
            session.tick(300);
        }
        session.tick(300);
    };
    wait_idle(&mut session);
    assert_eq!(
        session
            .console("harbor_repl", "SELECT id, v FROM t ORDER BY id", false)
            .unwrap(),
        ConsoleKind::Read
    );
    wait_idle(&mut session);
    let result = session.console.as_ref().unwrap().result().unwrap().unwrap();
    assert_eq!(result.columns, ["id", "v"]);
    assert_eq!(result.rows.len(), 3);
    // Row limit: the connection is dropped after 1000 rows.
    session
        .console("harbor_repl", "SELECT seq FROM seq_1_to_100000", false)
        .unwrap();
    wait_idle(&mut session);
    let result = session.console.as_ref().unwrap().result().unwrap().unwrap();
    assert!(result.truncated && result.rows.len() == CONSOLE_ROW_LIMIT);
    // Server errors are shown as the server reports them (the user wrote the SQL).
    session
        .console("", "SELECT * FROM neexistuje", false)
        .unwrap();
    wait_idle(&mut session);
    let error = session
        .console
        .as_ref()
        .unwrap()
        .result()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("1046"), "{error}");
    // Locking reads are refused by the read-only transaction, with a clear message.
    session
        .console("harbor_repl", "SELECT * FROM t FOR UPDATE", false)
        .unwrap();
    wait_idle(&mut session);
    let refused = session
        .console
        .as_ref()
        .unwrap()
        .result()
        .unwrap()
        .unwrap_err();
    assert!(
        refused.contains("jen pro čtení") || refused.contains("READ ONLY"),
        "{refused}"
    );
    // A write is only classified until the UI confirms it.
    assert_eq!(
        session
            .console("harbor_repl", "DELETE FROM t WHERE id = 3", false)
            .unwrap(),
        ConsoleKind::Write
    );
    assert!(
        session
            .console
            .as_ref()
            .unwrap()
            .statement
            .starts_with("SELECT"),
        "nothing ran"
    );
    session
        .console("harbor_repl", "DELETE FROM t WHERE id = 3", true)
        .unwrap();
    wait_idle(&mut session);
    assert_eq!(
        session
            .console
            .as_ref()
            .unwrap()
            .result()
            .unwrap()
            .unwrap()
            .affected,
        Some(1)
    );
    // A long read can be cancelled.
    session.console("", "SELECT SLEEP(15)", false).unwrap();
    let start = std::time::Instant::now();
    while session
        .console
        .as_ref()
        .is_none_or(|c| c.statement != "SELECT SLEEP(15)")
        && start.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(10));
        session.tick(300);
    }
    std::thread::sleep(Duration::from_millis(800));
    session.console.as_ref().unwrap().cancel();
    wait_idle(&mut session);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "cancel stopped the query"
    );
    let cancelled = session.console.as_ref().unwrap().result().unwrap();
    // SLEEP() returns 1 when interrupted instead of failing.
    assert!(
        cancelled.is_err() || cancelled.as_ref().unwrap().rows[0][0].as_deref() == Some("1"),
        "{cancelled:?}"
    );
    // Read-only profiles refuse writes.
    let mut read_only = profile(&primary_socket);
    read_only.read_only = true;
    let mut guarded = Session::default();
    guarded.connect(read_only, Secret::default(), false);
    wait_idle(&mut guarded);
    assert!(guarded
        .console("harbor_repl", "DELETE FROM t", true)
        .is_err());
    assert!(!guarded.request(Operation::Apply(Change::CreateDatabase {
        name: "x".into(),
        collation: "utf8mb4_unicode_ci".into()
    })));
    assert!(guarded
        .error
        .as_deref()
        .unwrap_or("")
        .contains("jen pro čtení"));
    run(&mut primary, "DROP DATABASE harbor_repl").await;
}
