//! Dedicated mutual-TLS listener for unattended application authentication.

use crate::StandaloneServerConfig;
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use dasobjectstore_daemon::api::{
    ApplicationAccessTokenExchangeRequest, ApplicationAccessTokenExchangeResponse,
    ApplicationMtlsAuthorizationContext, ApplicationMtlsAuthorizationRequest,
    ApplicationUploadCapabilityIssueRequest, ApplicationUploadCapabilityIssueResponse,
    ApplicationUploadCompletionRequest, ApplicationUploadCompletionResponse,
    APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE, APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
    APPLICATION_UPLOAD_COMPLETION_ROUTE,
};
use dasobjectstore_daemon::{DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::File;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

#[derive(Clone, Debug)]
pub struct MtlsApplicationConnectInfo {
    pub peer_address: SocketAddr,
    pub application_id: String,
    certificate_fingerprint_sha256: String,
}

pub struct MtlsApplicationListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl axum::serve::Listener for MtlsApplicationListener {
    type Io = TlsStream<TcpStream>;
    type Addr = MtlsApplicationConnectInfo;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok((stream, peer_address)) = self.listener.accept().await else {
                continue;
            };
            let Ok(tls) = self.acceptor.accept(stream).await else {
                continue;
            };
            let Some(certificate) = tls
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|chain| chain.first())
            else {
                continue;
            };
            let certificate_fingerprint_sha256 = certificate_fingerprint(certificate.as_ref());
            let Ok(response) = daemon_mtls_authorization(
                certificate_fingerprint_sha256.clone(),
                None,
                ApplicationMtlsAuthorizationContext::Connection,
            )
            .await
            else {
                continue;
            };
            let Some(application_id) = response.application_id.filter(|_| response.authorized)
            else {
                continue;
            };
            return (
                tls,
                MtlsApplicationConnectInfo {
                    peer_address,
                    application_id,
                    certificate_fingerprint_sha256,
                },
            );
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(MtlsApplicationConnectInfo {
            peer_address: self.listener.local_addr()?,
            application_id: String::new(),
            certificate_fingerprint_sha256: String::new(),
        })
    }
}

impl Connected<axum::serve::IncomingStream<'_, MtlsApplicationListener>>
    for MtlsApplicationConnectInfo
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, MtlsApplicationListener>) -> Self {
        stream.remote_addr().clone()
    }
}

pub async fn build_application_mtls_listener(
    config: &StandaloneServerConfig,
) -> Result<MtlsApplicationListener, MtlsListenerError> {
    config
        .validate()
        .map_err(|error| MtlsListenerError::Config(error.to_string()))?;
    let mtls = &config.application_mtls;
    if !mtls.enabled {
        return Err(MtlsListenerError::Config(
            "application mTLS listener is not enabled".to_string(),
        ));
    }
    let mut roots = RootCertStore::empty();
    for certificate in read_certificates(&mtls.client_ca_path)? {
        roots.add(certificate).map_err(|error| {
            MtlsListenerError::Tls(format!("invalid client CA certificate: {error}"))
        })?;
    }
    if roots.is_empty() {
        return Err(MtlsListenerError::Tls(
            "client CA file contains no certificates".to_string(),
        ));
    }
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| MtlsListenerError::Tls(error.to_string()))?;
    let certificates = read_certificates(&config.tls.certificate_path)?;
    let private_key = read_private_key(&config.tls.private_key_path)?;
    let mut server = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certificates, private_key)
        .map_err(|error| MtlsListenerError::Tls(error.to_string()))?;
    server.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let listener = TcpListener::bind(
        mtls.socket_addr()
            .map_err(|error| MtlsListenerError::Config(error.to_string()))?,
    )
    .await?;
    Ok(MtlsApplicationListener {
        listener,
        acceptor: TlsAcceptor::from(Arc::new(server)),
    })
}

