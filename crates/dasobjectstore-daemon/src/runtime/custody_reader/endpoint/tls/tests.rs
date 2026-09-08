//! Real loopback TLS, exact journal and wire conformance; not a loaded-reader/platform fixture.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use dasobjectstore_object_service::custody_attestation::{
    CustodySignedRecordV1, CUSTODY_SIGNED_RECORD_SCHEMA_V1,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use std::{net::TcpListener, time::Duration};

struct Certificates {
    roots: RootCertStore,
    server: CertificateDer<'static>,
    server_key: PrivateKeyDer<'static>,
    client: CertificateDer<'static>,
    client_key: PrivateKeyDer<'static>,
    renewed: CertificateDer<'static>,
}
fn certificates() -> Certificates {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let key = KeyPair::generate().unwrap();
    let ca = params.self_signed(&key).unwrap();
    let issuer = Issuer::new(params, key);
    let mut roots = RootCertStore::empty();
    roots.add(ca.der().clone()).unwrap();
    let server_key = KeyPair::generate().unwrap();
    let server = CertificateParams::new(vec!["reader.test".into()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    let mut renewed = CertificateParams::new(vec!["reader.test".into()]).unwrap();
    renewed.serial_number = Some(999u64.into());
    let renewed = renewed.signed_by(&server_key, &issuer).unwrap();
    let client_key = KeyPair::generate().unwrap();
    let client = CertificateParams::new(vec!["verifier.test".into()])
        .unwrap()
        .signed_by(&client_key, &issuer)
        .unwrap();
    Certificates {
        roots,
        server: server.der().clone(),
        server_key: PrivateKeyDer::Pkcs8(server_key.serialize_der().into()),
        client: client.der().clone(),
        client_key: PrivateKeyDer::Pkcs8(client_key.serialize_der().into()),
        renewed: renewed.der().clone(),
    }
}
fn limits() -> CustodyReadLimits {
    CustodyReadLimits {
        maximum_bytes: 1024,
        timeout: Duration::from_secs(3),
    }
}
fn identity(
    cert: &CertificateDer<'static>,
    key: &PrivateKeyDer<'static>,
    roots: &RootCertStore,
) -> TlsIdentity {
    TlsIdentity {
        chain: vec![cert.clone()],
        key: key.clone_key(),
        peer_roots: roots.clone(),
    }
}
#[derive(Clone, Copy, Debug)]
enum Case {
    Good,
    WrongCa,
    WrongPin,
    Renewed,
    WrongName,
    NoClient,
    WrongClientPin,
    WrongClientCa,
    Alpn,
    Truncated,
    Extra,
    Unclean,
    Corrupt,
    Disconnect,
    Timeout,
    ExpiredAfterHandshake,
    BadSignature,
}
fn run(case: Case) {
    let certs = certificates();
    let foreign_client = certificates();
    let client_material = if matches!(case, Case::WrongClientCa) {
        &foreign_client
    } else {
        &certs
    };
    let (mut binding, seal, authority, mut selected) = super::super::tests::fixture();
    binding.frontend_tls_peer_sha256 = raw_sha256(certs.server.as_ref());
    binding.verifier_tls_identity_sha256 = raw_sha256(client_material.client.as_ref());
    let now = DateTime::<Utc>::from(std::time::SystemTime::now());
    selected.measurements.issued_at_utc =
        (now - chrono::Duration::seconds(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    selected.measurements.expires_at_utc = (now
        + chrono::Duration::seconds(if matches!(case, Case::ExpiredAfterHandshake) {
            2
        } else {
            60
        }))
    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let pinned: CustodyEd25519AuthorityV1 = serde_json::from_slice(&authority).unwrap();
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9; 32]).unwrap();
    let raw = serde_jcs::to_vec(&CustodySignedRecordV1 {
        schema: CUSTODY_SIGNED_RECORD_SCHEMA_V1.into(),
        body: selected.measurements.clone(),
        authority: pinned.clone(),
        signature_base64: STANDARD.encode(
            key.sign(&serde_jcs::to_vec(&selected.measurements).unwrap())
                .as_ref(),
        ),
    })
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut server_binding = binding.clone();
    let server_cert = if matches!(case, Case::Renewed) {
        server_binding.frontend_tls_peer_sha256 = raw_sha256(certs.renewed.as_ref());
        &certs.renewed
    } else {
        &certs.server
    };
    if matches!(case, Case::WrongClientPin) {
        server_binding.verifier_tls_identity_sha256 = "f".repeat(64);
    }
    let server = ServerTls::new(
        &server_binding,
        identity(server_cert, &certs.server_key, &certs.roots),
    )
    .unwrap();
    let mut client_binding = binding.clone();
    if matches!(case, Case::WrongPin) {
        client_binding.frontend_tls_peer_sha256 = "e".repeat(64);
    }
    let roots = if matches!(case, Case::WrongCa) {
        certificates().roots
    } else {
        certs.roots.clone()
    };
    let name = if matches!(case, Case::WrongName) {
        "other.test"
    } else {
        "reader.test"
    };
    let mut client = ExactObjectClient::new(
        &client_binding,
        &seal,
        identity(&client_material.client, &client_material.client_key, &roots),
        address,
        name.into(),
        "reader.test".into(),
        &authority,
        vec![SelectedRead {
            receipt_jcs: selected.receipt_jcs.clone(),
            measurements: selected.measurements.clone(),
        }],
    )
    .unwrap();
    if matches!(case, Case::Alpn) {
        Arc::get_mut(&mut client.config).unwrap().alpn_protocols = vec![b"h2".to_vec()];
    }
    if matches!(case, Case::NoClient) {
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_root_certificates(certs.roots.clone())
                .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        client.config = Arc::new(config);
    }
    let root = std::env::temp_dir().join(format!("das-tls-journal-{}", uuid::Uuid::new_v4()));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
    journal
        .issue_pre_read_request(&raw, &pinned, &clock_now())
        .unwrap();
    let receipt: CustodyIntegrityReceiptV1 = serde_json::from_slice(&selected.receipt_jcs).unwrap();
    let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let server_requests = requests.clone();
    let server_thread = std::thread::spawn(move || -> Result<(), ReaderError> {
        listener.set_nonblocking(true).unwrap();
        let accept_end = std::time::Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < accept_end =>
                {
                    std::thread::sleep(Duration::from_millis(1))
                }
                _ => return Err(ReaderError::Read),
            }
        };
        if matches!(case, Case::Timeout) {
            std::thread::sleep(Duration::from_millis(200));
        }
        if matches!(case, Case::ExpiredAfterHandshake) {
            std::thread::sleep(Duration::from_millis(2200));
        }
        let mut channel = server.accept(stream, limits().start().unwrap())?;
        let request = channel.collect(|h| http::request_body_length(h, "reader.test"), false)?;
        let parsed = http::decode_request(&request, "reader.test", &pinned, &clock_now())?;
        server_requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // This transport fixture returns selected synthetic bytes, not a pretend real backend.
        if matches!(case, Case::Disconnect) {
            return Err(ReaderError::Read);
        }
        let metadata = ReaderResultV1 {
            schema: "das.custody.reader_result.v1".into(),
            request_sha256: raw_sha256(parsed.raw_jcs),
            receipt_jcs_sha256: parsed.record.body.receipt_jcs_sha256,
            configuration_sha256: receipt.configuration_sha256,
            ledger_head_sha256: parsed.record.body.ledger_head_sha256,
            content_length: 3,
        };
        let mut response = http::encode_result(&metadata, b"abc", 1024)?;
        match case {
            Case::Truncated => {
                response.pop();
            }
            Case::Extra => response.push(0),
            Case::Corrupt => {
                *response.last_mut().unwrap() ^= 1;
            }
            _ => {}
        }
        channel
            .write_all(&response)
            .map_err(|_| ReaderError::Read)?;
        if !matches!(case, Case::Unclean) {
            channel.finish()?;
        }
        Ok(())
    });
    let mut budget = limits();
    if matches!(case, Case::Timeout) {
        budget.timeout = Duration::from_millis(100);
    }
    let result = if matches!(case, Case::BadSignature) {
        // Deliberately malicious peer sends an invalid envelope over genuinely authenticated TLS.
        // This is not the product client's journalled send path, which rejects it even earlier.
        let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        value["signature_base64"] = serde_json::json!(STANDARD.encode([0u8; 64]));
        let bad = serde_jcs::to_vec(&value).unwrap();
        let mut message = format!("POST /custody/v1/read-object HTTP/1.1\r\nHost: reader.test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bad.len()).into_bytes();
        message.extend_from_slice(&bad);
        let connection = ClientConnection::new(client.config.clone(), client.name.clone()).unwrap();
        let mut channel = Channel::authenticate(
            connection.into(),
            TcpStream::connect(address).unwrap(),
            budget.start().unwrap(),
            &client.server_leaf,
        )
        .unwrap();
        channel.write_all(&message).unwrap();
        assert!(channel
            .collect(|h| http::response_body_length(h, 1024), true)
            .is_err());
        Err(ObjectServiceError::InvalidConfiguration(
            "expected adversarial denial".into(),
        ))
    } else {
        client.read(&journal, &raw, budget)
    };
    let server_result = server_thread.join().unwrap();
    if matches!(case, Case::Good) {
        assert_eq!(result.unwrap().bytes, b"abc");
    } else {
        assert!(result.is_err(), "{case:?}");
    }
    if matches!(case, Case::Timeout | Case::ExpiredAfterHandshake) {
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
    if matches!(case, Case::BadSignature) {
        assert!(server_result.is_err());
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
        return;
    }
    assert!(
        client.read(&journal, &raw, limits()).is_err(),
        "no local replay {case:?}"
    );
    let reopened = CustodyOffNucJournal::open_existing(root.join("journal.sqlite")).unwrap();
    let mut called = false;
    assert!(reopened
        .perform_pre_read_exact(
            &raw,
            &serde_json::from_slice(&authority).unwrap(),
            &clock_now(),
            limits().start().unwrap(),
            |_, _| {
                called = true;
                Ok(())
            }
        )
        .is_err());
    assert!(!called, "no restart replay {case:?}");
}
#[test]
fn real_tls_exact_journal_roundtrip() {
    run(Case::Good);
}
#[test]
fn real_tls_invalid_ed25519_envelope_denies_before_signed_dispatch() {
    run(Case::BadSignature);
}
#[test]
fn real_tls_inherited_timeout_and_post_handshake_expiry_transmit_nothing() {
    run(Case::Timeout);
    run(Case::ExpiredAfterHandshake);
}
#[test]
fn real_tls_ca_pin_name_client_auth_and_alpn_denials() {
    for case in [
        Case::WrongCa,
        Case::WrongPin,
        Case::Renewed,
        Case::WrongName,
        Case::NoClient,
        Case::WrongClientPin,
        Case::WrongClientCa,
        Case::Alpn,
    ] {
        run(case);
    }
}
#[test]
fn real_tls_response_faults_and_disconnect_never_reissue() {
    for case in [
        Case::Truncated,
        Case::Extra,
        Case::Unclean,
        Case::Corrupt,
        Case::Disconnect,
    ] {
        run(case);
    }
}
