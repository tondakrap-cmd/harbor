use harbor_core::*;

#[tokio::test]
#[ignore = "vyžaduje izolovaný TLS kontejner a explicitní testovací port"]
async fn tls_identity_ca_and_explicit_insecure_mode() {
    let port: u16 = std::env::var("HARBOR_TEST_TLS_PORT")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        [33316, 33317].contains(&port),
        "Only dedicated local test ports"
    );
    let ca = std::fs::canonicalize(std::env::var("HARBOR_TEST_CA").unwrap()).unwrap();
    assert!(ca.to_str().unwrap().contains("/.test-data/"));
    let profile = Profile {
        name: "Izolovaný TLS test".into(),
        user: "root".into(),
        transport: Transport::Tcp {
            host: "localhost".into(),
            port,
        },
        tls: TlsMode::VerifyIdentity,
        ca: Some(ca.to_str().unwrap().into()),
        ..Default::default()
    };
    let mut backend = Backend::connect(&profile, Secret::default()).await.unwrap();
    let snapshot = backend.snapshot().await.unwrap();
    assert!(!snapshot.demo);
    // A successful TLS connection alone is not enough: verify wrong identity and CA fail closed.
    let wrong_host = Profile {
        transport: Transport::Tcp {
            host: "127.0.0.1".into(),
            port,
        },
        ..profile.clone()
    };
    assert!(matches!(
        Backend::connect(&wrong_host, Secret::default()).await,
        Err(Error::Tls)
    ));
    let wrong_ca = Profile {
        ca: Some(ca.with_file_name("wrong-ca.pem").to_str().unwrap().into()),
        ..profile.clone()
    };
    assert!(matches!(
        Backend::connect(&wrong_ca, Secret::default()).await,
        Err(Error::Tls)
    ));
    assert!(matches!(
        Backend::connect(&profile, Secret::new("incorrect".into())).await,
        Err(Error::Authentication)
    ));
    // Without an explicit CA the system trust store is used (rustls-native-certs honors
    // SSL_CERT_FILE, which stands in for update-ca-trust / update-ca-certificates here).
    let system_store = Profile {
        ca: None,
        ..profile.clone()
    };
    assert!(matches!(
        Backend::connect(&system_store, Secret::default()).await,
        Err(Error::Tls)
    ));
    std::env::set_var("SSL_CERT_FILE", &ca);
    let trusted = Backend::connect(&system_store, Secret::default()).await;
    std::env::remove_var("SSL_CERT_FILE");
    trusted.expect("CA from the system trust store is accepted");
    let insecure = Profile {
        tls: TlsMode::InsecureTestOnly,
        ca: None,
        ..profile
    };
    Backend::connect(&insecure, Secret::default())
        .await
        .unwrap();
}
