//! Process-local AWS settings for explicitly supplied session credentials.
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
const CONFIG: &[u8] = b"[default]\ns3 =\n    payload_signing_enabled = true\n";

pub(super) struct SignedPayloadConfig {
    root: PathBuf,
}

impl SignedPayloadConfig {
    pub(super) fn create() -> io::Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-aws-upload-{}-{nonce}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut directory = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            directory.mode(0o700);
        }
        directory.create(&root)?;
        let guard = Self { root };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(guard.root.join("config"))?;
        file.write_all(CONFIG)?;
        file.sync_all()?;
        Ok(guard)
    }
    pub(super) fn configure(&self, command: &mut Command) {
        // Explicit session credentials must not select an unrelated inherited
        // profile. Proxy environment, endpoint args and explicit CA are retained.
        command
            .env("AWS_CONFIG_FILE", self.root.join("config"))
            .env_remove("AWS_PROFILE")
            .env_remove("AWS_DEFAULT_PROFILE");
    }
}
impl Drop for SignedPayloadConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.root.join("config"));
        let _ = fs::remove_dir(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_session_clears_stale_token_but_profile_mode_is_unchanged() {
        let mut plan = crate::s3::AwsS3CommandPlan {
            program: "aws".into(),
            args: vec![],
            operation: crate::s3::AwsS3Operation::UploadFile {
                source: PathBuf::from("source"),
                destination: "s3://test/object".into(),
            },
            backpressure_policy: Default::default(),
        };
        let credentials = crate::auth::RemoteS3Credentials {
            access_key_id: "actual-test-access".into(),
            secret_access_key: "actual-test-secret".into(),
            session_token: None,
        };
        let (command, guard) =
            crate::s3::aws_command(&plan, Some(&credentials), Some("/verified/ca")).unwrap();
        assert!(guard.is_some());
        let vars: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(vars[std::ffi::OsStr::new("AWS_SESSION_TOKEN")], None);
        assert_eq!(
            vars[std::ffi::OsStr::new("AWS_ACCESS_KEY_ID")],
            Some(std::ffi::OsStr::new("actual-test-access"))
        );
        plan.args = vec!["--profile".into(), "existing".into()];
        let (command, guard) = crate::s3::aws_command(&plan, None, None).unwrap();
        assert!(guard.is_none());
        assert_eq!(command.get_envs().count(), 0);
    }
    #[test]
    #[ignore = "requires genuine /usr/bin/aws; run in disposable native tool guest"]
    fn real_aws_https_request_has_exact_signed_payload_hash() {
        use sha2::{Digest as _, Sha256};
        use std::io::{Read as _, Write as _};
        let guard = SignedPayloadConfig::create().unwrap();
        let payload = b"actual AWS signed payload regression\n";
        let source = guard.root.join("source");
        fs::write(&source, payload).unwrap();
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let ca = guard.root.join("ca.pem");
        fs::write(&ca, certificate.cert.pem()).unwrap();
        let server = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let receiver = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(error) => panic!("bounded HTTPS accept failed: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut tls = rustls::StreamOwned::new(
                rustls::ServerConnection::new(std::sync::Arc::new(server)).unwrap(),
                socket,
            );
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 16384);
                let mut byte = [0u8; 1];
                tls.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request).unwrap().to_ascii_lowercase();
            if headers.contains("expect: 100-continue") {
                tls.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                tls.flush().unwrap();
            }
            let mut body = vec![0u8; payload.len()];
            tls.read_exact(&mut body).unwrap();
            assert_eq!(body, payload);
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            tls.flush().unwrap();
            headers
        });
        let config: crate::config::RemoteConfig=serde_json::from_value(serde_json::json!({
            "endpoint_url":format!("https://{address}"),"region":"garage","profile":"unused-session-profile"})).unwrap();
        let mut plan = crate::s3::plan_upload_with_credentials(
            &config,
            "signed-test",
            &source,
            None,
            Some("object"),
            None,
            false,
            false,
            crate::s3::AwsS3CredentialSource::Environment,
        )
        .unwrap();
        plan.program = "/usr/bin/aws".into();
        let credentials = crate::auth::RemoteS3Credentials {
            access_key_id: "public-test-access".into(),
            secret_access_key: "public-test-secret".into(),
            session_token: Some("public-test-token".into()),
        };
        let result = crate::s3::execute_aws_plan(&plan, Some(&credentials), ca.to_str());
        let headers = receiver.join().unwrap();
        assert!(result.is_ok(), "actual AWS execution denied");
        assert!(headers.contains(&format!(
            "x-amz-content-sha256: {:x}",
            Sha256::digest(payload)
        )));
        assert!(headers.contains("authorization: aws4-hmac-sha256 "));
        assert!(headers.contains("/garage/s3/aws4_request"));
        assert!(!headers.contains("unsigned-payload"));
        fs::remove_file(source).unwrap();
        fs::remove_file(ca).unwrap();
    }
    #[test]
    fn private_child_config_is_fixed_and_lives_until_drop() {
        let guard = SignedPayloadConfig::create().unwrap();
        let root = guard.root.clone();
        assert_eq!(fs::read(root.join("config")).unwrap(), CONFIG);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(root.join("config"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let mut command = Command::new("aws");
        command
            .env("HTTPS_PROXY", "https://proxy.invalid")
            .env("AWS_CA_BUNDLE", "/actual/ca.pem");
        guard.configure(&mut command);
        let vars: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(
            vars[std::ffi::OsStr::new("HTTPS_PROXY")],
            Some(std::ffi::OsStr::new("https://proxy.invalid"))
        );
        assert_eq!(
            vars[std::ffi::OsStr::new("AWS_CA_BUNDLE")],
            Some(std::ffi::OsStr::new("/actual/ca.pem"))
        );
        assert_eq!(vars[std::ffi::OsStr::new("AWS_PROFILE")], None);
        drop(guard);
        assert!(!root.exists());
    }
}
