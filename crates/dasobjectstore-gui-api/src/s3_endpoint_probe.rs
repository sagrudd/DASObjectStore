//! Non-secret runtime proof that the public S3 endpoint matches its descriptor.

#[cfg(test)]
#[path = "../../dasobjectstore-cli/src/supplier_tls_compat.rs"]
mod supplier_tls_compat;

use rustls::pki_types::{pem::PemObject as _, CertificateDer};
use std::fmt;
use std::path::Path;
use std::time::Duration;

const PROBE_BUCKET: &str = "dasobjectstore-protocol-probe";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROBE_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_TRUST_BUNDLE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedS3Endpoint {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum S3EndpointProbeError {
    InvalidDescriptor(String),
    ProtocolMismatch {
        advertised_endpoint: String,
        observed_protocol: String,
    },
    Unavailable {
        endpoint: String,
        reason: String,
    },
    InvalidS3Response {
        endpoint: String,
        status: u16,
    },
}

impl S3EndpointProbeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProtocolMismatch { .. } => "advertised_endpoint_protocol_mismatch",
            Self::InvalidDescriptor(_) => "s3_connection_descriptor_invalid",
            Self::Unavailable { .. } => "s3_endpoint_unavailable",
            Self::InvalidS3Response { .. } => "s3_endpoint_protocol_invalid",
        }
    }
}

impl fmt::Display for S3EndpointProbeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDescriptor(message) => out.write_str(message),
            Self::ProtocolMismatch {
                advertised_endpoint,
                observed_protocol,
            } => write!(
                out,
                "advertised endpoint {advertised_endpoint} does not match observed {observed_protocol}; correct s3_ingress.public_endpoint_url in /opt/dasobjectstore/config.json"
            ),
            Self::Unavailable { endpoint, reason } => {
                write!(out, "public S3 endpoint {endpoint} is unavailable: {reason}")
            }
            Self::InvalidS3Response { endpoint, status } => write!(
                out,
                "public endpoint {endpoint} returned HTTP {status} without a valid S3 protocol response"
            ),
        }
    }
}

pub async fn verify_public_s3_endpoint(
    endpoint: &str,
    trusted_certificate_path: &Path,
) -> Result<VerifiedS3Endpoint, S3EndpointProbeError> {
    let parsed = reqwest::Url::parse(endpoint).map_err(|_| {
        S3EndpointProbeError::InvalidDescriptor("public S3 endpoint is not a URL".to_string())
    })?;
    let scheme = parsed.scheme();
    let host = parsed.host_str().ok_or_else(|| {
        S3EndpointProbeError::InvalidDescriptor(
            "public S3 endpoint does not contain a host".to_string(),
        )
    })?;
    let port = parsed.port_or_known_default().ok_or_else(|| {
        S3EndpointProbeError::InvalidDescriptor(
            "public S3 endpoint does not contain a usable port".to_string(),
        )
    })?;
    if scheme != "https" {
        return Err(S3EndpointProbeError::InvalidDescriptor(
            "public S3 endpoint must use HTTPS".to_string(),
        ));
    }
    if scheme == "https" && plaintext_http_responds(host, port).await {
        return Err(S3EndpointProbeError::ProtocolMismatch {
            advertised_endpoint: endpoint.to_string(),
            observed_protocol: format!("plaintext HTTP on {host}:{port}"),
        });
    }
    let trust_metadata = tokio::fs::metadata(trusted_certificate_path)
        .await
        .map_err(|error| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: format!("configured TLS trust material is unavailable: {error}"),
        })?;
    if trust_metadata.len() == 0 || trust_metadata.len() > MAX_TRUST_BUNDLE_BYTES {
        return Err(S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "configured TLS trust material has an invalid size".to_string(),
        });
    }
    let trust_pem = tokio::fs::read(trusted_certificate_path)
        .await
        .map_err(|error| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: format!("configured TLS trust material cannot be read: {error}"),
        })?;
    let configured_certificates = parse_trust_bundle(endpoint, &trust_pem)?;
    let configured_leaf =
        configured_certificates
            .first()
            .ok_or_else(|| S3EndpointProbeError::Unavailable {
                endpoint: endpoint.to_string(),
                reason: "configured TLS trust material contains no certificates".to_string(),
            })?;
    let trust_anchors = if configured_certificates.len() == 1 {
        &configured_certificates[..1]
    } else {
        &configured_certificates[1..]
    };
    let probe_url = format!(
        "{}/{}?list-type=2&max-keys=0",
        endpoint.trim_end_matches('/'),
        PROBE_BUCKET
    );
    let mut client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .tls_built_in_root_certs(false)
        .min_tls_version(reqwest::tls::Version::TLS_1_3)
        .tls_info(true);
    for trust_anchor in trust_anchors {
        let trust_anchor = reqwest::Certificate::from_der(trust_anchor.as_ref()).map_err(|_| {
            S3EndpointProbeError::Unavailable {
                endpoint: endpoint.to_string(),
                reason: "configured TLS trust chain contains an invalid certificate".to_string(),
            }
        })?;
        client = client.add_root_certificate(trust_anchor);
    }
    let mut response = client
        .build()
        .map_err(|error| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: error.to_string(),
        })?
        .get(probe_url)
        .send()
        .await
        .map_err(|error| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: error.to_string(),
        })?;
    let presented_leaf = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(reqwest::tls::TlsInfo::peer_certificate);
    if presented_leaf != Some(configured_leaf.as_ref()) {
        return Err(S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "the endpoint did not present the configured appliance TLS certificate"
                .to_string(),
        });
    }
    let status = response.status().as_u16();
    let content_type_is_xml = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/xml"));
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROBE_RESPONSE_BYTES as u64)
    {
        return Err(S3EndpointProbeError::InvalidS3Response {
            endpoint: endpoint.to_string(),
            status,
        });
    }
    let mut body = Vec::new();
    while let Some(chunk) =
        response
            .chunk()
            .await
            .map_err(|error| S3EndpointProbeError::Unavailable {
                endpoint: endpoint.to_string(),
                reason: error.to_string(),
            })?
    {
        if chunk.len() > MAX_PROBE_RESPONSE_BYTES - body.len() {
            return Err(S3EndpointProbeError::InvalidS3Response {
                endpoint: endpoint.to_string(),
                status,
            });
        }
        body.extend_from_slice(&chunk);
    }
    let body_is_s3_xml = body.starts_with(b"<?xml")
        && (body
            .windows(b"<Error>".len())
            .any(|part| part == b"<Error>")
            || body
                .windows(b"<ListBucketResult".len())
                .any(|part| part == b"<ListBucketResult"));
    if !content_type_is_xml || !body_is_s3_xml {
        return Err(S3EndpointProbeError::InvalidS3Response {
            endpoint: endpoint.to_string(),
            status,
        });
    }
    Ok(VerifiedS3Endpoint {
        scheme: scheme.to_string(),
        host: host.to_string(),
        port,
    })
}

