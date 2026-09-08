//! Joined guest-only loader/TLS/AWS protocol fixture, never real Garage evidence.
use super::super::endpoint::tls::{ExactObjectClient, ServerTls, TlsIdentity};
use super::super::endpoint::{ExactObjectServer, SelectedRead};
use super::*;
use chrono::{DateTime, SecondsFormat, Utc};
use dasobjectstore_object_service::custody_attestation::{
    CustodyEd25519AuthorityV1, CustodyOffNucJournal, CustodyOffNucPreReadRequestV1,
    CustodySignedRecordV1, CUSTODY_SIGNED_RECORD_SCHEMA_V1,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::KeyPair as _,
};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer},
    RootCertStore,
};
use std::{net::TcpListener, time::Instant};

const CONTROL: &str = "/run/das-systemd-vm-fixture";
const PUBLIC: &str = "/run/das-vm-tls-public";
const READER: &str = "/run/das-vm-reader-material";
const VERIFIER: &str = "/run/das-vm-verifier-material";
const BODY: &[u8] = b"actual synthetic manager receipt";
const BACKEND: &str = "http://127.0.0.1:19000";
const TLS_ADDRESS: &str = "127.0.0.1:19443";
const REPLAY_DONE: &str = "/run/das-vm-verifier-results/replay-checked";

fn limits() -> CustodyReadLimits {
    CustodyReadLimits {
        maximum_bytes: 4096,
        timeout: Duration::from_secs(120),
    }
}
fn mode() -> String {
    let value = fs::read_to_string(format!("{CONTROL}/mode")).unwrap();
    assert!(matches!(
        value.trim(),
        "positive" | "binding" | "corrupt" | "disconnect"
    ));
    value.trim().into()
}
fn public(name: &str, bytes: &[u8]) {
    let path = Path::new(PUBLIC).join(name);
    assert!(!path.exists());
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
}
fn private(directory: &str, name: &str, bytes: &[u8], uid: u32) {
    let path = Path::new(directory).join(name);
    assert!(!path.exists());
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: live NUL-terminated owned guest path, root-only guarded test setup.
    assert_eq!(unsafe { libc::chown(c.as_ptr(), uid, uid) }, 0);
}
fn now() -> String {
    wall_clock().to_rfc3339_opts(SecondsFormat::Secs, true)
}
fn wall_clock() -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::now())
}