pub fn application_mtls_router() -> Router {
    Router::new()
        .route(APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE, post(mtls_exchange))
        .route(
            APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
            post(mtls_issue_upload_capability),
        )
        .route(
            APPLICATION_UPLOAD_COMPLETION_ROUTE,
            post(mtls_complete_upload),
        )
}

async fn mtls_exchange(
    ConnectInfo(peer): ConnectInfo<MtlsApplicationConnectInfo>,
    Json(request): Json<ApplicationAccessTokenExchangeRequest>,
) -> Result<Json<ApplicationAccessTokenExchangeResponse>, MtlsRouteError> {
    authorize_application(&peer, &request.exchange.application_id).await?;
    daemon_call(move |client| client.exchange_application_access_token(request)).await
}

async fn mtls_issue_upload_capability(
    ConnectInfo(peer): ConnectInfo<MtlsApplicationConnectInfo>,
    Json(request): Json<ApplicationUploadCapabilityIssueRequest>,
) -> Result<Json<ApplicationUploadCapabilityIssueResponse>, MtlsRouteError> {
    authorize_application(&peer, &request.application_id).await?;
    daemon_call(move |client| client.issue_application_upload_capability(request)).await
}

async fn mtls_complete_upload(
    ConnectInfo(peer): ConnectInfo<MtlsApplicationConnectInfo>,
    Json(request): Json<ApplicationUploadCompletionRequest>,
) -> Result<Json<ApplicationUploadCompletionResponse>, MtlsRouteError> {
    authorize_application(&peer, &request.capability.application_id).await?;
    daemon_call(move |client| client.complete_application_upload(request)).await
}

async fn authorize_application(
    peer: &MtlsApplicationConnectInfo,
    requested_application_id: &str,
) -> Result<(), MtlsRouteError> {
    let response = daemon_mtls_authorization(
        peer.certificate_fingerprint_sha256.clone(),
        Some(requested_application_id.to_string()),
        ApplicationMtlsAuthorizationContext::Request,
    )
    .await
    .map_err(|error| MtlsRouteError::unavailable(error.to_string()))?;
    if !response.authorized
        || response.application_id.as_deref() != Some(peer.application_id.as_str())
        || response.application_id.as_deref() != Some(requested_application_id)
    {
        return Err(MtlsRouteError::forbidden(
            "client certificate is not authorized for the requested application identity",
        ));
    }
    Ok(())
}

async fn daemon_mtls_authorization(
    certificate_fingerprint_sha256: String,
    requested_application_id: Option<String>,
    context: ApplicationMtlsAuthorizationContext,
) -> Result<
    dasobjectstore_daemon::api::ApplicationMtlsAuthorizationResponse,
    dasobjectstore_daemon::DaemonClientError,
> {
    tokio::task::spawn_blocking(move || {
        let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
            DaemonRuntimeConfig::default_packaged().socket_path,
        ));
        client.authorize_application_mtls(ApplicationMtlsAuthorizationRequest {
            certificate_fingerprint_sha256,
            requested_application_id,
            context,
        })
    })
    .await
    .map_err(|_| {
        dasobjectstore_daemon::DaemonClientError::Transport(
            "daemon mTLS authorization task failed".to_string(),
        )
    })?
}

fn certificate_fingerprint(certificate_der: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(certificate_der))
}

async fn daemon_call<T: Send + 'static>(
    call: impl FnOnce(
            &DaemonClient<UnixSocketDaemonTransport>,
        ) -> Result<T, dasobjectstore_daemon::DaemonClientError>
        + Send
        + 'static,
) -> Result<Json<T>, MtlsRouteError> {
    tokio::task::spawn_blocking(move || {
        let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
            DaemonRuntimeConfig::default_packaged().socket_path,
        ));
        call(&client)
    })
    .await
    .map_err(|_| MtlsRouteError::unavailable("daemon bridge task failed"))?
    .map(Json)
    .map_err(|error| MtlsRouteError::unavailable(error.to_string()))
}

