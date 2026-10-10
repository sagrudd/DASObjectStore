//! Private compatibility boundary for the former axum-server 0.7 PEM loader.
use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::{
    pem::{Error as PemError, PemObject},
    CertificateDer, PrivateKeyDer,
};
use std::{io, path::Path, sync::Arc};

pub(super) async fn from_pem_file(cert: &Path, key: &Path) -> io::Result<RustlsConfig> {
    let cert = read_file(cert).await?;
    let key = read_file(key).await?;
    let config = config_from_pem(&cert, &key)?;
    Ok(RustlsConfig::from_config(Arc::new(config)))
}

async fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    tokio::fs::read(path).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to read from file `{}`: {error}", path.display()),
        )
    })
}

fn config_from_pem(cert: &[u8], key: &[u8]) -> io::Result<rustls::ServerConfig> {
    let cert = CertificateDer::pem_reader_iter(cert)
        .map(|item| item.map_err(pem_io_error))
        .collect::<Result<Vec<_>, _>>()?;
    // The supplier scanned the entire key input and ignored iterator errors.
    // This is intentionally distinct from the mTLS first-key reader.
    let mut keys = PrivateKeyDer::pem_reader_iter(key)
        .filter_map(Result::ok)
        .map(|key| key.secret_der().to_vec())
        .collect::<Vec<_>>();
    if keys.len() != 1 {
        return Err(io::Error::other("private key format not supported"));
    }
    // Preserve the old DER builder's format classification instead of trusting
    // the PEM label to decide the signing key representation.
    let key = PrivateKeyDer::try_from(keys.remove(0)).map_err(io::Error::other)?;
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert, key)
        .map_err(io::Error::other)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