#[test]
#[ignore = "root-only joined fixture preparation in separately approved disposable VM"]
fn prepare_joined_tls_vm() {
    loader_vm_tests::guest_guard(0);
    let _ = mode();
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let key = KeyPair::generate().unwrap();
    let ca = params.self_signed(&key).unwrap();
    let issuer = Issuer::new(params, key);
    public("ca.der", ca.der());
    let server_key = KeyPair::generate().unwrap();
    let server = CertificateParams::new(vec!["reader.test".into()])
        .unwrap()
        .signed_by(&server_key, &issuer)
        .unwrap();
    let client_key = KeyPair::generate().unwrap();
    let client = CertificateParams::new(vec!["verifier.test".into()])
        .unwrap()
        .signed_by(&client_key, &issuer)
        .unwrap();
    public("server.der", server.der());
    public("client.der", client.der());
    private(
        READER,
        "key.der",
        &zeroize::Zeroizing::new(server_key.serialize_der()),
        2000,
    );
    private(
        VERIFIER,
        "key.der",
        &zeroize::Zeroizing::new(client_key.serialize_der()),
        2001,
    );
    let mut seed = zeroize::Zeroizing::new([0u8; 32]);
    SystemRandom::new().fill(seed.as_mut()).unwrap();
    let signing = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed.as_ref()).unwrap();
    private(VERIFIER, "signing.seed", seed.as_ref(), 2001);
    let authority = CustodyEd25519AuthorityV1 {
        authority_id: "isolated-functional-verifier".into(),
        algorithm: "ed25519".into(),
        public_key_base64: STANDARD.encode(signing.public_key().as_ref()),
        public_key_sha256: raw_sha256(signing.public_key().as_ref()),
    };
    let authority = serde_jcs::to_vec(&authority).unwrap();
    public("authority.jcs", &authority);
    loader_vm_tests::prepare("positive", |f| {
        let connection = rusqlite::Connection::open_with_flags(
            &f.selection.ledger,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let receipt_jcs: String = connection
            .query_row(
                "SELECT receipt_jcs FROM custody_readback_receipts",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let receipt: CustodyIntegrityReceiptV1 = serde_json::from_str(&receipt_jcs).unwrap();
        assert_eq!(receipt.content_sha256, raw_sha256(BODY));
        f.selection.binding.service_identity = "das-vm-tls-reader.service".into();
        f.selection.binding.frontend_tls_peer_sha256 = raw_sha256(server.der());
        f.selection.binding.verifier_tls_identity_sha256 = raw_sha256(client.der());
        f.selection.binding.verifier_authority_sha256 = raw_sha256(&authority);
        f.selection.binding.object_lock_policy_sha256 = receipt.object_lock_policy_sha256.clone();
        f.selection.backend_endpoint = BACKEND.into();
        f.selection.aws_executable = Path::new("/usr/bin/aws").canonicalize().unwrap();
        // The guest installer independently verifies signed RPM closure and writes
        // this exact installed executable hash before fixture publication.
        let installed = fs::read_to_string(format!("{CONTROL}/aws-executable.sha256")).unwrap();
        f.selection.aws_executable_sha256 = installed.trim().into();
        let b = &f.selection.binding;
        let measurements = CustodyOffNucPreReadRequestV1 {
            schema: "dasobjectstore.local_trusted_administrator_custody_pre_read_request.v1".into(),
            assurance_class: "local_trusted_administrator_overlay".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            release_train: "r237".into(),
            release_stage: "s4".into(),
            purpose: "custody".into(),
            verifier_id: "isolated-functional-verifier".into(),
            target_id: receipt.target_id.clone(),
            machine_identity_sha256: b.host_identity_sha256.clone(),
            s3_endpoint_authority: BACKEND.into(),
            endpoint_authority_sha256: b.endpoint_authority_sha256.clone(),
            tls_peer_sha256: b.tls_peer_sha256.clone(),
            routing_sha256: "a".repeat(64),
            reader_identity: b.reader_identity.clone(),
            store_id: b.store_id.clone(),
            bucket_name: b.bucket_name.clone(),
            stores_namespace_sha256: b.stores_namespace_sha256.clone(),
            object_lock_policy_sha256: b.object_lock_policy_sha256.clone(),
            lock_ledger_sha256: raw_sha256(&fs::read(&f.selection.ledger).unwrap()),
            ledger_head_sha256: f.seal.ledger_head_sha256.clone(),
            inventory_sha256: b.inventory_sha256.clone(),
            lockset_sha256: "c".repeat(64),
            verifier_executable_sha256: b.executable_sha256.clone(),
            verifier_provenance_sha256: "e".repeat(64),
            receipt_jcs_sha256: raw_sha256(receipt_jcs.as_bytes()),
            nonce: uuid::Uuid::new_v4().to_string(),
            sequence: 1,
            previous_request_sha256: None,
            issued_at_utc: now(),
            expires_at_utc: (wall_clock() + chrono::Duration::seconds(120))
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        public("receipt.jcs", receipt_jcs.as_bytes());
        public(
            "measurements.jcs",
            &serde_jcs::to_vec(&measurements).unwrap(),
        );
        public("seal.jcs", &f.seal.encode().unwrap());
        public(
            "object-path",
            format!("/{}/{}", receipt.bucket_name, receipt.object_key).as_bytes(),
        );
    });
}

fn selected() -> SelectedRead {
    SelectedRead {
        receipt_jcs: fs::read(format!("{PUBLIC}/receipt.jcs")).unwrap(),
        measurements: serde_json::from_slice(
            &fs::read(format!("{PUBLIC}/measurements.jcs")).unwrap(),
        )
        .unwrap(),
    }
}
fn identity(reader: bool) -> TlsIdentity {
    let cert = if reader { "server.der" } else { "client.der" };
    let directory = if reader { READER } else { VERIFIER };
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(
            fs::read(format!("{PUBLIC}/ca.der")).unwrap(),
        ))
        .unwrap();
    TlsIdentity {
        chain: vec![CertificateDer::from(
            fs::read(format!("{PUBLIC}/{cert}")).unwrap(),
        )],
        key: PrivateKeyDer::Pkcs8(fs::read(format!("{directory}/key.der")).unwrap().into()),
        peer_roots: roots,
    }
}

#[test]
#[ignore = "actual encrypted systemd reader plus genuine TLS/AWS in reviewed disposable VM"]
fn serve_joined_tls_vm() {
    loader_vm_tests::guest_guard(2000);
    let selection = loader_vm_tests::selected_fixture();
    let mut tls_binding = selection.binding.clone();
    if mode() == "binding" {
        tls_binding.credential_generation += 1;
    }
    let tls = ServerTls::new(&tls_binding, identity(true)).unwrap();
    let runner = super::super::super::service::SystemServiceCommandRunner;
    let reader = ReaderContinuation::load(
        selection,
        &runner,
        Path::new("/run/das-vm-scratch"),
        limits(),
    )
    .unwrap();
    let authority = fs::read(format!("{PUBLIC}/authority.jcs")).unwrap();
    let mut server =
        ExactObjectServer::new(reader, "reader.test".into(), &authority, vec![selected()]).unwrap();
    let listener = TcpListener::bind(TLS_ADDRESS).unwrap();
    listener.set_nonblocking(true).unwrap();
    fs::write("/run/das-vm-results/ready", b"ready").unwrap();
    let end = Instant::now() + Duration::from_secs(120);
    let socket = loop {
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end =>
            {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => panic!("bounded joined fixture accept failed"),
        }
    };
    let result = server.serve_connection(socket, &tls, limits());
    if mode() == "positive" {
        assert!(result.is_ok());
    }
    // Application-level denials can be a successfully transmitted fixed403.
    if mode() == "binding" {
        assert!(result.is_err());
    }
    // Keep the real listening socket alive until the separate verifier has
    // completed replay checks. A second connection fails independently of GET
    // counting, so connection-refused cannot masquerade as journal denial.
    no_second_connection(&listener, Path::new(REPLAY_DONE), Duration::from_secs(15)).unwrap();
    fs::write("/run/das-vm-results/server-passed", b"passed").unwrap();
}

fn no_second_connection(
    listener: &TcpListener,
    done: &Path,
    budget: Duration,
) -> std::io::Result<()> {
    let end = Instant::now() + budget;
    loop {
        match listener.accept() {
            Ok(_) => return Err(std::io::Error::other("unexpected replay connection")),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
        match fs::symlink_metadata(done) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                // The verifier publishes completion after synchronous replay
                // calls return. Check again for a connection queued between the
                // first accept check and observing that completion.
                return match listener.accept() {
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
                    Err(error) => Err(error),
                    Ok(_) => Err(std::io::Error::other("unexpected replay connection")),
                };
            }
            Ok(_) => return Err(std::io::Error::other("invalid fixture completion marker")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= end {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "replay check not completed",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn joined_retry_observer_denies_queued_connection_even_with_done_marker() {
    let path = std::env::temp_dir().join(format!("das-replay-observer-{}", uuid::Uuid::new_v4()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let peer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    fs::write(&path, b"done").unwrap();
    let denied = no_second_connection(&listener, &path, Duration::from_secs(1));
    fs::remove_file(&path).unwrap();
    drop(peer);
    assert!(denied.is_err());
    let missing = no_second_connection(&listener, &path, Duration::from_millis(5));
    assert_eq!(missing.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    fs::write(&path, b"done").unwrap();
    let accepted = no_second_connection(&listener, &path, Duration::from_secs(1));
    fs::remove_file(path).unwrap();
    accepted.unwrap();
}

fn journal_snapshot(path: &Path, request_id: &str) -> (String, String, String) {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let count: u64 = db
        .query_row("SELECT count(*) FROM first_attempts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
    db.query_row("SELECT r.status,a.attempt_marker_sha256,a.result FROM issued_pre_read_requests r JOIN first_attempts a ON a.request_id=r.request_id WHERE r.request_id=?1", [request_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap()
}

#[test]
#[ignore = "separate guest UID verifier; functional isolation, not off-NUC provenance"]
fn verify_joined_tls_vm() {
    loader_vm_tests::guest_guard(2001);
    let selection = loader_vm_tests::selected_fixture();
    let authority_raw = fs::read(format!("{PUBLIC}/authority.jcs")).unwrap();
    let authority: CustodyEd25519AuthorityV1 = serde_json::from_slice(&authority_raw).unwrap();
    let seal = ReaderSealV1::decode(&fs::read(format!("{PUBLIC}/seal.jcs")).unwrap()).unwrap();
    let mut request = selected().measurements;
    request.issued_at_utc = now();
    request.expires_at_utc =
        (wall_clock() + chrono::Duration::seconds(120)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let seed = zeroize::Zeroizing::new(fs::read(format!("{VERIFIER}/signing.seed")).unwrap());
    let signing = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    let signature = STANDARD.encode(signing.sign(&serde_jcs::to_vec(&request).unwrap()).as_ref());
    let raw = serde_jcs::to_vec(&CustodySignedRecordV1 {
        schema: CUSTODY_SIGNED_RECORD_SCHEMA_V1.into(),
        body: request.clone(),
        authority: authority.clone(),
        signature_base64: signature,
    })
    .unwrap();
    let journal_path = Path::new(VERIFIER).join("journal.sqlite3");
    let journal = CustodyOffNucJournal::create(&journal_path).unwrap();
    journal
        .issue_pre_read_request(&raw, &authority, &now())
        .unwrap();
    let make_client = || {
        ExactObjectClient::new(
            &selection.binding,
            &seal,
            identity(false),
            TLS_ADDRESS.parse().unwrap(),
            "reader.test".into(),
            "reader.test".into(),
            &authority_raw,
            vec![selected()],
        )
        .unwrap()
    };
    let result = make_client().read(&journal, &raw, limits());
    if mode() == "positive" {
        assert_eq!(result.unwrap().bytes, BODY);
    } else {
        assert!(result.is_err());
    }
    let before = journal_snapshot(&journal_path, &request.request_id);
    assert!(matches!(before.0.as_str(), "started" | "terminal"));
    if mode() != "positive" {
        assert_ne!(
            before.2, "passed",
            "failed exchange must not become passing evidence"
        );
    }
    drop(journal);
    let reopened = CustodyOffNucJournal::open_existing(&journal_path).unwrap();
    assert!(reopened
        .issue_pre_read_request(&raw, &authority, &now())
        .is_err());
    assert!(make_client().read(&reopened, &raw, limits()).is_err());
    // A fresh signature/nonce must not turn the old ID into a new issuance.
    request.nonce = uuid::Uuid::new_v4().to_string();
    request.issued_at_utc = now();
    request.expires_at_utc =
        (wall_clock() + chrono::Duration::seconds(120)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let changed_signature =
        STANDARD.encode(signing.sign(&serde_jcs::to_vec(&request).unwrap()).as_ref());
    let changed = serde_jcs::to_vec(&CustodySignedRecordV1 {
        schema: CUSTODY_SIGNED_RECORD_SCHEMA_V1.into(),
        body: request.clone(),
        authority: authority.clone(),
        signature_base64: changed_signature,
    })
    .unwrap();
    assert!(reopened
        .issue_pre_read_request(&changed, &authority, &now())
        .is_err());
    assert!(make_client().read(&reopened, &changed, limits()).is_err());
    assert_eq!(journal_snapshot(&journal_path, &request.request_id), before);
    fs::write(REPLAY_DONE, b"done").unwrap();
    fs::set_permissions(REPLAY_DONE, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(format!("{VERIFIER}/passed"), b"passed").unwrap();
}
