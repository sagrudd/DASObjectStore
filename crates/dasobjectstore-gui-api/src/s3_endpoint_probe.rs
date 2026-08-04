//! Non-secret runtime proof that the public S3 endpoint matches its descriptor.

use std::fmt;
use std::io::BufReader;
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
    let configured_certificates = rustls_pemfile::certs(&mut BufReader::new(trust_pem.as_slice()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "configured TLS trust material is not a valid PEM certificate bundle"
                .to_string(),
        })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use axum_server::tls_rustls::RustlsConfig;
    use rcgen::{
        generate_simple_self_signed, BasicConstraints, CertificateParams, CertifiedIssuer, IsCa,
        KeyPair,
    };
    use std::fs::File;
    use std::io::BufReader;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
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
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let root = test_root(label);
        std::fs::create_dir_all(&root).expect("create test root");
        let certificate_path = root.join("server.crt");
        let private_key_path = root.join("server.key");
        std::fs::write(&certificate_path, certificate_pem).expect("write certificate");
        std::fs::write(&private_key_path, private_key_pem).expect("write private key");
        let certificates = rustls_pemfile::certs(&mut BufReader::new(
            File::open(&certificate_path).expect("open certificate"),
        ))
        .collect::<Result<Vec<_>, _>>()
        .expect("parse certificate");
        let private_key = rustls_pemfile::private_key(&mut BufReader::new(
            File::open(&private_key_path).expect("open private key"),
        ))
        .expect("parse private key")
        .expect("private key is present");
        let server = rustls::ServerConfig::builder_with_protocol_versions(&[
            &rustls::version::TLS13,
        ])
        .with_no_client_auth()
        .with_single_cert(certificates, private_key)
        .expect("construct TLS 1.3 fixture");
        let tls = RustlsConfig::from_config(Arc::new(server));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let address = listener.local_addr().expect("address");
        drop(listener);
        let server = tokio::spawn(async move {
            axum_server::bind_rustls(address, tls)
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
    async fn tls13_s3_probe_sends_no_credentials_or_application_mutation() {
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&requests);
        let fixture = tls_s3_server(
            "tls13-no-credentials",
            Router::new().route(
                "/{*path}",
                get(move |headers: HeaderMap| {
                    let observed = Arc::clone(&observed);
                    async move {
                        assert!(
                            headers.get(header::AUTHORIZATION).is_none(),
                            "read-only S3 TLS probe must not send credentials"
                        );
                        observed.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::FORBIDDEN,
                            [(header::CONTENT_TYPE, "application/xml")],
                            r#"<?xml version="1.0"?><Error><Code>SignatureDoesNotMatch</Code></Error>"#,
                        )
                    }
                }),
            ),
        )
        .await;
        let endpoint = format!("https://localhost:{}", fixture.address.port());
        verify_public_s3_endpoint(&endpoint, &fixture.certificate_path)
            .await
            .expect("TLS 1.3 read-only S3 probe verifies");
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "probe must make exactly one authenticated-protocol request"
        );
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