fn pem_io_error(error: PemError) -> io::Error {
    let message = match error {
        PemError::Io(error) => return error,
        PemError::MissingSectionEnd { end_marker } => format!(
            "section end {:?} missing",
            String::from_utf8_lossy(&end_marker)
        ),
        PemError::IllegalSectionStart { line } => format!(
            "illegal section start: {:?}",
            String::from_utf8_lossy(&line)
        ),
        PemError::Base64Decode(message) => message,
        other => format!("{other:?}"),
    };
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{config_from_pem, from_pem_file};
    use std::{
        fs, io,
        time::{SystemTime, UNIX_EPOCH},
    };

    pub(crate) struct GeneratedFormats {
        root: std::path::PathBuf,
        pub(crate) pairs: Vec<(String, String)>,
    }

    impl Drop for GeneratedFormats {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    pub(crate) fn generated_formats() -> GeneratedFormats {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("supplier-formats-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("owned ephemeral key fixture directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .expect("private key fixture directory");
        }
        let mut fixture = GeneratedFormats {
            root,
            pairs: Vec::new(),
        };
        fn openssl(root: &std::path::Path, args: &[&str]) {
            let result = std::process::Command::new("openssl")
                .current_dir(root)
                .args(args)
                .output()
                .expect("installed native OpenSSL fixture producer required");
            assert!(
                result.status.success(),
                "native OpenSSL fixture producer failed"
            );
        }
        openssl(
            &fixture.root,
            &["genrsa", "-traditional", "-out", "rsa1.pem", "2048"],
        );
        openssl(
            &fixture.root,
            &[
                "pkcs8", "-topk8", "-nocrypt", "-in", "rsa1.pem", "-out", "rsa8.pem",
            ],
        );
        openssl(
            &fixture.root,
            &[
                "ecparam",
                "-name",
                "prime256v1",
                "-genkey",
                "-noout",
                "-out",
                "ec.pem",
            ],
        );
        for (key, cert) in [("rsa1.pem", "rsa.crt"), ("ec.pem", "ec.crt")] {
            openssl(
                &fixture.root,
                &[
                    "req",
                    "-new",
                    "-x509",
                    "-key",
                    key,
                    "-out",
                    cert,
                    "-days",
                    "1",
                    "-subj",
                    "/CN=localhost",
                    "-addext",
                    "subjectAltName=DNS:localhost",
                    "-addext",
                    "basicConstraints=critical,CA:FALSE",
                ],
            );
        }
        for (cert, key) in [
            ("rsa.crt", "rsa1.pem"),
            ("rsa.crt", "rsa8.pem"),
            ("ec.crt", "ec.pem"),
        ] {
            fixture.pairs.push((
                fs::read_to_string(fixture.root.join(cert)).expect("owned certificate"),
                fs::read_to_string(fixture.root.join(key)).expect("owned key"),
            ));
        }
        fixture
    }

    pub(crate) fn new_config(cert: &[u8], key: &[u8]) -> io::Result<rustls::ServerConfig> {
        config_from_pem(cert, key)
    }

    #[test]
    fn supplier_generated_formats_preserve_supported_der_and_alpn() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let fixture = generated_formats();
        for (cert, key) in &fixture.pairs {
            let config =
                new_config(cert.as_bytes(), key.as_bytes()).expect("generated matching format");
            assert_eq!(
                config.alpn_protocols,
                [b"h2".to_vec(), b"http/1.1".to_vec()]
            );
        }
    }

    fn material() -> (String, String) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate owned matching certificate and key");
        (generated.cert.pem(), generated.signing_key.serialize_pem())
    }

    #[test]
    fn supplier_full_scan_requires_exactly_one_key_and_preserves_alpn() {
        let (cert, key) = material();
        let config = config_from_pem(cert.as_bytes(), key.as_bytes()).expect("matching pair");
        assert_eq!(
            config.alpn_protocols,
            [b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        for keys in [String::new(), cert.clone(), format!("{key}{key}")] {
            let error = config_from_pem(cert.as_bytes(), keys.as_bytes())
                .expect_err("zero or multiple keys deny");
            assert_eq!(error.kind(), io::ErrorKind::Other);
            assert_eq!(error.to_string(), "private key format not supported");
        }
    }

    #[test]
    fn supplier_key_scan_ignores_item_errors_and_scans_past_non_key_sections() {
        let (cert, key) = material();
        let malformed = "-----BEGIN PRIVATE KEY-----\n!\n-----END PRIVATE KEY-----\n";
        for keys in [
            format!("{cert}{key}"),
            format!("{malformed}{key}"),
            format!("{key}{malformed}"),
        ] {
            config_from_pem(cert.as_bytes(), keys.as_bytes())
                .expect("one successful key despite other sections");
        }
    }

    #[test]
    fn supplier_full_certificate_errors_and_mismatched_pair_remain_denials() {
        let (cert, key) = material();
        let malformed =
            format!("{cert}-----BEGIN CERTIFICATE-----\n!\n-----END CERTIFICATE-----\n");
        let error = config_from_pem(malformed.as_bytes(), key.as_bytes())
            .expect_err("trailing certificate error denies");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let (_, other_key) = material();
        assert!(config_from_pem(cert.as_bytes(), other_key.as_bytes()).is_err());
        assert!(config_from_pem(&[], key.as_bytes()).is_err());
    }

    #[tokio::test]
    async fn supplier_actual_file_loader_preserves_read_order_and_error_display() {
        let (cert, key) = material();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("supplier-tls-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("private fixture directory");
        let cert_path = root.join("cert.pem");
        let key_path = root.join("key.pem");
        let error = from_pem_file(&cert_path, &key_path)
            .await
            .expect_err("certificate read precedes key");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().starts_with(&format!(
            "failed to read from file `{}`:",
            cert_path.display()
        )));
        fs::write(&cert_path, cert).expect("owned certificate");
        let error = from_pem_file(&cert_path, &key_path)
            .await
            .expect_err("missing key denies");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().starts_with(&format!(
            "failed to read from file `{}`:",
            key_path.display()
        )));
        fs::write(&key_path, key).expect("owned key");
        from_pem_file(&cert_path, &key_path)
            .await
            .expect("actual matched file loader");
        fs::remove_dir_all(root).expect("remove only owned synthetic fixture");
    }
}
