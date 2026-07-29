//! Non-secret runtime proof that the public S3 endpoint matches its descriptor.

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
    let certificates = reqwest::Certificate::from_pem_bundle(&trust_pem).map_err(|_| {
        S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "configured TLS trust material is not a valid PEM certificate bundle"
                .to_string(),
        }
    })?;
    if certificates.is_empty() {
        return Err(S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "configured TLS trust material contains no certificates".to_string(),
        });
    }

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
        .min_tls_version(reqwest::tls::Version::TLS_1_3);
    for certificate in certificates {
        client = client.add_root_certificate(certificate);
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
    use axum::http::{header, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use axum_server::tls_rustls::RustlsConfig;
    use rcgen::generate_simple_self_signed;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = test_root("native-tls");
        std::fs::create_dir_all(&root).expect("create test root");
        let certificate = generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate certificate");
        let certificate_path = root.join("server.crt");
        let private_key_path = root.join("server.key");
        std::fs::write(&certificate_path, certificate.cert.pem()).expect("write certificate");
        std::fs::write(&private_key_path, certificate.signing_key.serialize_pem())
            .expect("write private key");
        let tls = RustlsConfig::from_pem_file(&certificate_path, &private_key_path)
            .await
            .expect("load TLS");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let address = listener.local_addr().expect("address");
        drop(listener);
        let app = Router::new().route(
            "/{*path}",
            get(|| async {
                (
                    StatusCode::FORBIDDEN,
                    [(header::CONTENT_TYPE, "application/xml")],
                    r#"<?xml version="1.0"?><Error><Code>SignatureDoesNotMatch</Code></Error>"#,
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum_server::bind_rustls(address, tls)
                .serve(app.into_make_service())
                .await
        });

        let endpoint = format!("https://localhost:{}", address.port());
        let verified = verify_public_s3_endpoint(&endpoint, &certificate_path)
            .await
            .expect("native TLS S3 endpoint verifies");
        assert_eq!(verified.scheme, "https");
        assert_eq!(verified.host, "localhost");
        assert_eq!(verified.port, address.port());
        server.abort();
        let _ = std::fs::remove_dir_all(root);
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