async fn plaintext_http_responds(host: &str, port: u16) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(Ok(mut stream)) =
        tokio::time::timeout(PROBE_TIMEOUT, tokio::net::TcpStream::connect((host, port))).await
    else {
        return false;
    };
    let request = format!(
        "GET /{PROBE_BUCKET}?list-type=2&max-keys=0 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut prefix = [0_u8; 16];
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT, stream.read(&mut prefix)).await,
        Ok(Ok(read)) if read >= 5 && prefix.starts_with(b"HTTP/")
    )
}

fn parse_trust_bundle(
    endpoint: &str,
    trust_pem: &[u8],
) -> Result<Vec<CertificateDer<'static>>, S3EndpointProbeError> {
    CertificateDer::pem_slice_iter(trust_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "configured TLS trust material is not a valid PEM certificate bundle"
                .to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::supplier_tls_compat;
    use super::*;
    use axum::http::{header, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use rcgen::{
        generate_simple_self_signed, BasicConstraints, CertificateParams, CertifiedIssuer, IsCa,
        KeyPair,
    };
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct TlsS3Fixture {
        address: std::net::SocketAddr,
        certificate_path: PathBuf,
        root: PathBuf,
        server: tokio::task::JoinHandle<std::io::Result<()>>,
    }

    impl Drop for TlsS3Fixture {
        fn drop(&mut self) {
            self.server.abort();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn tls_s3_server(label: &str, app: Router) -> TlsS3Fixture {
        let certificate = generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate certificate");
        tls_s3_server_with_pem(
            label,
            app,
            certificate.cert.pem(),
            certificate.signing_key.serialize_pem(),
        )
        .await
    }

    async fn tls_s3_server_with_pem(
        label: &str,
        app: Router,
        certificate_pem: String,
        private_key_pem: String,
    ) -> TlsS3Fixture {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = test_root(label);
        std::fs::create_dir_all(&root).expect("create test root");
        let certificate_path = root.join("server.crt");
        let private_key_path = root.join("server.key");
        std::fs::write(&certificate_path, certificate_pem).expect("write certificate");
        std::fs::write(&private_key_path, private_key_pem).expect("write private key");
        let tls = supplier_tls_compat::from_pem_file(&certificate_path, &private_key_path)
            .await
            .expect("load TLS");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        listener
            .set_nonblocking(true)
            .expect("make owned test TLS listener nonblocking");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            axum_server::from_tcp_rustls(listener, tls)
                .expect("compose owned test TLS listener")
                .serve(app.into_make_service())
                .await
        });
        TlsS3Fixture {
            address,
            certificate_path,
            root,
            server,
        }
    }

    fn assert_supplier_accepts(cert: &[u8], key: &[u8]) {
        let config =
            supplier_tls_compat::tests::new_config(cert, key).expect("fixed valid supplier corpus");
        assert_eq!(
            config.alpn_protocols,
            [b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }
    fn assert_supplier_denies(cert: &[u8], key: &[u8], kind: std::io::ErrorKind, message: &str) {
        let error = supplier_tls_compat::tests::new_config(cert, key)
            .expect_err("fixed denied supplier corpus");
        assert_eq!(error.kind(), kind);
        assert_eq!(error.to_string(), message);
    }
    #[test]
    fn supplier_old_new_generated_format_and_error_corpus_matches() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let fixture = supplier_tls_compat::tests::generated_formats();
        let malformed_key = "-----BEGIN PRIVATE KEY-----\n!\n-----END PRIVATE KEY-----\n";
        let malformed_cert = "-----BEGIN CERTIFICATE-----\n!\n-----END CERTIFICATE-----\n";
        for (index, (cert, key)) in fixture.pairs.iter().enumerate() {
            for keys in [
                key.clone(),
                format!("{cert}{key}"),
                format!("{malformed_key}{key}"),
                format!("{key}{malformed_key}"),
            ] {
                assert_supplier_accepts(cert.as_bytes(), keys.as_bytes());
            }
            let relabeled = key.replace("PRIVATE KEY", "RSA PRIVATE KEY");
            if index == 1 {
                assert_supplier_accepts(cert.as_bytes(), relabeled.as_bytes());
            } else {
                assert_supplier_denies(
                    cert.as_bytes(),
                    relabeled.as_bytes(),
                    std::io::ErrorKind::Other,
                    "private key format not supported",
                );
            }
            for keys in [String::new(), cert.clone(), format!("{key}{key}")] {
                assert_supplier_denies(
                    cert.as_bytes(),
                    keys.as_bytes(),
                    std::io::ErrorKind::Other,
                    "private key format not supported",
                );
            }
            for certs in [
                format!("{cert}{malformed_cert}"),
                format!("{malformed_cert}{cert}"),
                format!("{cert}{malformed_cert}").replace("\n", "\r\n"),
            ] {
                assert_supplier_denies(
                    certs.as_bytes(),
                    key.as_bytes(),
                    std::io::ErrorKind::InvalidData,
                    "InvalidCharacter(33)",
                );
            }
            assert_supplier_denies(
                &[],
                key.as_bytes(),
                std::io::ErrorKind::Other,
                "peer sent no certificates",
            );
        }
        assert_supplier_denies(
            fixture.pairs[0].0.as_bytes(),
            fixture.pairs[2].1.as_bytes(),
            std::io::ErrorKind::Other,
            "keys may not be consistent: KeyMismatch",
        );
        assert_supplier_denies(
            fixture.pairs[0].0.as_bytes(),
            format!("{}{}", fixture.pairs[0].1, fixture.pairs[2].1).as_bytes(),
            std::io::ErrorKind::Other,
            "private key format not supported",
        );
    }

    #[tokio::test]
    async fn supplier_old_new_three_formats_complete_real_https_handshakes() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let formats = supplier_tls_compat::tests::generated_formats();
        for (cert, key) in &formats.pairs {
            {
                let root = test_root("supplier-format-handshake");
                std::fs::create_dir_all(&root).expect("owned handshake root");
                let certificate_path = root.join("server.crt");
                std::fs::write(&certificate_path, cert).expect("owned certificate");
                let config =
                    supplier_tls_compat::tests::new_config(cert.as_bytes(), key.as_bytes())
                        .expect("actual supplier DER/provider config");
                let tls =
                    axum_server::tls_rustls::RustlsConfig::from_config(std::sync::Arc::new(config));
                let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("owned listener");
                listener
                    .set_nonblocking(true)
                    .expect("nonblocking listener");
                let address = listener.local_addr().expect("listener address");
                let server = tokio::spawn(async move {
                    axum_server::from_tcp_rustls(listener, tls)
                        .expect("compose TLS listener")
                        .serve(valid_s3_app().into_make_service())
                        .await
                });
                let fixture = TlsS3Fixture {
                    address,
                    certificate_path,
                    root,
                    server,
                };
                let endpoint = format!("https://localhost:{}", fixture.address.port());
                let verified = verify_public_s3_endpoint(&endpoint, &fixture.certificate_path)
                    .await
                    .expect("real supported-format TLS and S3 verification");
                assert_eq!(verified.host, "localhost");
                assert_eq!(verified.port, fixture.address.port());
            }
        }
    }

    #[test]
    fn supplier_dual_provider_failure_is_isolated() {
        if std::env::var_os("DAS_SUPPLIER_PROVIDER_CHILD").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            let (_, module) = module_path!().split_once("::").expect("test module path");
            let filter = format!("{module}::supplier_dual_provider_failure_is_isolated");
            let output =
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", &filter, "--nocapture"])
                    .env("DAS_SUPPLIER_PROVIDER_CHILD", "1")
                    .output()
                    .expect("provider child");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success(), "isolated provider assay failed");
            assert!(stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"));
            return;
        }
        assert!(
            rustls::crypto::CryptoProvider::get_default().is_none(),
            "fresh child default provider"
        );
        let _ = rustls::crypto::ring::default_provider();
        let _ = rustls::crypto::aws_lc_rs::default_provider();
        let fixture = supplier_tls_compat::tests::generated_formats();
        let (cert, key) = &fixture.pairs[0];
        let new = std::panic::catch_unwind(|| {
            supplier_tls_compat::tests::new_config(cert.as_bytes(), key.as_bytes())
        });
        assert!(
            new.is_err(),
            "dual-provider new builder must fail without selected provider"
        );
        assert!(rustls::crypto::CryptoProvider::get_default().is_none());
    }

    fn ca_issued_fullchains() -> (String, String, String) {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca =
            CertifiedIssuer::self_signed(ca_params, KeyPair::generate().expect("generate CA key"))
                .expect("generate CA");

        let server_key = KeyPair::generate().expect("generate server key");
        let server = CertificateParams::new(vec!["localhost".to_string()])
            .expect("server parameters")
            .signed_by(&server_key, &ca)
            .expect("sign server certificate");
        let sibling_key = KeyPair::generate().expect("generate sibling key");
        let sibling = CertificateParams::new(vec!["localhost".to_string()])
            .expect("sibling parameters")
            .signed_by(&sibling_key, &ca)
            .expect("sign sibling certificate");
        (
            format!("{}{}", server.pem(), ca.pem()),
            server_key.serialize_pem(),
            format!("{}{}", sibling.pem(), ca.pem()),
        )
    }

    fn valid_s3_app() -> Router {
        Router::new().route(
            "/{*path}",
            get(|| async {
                (
                    StatusCode::FORBIDDEN,
                    [(header::CONTENT_TYPE, "application/xml")],
                    r#"<?xml version="1.0"?><Error><Code>SignatureDoesNotMatch</Code></Error>"#,
                )
            }),
        )
    }

    async fn plaintext_s3_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut request = [0_u8; 1024];
                    let _ = socket.read(&mut request).await;
                    let body =
                        r#"<?xml version="1.0"?><Error><Code>SignatureDoesNotMatch</Code></Error>"#;
                    let response = format!(
                        "HTTP/1.1 403 Forbidden\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (address, task)
    }

    #[tokio::test]
    async fn verifies_native_tls_s3_with_configured_trust_material() {
        let fixture = tls_s3_server("native-tls", valid_s3_app()).await;
        let endpoint = format!("https://localhost:{}", fixture.address.port());
        let verified = verify_public_s3_endpoint(&endpoint, &fixture.certificate_path)
            .await
            .expect("native TLS S3 endpoint verifies");
        assert_eq!(verified.scheme, "https");
        assert_eq!(verified.host, "localhost");
        assert_eq!(verified.port, fixture.address.port());
    }

    #[tokio::test]
    async fn verifies_ca_issued_fullchain_and_rejects_sibling_leaf() {
        let (server_fullchain, server_key, sibling_fullchain) = ca_issued_fullchains();
        let fixture =
            tls_s3_server_with_pem("ca-fullchain", valid_s3_app(), server_fullchain, server_key)
                .await;
        let endpoint = format!("https://localhost:{}", fixture.address.port());
        verify_public_s3_endpoint(&endpoint, &fixture.certificate_path)
            .await
            .expect("CA-issued configured leaf verifies");

        let sibling_path = fixture.root.join("sibling-fullchain.pem");
        std::fs::write(&sibling_path, sibling_fullchain).expect("write sibling fullchain");
        let error = verify_public_s3_endpoint(&endpoint, &sibling_path)
            .await
            .expect_err("sibling signed by the same CA is not the configured leaf");
        assert_eq!(error.code(), "s3_endpoint_unavailable");
        assert!(error
            .to_string()
            .contains("configured appliance TLS certificate"));
    }

    #[tokio::test]
    async fn rejects_redirect_wrong_certificate_and_wrong_san() {
        let redirect = tls_s3_server(
            "redirect",
            Router::new().route(
                "/{*path}",
                get(|| async { axum::response::Redirect::temporary("https://example.invalid/") }),
            ),
        )
        .await;
        let endpoint = format!("https://localhost:{}", redirect.address.port());
        let error = verify_public_s3_endpoint(&endpoint, &redirect.certificate_path)
            .await
            .expect_err("redirect is not followed or accepted as S3");
        assert_eq!(error.code(), "s3_endpoint_protocol_invalid");

        let fixture = tls_s3_server("wrong-certificate", valid_s3_app()).await;
        let other_certificate = generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate other certificate");
        let other_certificate_path = fixture.root.join("other.crt");
        std::fs::write(&other_certificate_path, other_certificate.cert.pem())
            .expect("write other certificate");
        let endpoint = format!("https://localhost:{}", fixture.address.port());
        let error = verify_public_s3_endpoint(&endpoint, &other_certificate_path)
            .await
            .expect_err("different leaf is rejected");
        assert_eq!(error.code(), "s3_endpoint_unavailable");

        let wrong_san_endpoint = format!("https://127.0.0.1:{}", fixture.address.port());
        let error = verify_public_s3_endpoint(&wrong_san_endpoint, &fixture.certificate_path)
            .await
            .expect_err("wrong SAN is rejected");
        assert_eq!(error.code(), "s3_endpoint_unavailable");
    }

    #[tokio::test]
    async fn rejects_oversized_s3_response() {
        let fixture = tls_s3_server(
            "oversized",
            Router::new().route(
                "/{*path}",
                get(|| async {
                    (
                        StatusCode::FORBIDDEN,
                        [(header::CONTENT_TYPE, "application/xml")],
                        vec![b'x'; MAX_PROBE_RESPONSE_BYTES + 1],
                    )
                }),
            ),
        )
        .await;
        let endpoint = format!("https://localhost:{}", fixture.address.port());
        let error = verify_public_s3_endpoint(&endpoint, &fixture.certificate_path)
            .await
            .expect_err("oversized response is rejected");
        assert_eq!(error.code(), "s3_endpoint_protocol_invalid");
    }

    #[tokio::test]
    async fn rejects_plaintext_endpoint_and_false_https_advertisement() {
        let (address, task) = plaintext_s3_server().await;
        let error = verify_public_s3_endpoint(
            &format!("http://{address}"),
            Path::new("/unused-for-plaintext-rejection"),
        )
        .await
        .expect_err("plaintext descriptor rejected");
        assert!(matches!(error, S3EndpointProbeError::InvalidDescriptor(_)));

        let error = verify_public_s3_endpoint(
            &format!("https://{address}"),
            Path::new("/unused-for-protocol-mismatch"),
        )
        .await
        .expect_err("false HTTPS rejected");
        assert_eq!(error.code(), "advertised_endpoint_protocol_mismatch");
        task.abort();
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-s3-endpoint-probe-{label}-{}",
            uuid::Uuid::new_v4()
        ))
    }
}

#[cfg(test)]
mod bundle_parser_differential_tests {
    use super::*;
    #[test]
    fn actual_probe_parser_matches_old_full_collection() {
        let one = b"-----BEGIN CERTIFICATE-----\nAQ==\n-----END CERTIFICATE-----\n";
        let bad = b"-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n";
        for (bytes, expected) in [
            (Vec::new(), Some(Vec::<Vec<u8>>::new())),
            (one.to_vec(), Some(vec![vec![1]])),
            (bad.to_vec(), None),
            ([one.as_slice(), bad.as_slice()].concat(), None),
        ] {
            if let Some(expected) = expected {
                let actual = parse_trust_bundle("https://fixture.invalid", &bytes).unwrap();
                assert_eq!(
                    actual
                        .iter()
                        .map(|cert| cert.as_ref().to_vec())
                        .collect::<Vec<_>>(),
                    expected
                );
            } else {
                assert!(
                    matches!(parse_trust_bundle("https://fixture.invalid",&bytes),Err(S3EndpointProbeError::Unavailable{reason,..}) if reason=="configured TLS trust material is not a valid PEM certificate bundle")
                );
            }
        }
    }
}
