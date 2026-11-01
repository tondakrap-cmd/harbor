//! Host access over SSH against the isolated systemd test container
//! (`bash scripts/test-host.sh`); never run against a real server.
use harbor_core::*;
use std::time::Duration;

fn settings() -> HostSettings {
    let port: u16 = std::env::var("HARBOR_TEST_SSH_PORT")
        .expect("explicit test SSH port")
        .parse()
        .unwrap();
    assert!(
        std::env::var("HARBOR_SSH_IDENTITY").is_ok_and(|p| p.contains("/.test-data/")),
        "only the project's test key is allowed"
    );
    HostSettings {
        access: HostAccess::Ssh {
            target: "harbor@127.0.0.1".into(),
            port,
        },
        elevation: Elevation::Sudo,
        ..Default::default()
    }
}
fn wait(worker: &mut HostWorker) -> Result<HostResponse> {
    let start = std::time::Instant::now();
    loop {
        if let Some(result) = worker.poll() {
            return result;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "host worker timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn ask(worker: &mut HostWorker, request: HostRequest) -> Result<HostResponse> {
    assert!(worker.send(request));
    wait(worker)
}
async fn sql_value(query: &str) -> Result<String> {
    let profile = Profile {
        name: "host test".into(),
        user: "harbor".into(),
        transport: Transport::Tcp {
            host: "127.0.0.1".into(),
            port: std::env::var("HARBOR_TEST_HOST_DB_PORT")
                .unwrap()
                .parse()
                .unwrap(),
        },
        tls: TlsMode::InsecureTestOnly,
        ..Default::default()
    };
    let mut backend = Backend::connect(&profile, Secret::new("host-test-heslo".into())).await?;
    let _ = query;
    let variables = backend.variables().await?;
    let snapshot = backend.snapshot().await?;
    Ok(format!(
        "{}|{}|{}",
        variables
            .iter()
            .find(|v| v.name == "max_connections")
            .map(|v| v.value.clone())
            .unwrap_or_default(),
        snapshot.datadir,
        snapshot.log_error
    ))
}
#[tokio::test]
#[ignore = "vyžaduje izolovaný kontejner se systemd a SSH (scripts/test-host.sh)"]
async fn ssh_host_service_logs_and_config() {
    let mut worker = HostWorker::new(settings(), false);
    // Clean state: no Harbor option file.
    if let Ok(HostResponse::Config(config)) = ask(&mut worker, HostRequest::ReadConfig) {
        if config.exists {
            ask(
                &mut worker,
                HostRequest::WriteConfig {
                    entries: vec![],
                    profile: "test".into(),
                },
            )
            .unwrap();
        }
    }
    let info = sql_value("").await.unwrap();
    let parts: Vec<&str> = info.split('|').collect();
    let (datadir, log_error) = (parts[1].to_string(), parts[2].to_string());
    let Ok(HostResponse::Status(status)) = ask(
        &mut worker,
        HostRequest::Status {
            datadir: datadir.clone(),
        },
    ) else {
        panic!("status")
    };
    assert!(status.running(), "{status:?}");
    assert!(status.pid > 0);
    assert!(
        status.disk.as_ref().is_some_and(|d| d.total_kib > 0),
        "df of {datadir}"
    );
    let Ok(HostResponse::Logs(journal)) = ask(
        &mut worker,
        HostRequest::Logs {
            source: LogSource::Journal,
            lines: 50,
        },
    ) else {
        panic!("journal")
    };
    assert!(journal.iter().any(|l| l.contains("MariaDB")), "{journal:?}");
    if !log_error.is_empty() {
        let logs = ask(
            &mut worker,
            HostRequest::Logs {
                source: LogSource::File(log_error.clone()),
                lines: 20,
            },
        );
        assert!(logs.is_ok(), "{log_error}: {:?}", logs.err());
    }
    // An unknown option is refused by the server binary and nothing is written.
    let refused = ask(
        &mut worker,
        HostRequest::WriteConfig {
            entries: vec![("max_conections".into(), "300".into())],
            profile: "test".into(),
        },
    );
    assert!(refused.is_err(), "unknown option must be refused");
    let Ok(HostResponse::Config(config)) = ask(&mut worker, HostRequest::ReadConfig) else {
        panic!()
    };
    assert!(config.entries.is_empty());
    // A valid value is written and becomes effective after a restart.
    let Ok(HostResponse::ConfigWritten(config)) = ask(
        &mut worker,
        HostRequest::WriteConfig {
            entries: vec![("max_connections".into(), "321".into())],
            profile: "test".into(),
        },
    ) else {
        panic!("write")
    };
    assert!(config.path.ends_with("/zz-harbor.cnf"));
    assert_eq!(
        config.entries,
        vec![("max_connections".to_string(), "321".to_string())]
    );
    let before = status.pid;
    assert!(matches!(
        ask(&mut worker, HostRequest::Control(ServiceAction::Restart)),
        Ok(HostResponse::Controlled(_))
    ));
    let Ok(HostResponse::Status(after)) = ask(
        &mut worker,
        HostRequest::Status {
            datadir: datadir.clone(),
        },
    ) else {
        panic!()
    };
    assert!(after.running() && after.pid != before);
    assert!(sql_value("").await.unwrap().starts_with("321|"));
    // Stop and start.
    ask(&mut worker, HostRequest::Control(ServiceAction::Stop)).unwrap();
    let Ok(HostResponse::Status(stopped)) = ask(
        &mut worker,
        HostRequest::Status {
            datadir: datadir.clone(),
        },
    ) else {
        panic!()
    };
    assert!(!stopped.running());
    assert!(sql_value("").await.is_err(), "server is down");
    ask(&mut worker, HostRequest::Control(ServiceAction::Start)).unwrap();
    // A second write keeps a backup of the previous file; removing all entries
    // leaves an empty Harbor file (server defaults apply again).
    ask(
        &mut worker,
        HostRequest::WriteConfig {
            entries: vec![],
            profile: "test".into(),
        },
    )
    .unwrap();
    ask(&mut worker, HostRequest::Control(ServiceAction::Restart)).unwrap();
    assert!(sql_value("").await.unwrap().starts_with("151|"));
    // A wrong service name is reported clearly.
    let mut wrong = settings();
    wrong.service = "neexistuje".into();
    let mut worker = HostWorker::new(wrong, false);
    let error = ask(&mut worker, HostRequest::Status { datadir })
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("neexistuje"), "{error}");
}