fn read_certificates(
    path: &std::path::Path,
) -> Result<Vec<CertificateDer<'static>>, MtlsListenerError> {
    let file = File::open(path)?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(MtlsListenerError::Io)
}

fn read_private_key(path: &std::path::Path) -> Result<PrivateKeyDer<'static>, MtlsListenerError> {
    let file = File::open(path)?;
    rustls_pemfile::private_key(&mut BufReader::new(file))?.ok_or_else(|| {
        MtlsListenerError::Tls("server private-key file contains no key".to_string())
    })
}

#[derive(Debug)]
pub enum MtlsListenerError {
    Config(String),
    Tls(String),
    Io(io::Error),
}

impl Display for MtlsListenerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) | Self::Tls(message) => formatter.write_str(message),
            Self::Io(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for MtlsListenerError {}

impl From<io::Error> for MtlsListenerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Serialize)]
struct MtlsErrorBody {
    code: &'static str,
    message: String,
}

struct MtlsRouteError(StatusCode, Json<MtlsErrorBody>);

impl MtlsRouteError {
    fn forbidden(message: impl Into<String>) -> Self {
        Self(
            StatusCode::FORBIDDEN,
            Json(MtlsErrorBody {
                code: "mtls_application_identity_mismatch",
                message: message.into(),
            }),
        )
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MtlsErrorBody {
                code: "daemon_unavailable",
                message: message.into(),
            }),
        )
    }
}

impl axum::response::IntoResponse for MtlsRouteError {
    fn into_response(self) -> axum::response::Response {
        (self.0, self.1).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::build_application_mtls_listener;
    use crate::StandaloneServerConfig;
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, RootCertStore};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio_rustls::TlsConnector;

    #[tokio::test]
    async fn listener_rejects_tls_clients_without_a_certificate() {
        let root = test_root("client-certificate-required");
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_key = KeyPair::generate().expect("CA key");
        let ca_certificate = ca_params.self_signed(&ca_key).expect("CA certificate");
        let issuer = Issuer::new(ca_params, ca_key);
        let server_key = KeyPair::generate().expect("server key");
        let server_certificate = CertificateParams::new(vec!["localhost".to_string()])
            .expect("server params")
            .signed_by(&server_key, &issuer)
            .expect("server certificate");
        let ca_path = root.join("client-ca.crt");
        let certificate_path = root.join("server.crt");
        let private_key_path = root.join("server.key");
        fs::write(&ca_path, ca_certificate.pem()).expect("write CA");
        fs::write(&certificate_path, server_certificate.pem()).expect("write certificate");
        fs::write(&private_key_path, server_key.serialize_pem()).expect("write key");

        let port = available_port();
        let mut server = StandaloneServerConfig::default();
        server.tls.certificate_path = certificate_path;
        server.tls.private_key_path = private_key_path;
        server.application_mtls.enabled = true;
        server.application_mtls.https_port = port;
        server.application_mtls.client_ca_path = ca_path;
        let listener = build_application_mtls_listener(&server)
            .await
            .expect("listener builds");
        let task = tokio::spawn(async move { axum::serve(listener, axum::Router::new()).await });

        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca_certificate.der().to_vec()))
            .expect("trust CA");
        let client = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("TCP connects");
        let result = TlsConnector::from(Arc::new(client))
            .connect(
                ServerName::try_from("localhost").expect("server name"),
                stream,
            )
            .await;
        if let Ok(mut tls) = result {
            let write = tls
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await;
            if write.is_ok() {
                let mut response = [0_u8; 1];
                let read = tls.read(&mut response).await;
                assert!(
                    matches!(read, Ok(0) | Err(_)),
                    "mTLS listener served a client without a certificate"
                );
            }
        }
        task.abort();
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn available_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .expect("reserve port")
            .local_addr()
            .expect("local address")
            .port()
    }

    fn test_root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(".dasobjectstore-codex-validation")
            .join(format!(
                "mtls-listener-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }
}
