//! Non-secret runtime proof that the public S3 endpoint matches its descriptor.

use std::fmt;
use std::time::Duration;

const PROBE_BUCKET: &str = "dasobjectstore-protocol-probe";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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
    if scheme == "https" && plaintext_http_responds(host, port).await {
        return Err(S3EndpointProbeError::ProtocolMismatch {
            advertised_endpoint: endpoint.to_string(),
            observed_protocol: format!("plaintext HTTP on {host}:{port}"),
        });
    }
    if scheme != "http" {
        return Err(S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: "the direct S3 gateway has no configured TLS listener".to_string(),
        });
    }

    let probe_url = format!(
        "{}/{}?list-type=2&max-keys=0",
        endpoint.trim_end_matches('/'),
        PROBE_BUCKET
    );
    let response = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
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
    let body = response
        .bytes()
        .await
        .map_err(|error| S3EndpointProbeError::Unavailable {
            endpoint: endpoint.to_string(),
            reason: error.to_string(),
        })?;
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
    async fn verifies_plaintext_s3_and_rejects_false_https_advertisement() {
        let (address, task) = plaintext_s3_server().await;
        let verified = verify_public_s3_endpoint(&format!("http://{address}"))
            .await
            .expect("HTTP S3 endpoint verifies");
        assert_eq!(verified.scheme, "http");

        let error = verify_public_s3_endpoint(&format!("https://{address}"))
            .await
            .expect_err("false HTTPS rejected");
        assert_eq!(error.code(), "advertised_endpoint_protocol_mismatch");
        task.abort();
    }
}
