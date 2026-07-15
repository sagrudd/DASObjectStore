//! Dedicated mutual-TLS listener for unattended application authentication.

use crate::StandaloneServerConfig;
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use dasobjectstore_daemon::api::{
    ApplicationAccessTokenExchangeRequest, ApplicationAccessTokenExchangeResponse,
    ApplicationUploadCapabilityIssueRequest, ApplicationUploadCapabilityIssueResponse,
    ApplicationUploadCompletionRequest, ApplicationUploadCompletionResponse,
    APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE, APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
    APPLICATION_UPLOAD_COMPLETION_ROUTE,
};
use dasobjectstore_daemon::runtime::resolve_mtls_application_identity;
use dasobjectstore_daemon::{DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use serde::Serialize;
use std::fmt::{self, Display};
use std::fs::File;
use std::io::{self, BufReader};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;

#[derive(Clone, Debug)]
pub struct MtlsApplicationConnectInfo {
    pub peer_address: SocketAddr,
    pub application_id: String,
    certificate_der: Arc<Vec<u8>>,
    identity_registry_path: std::path::PathBuf,
    key_registry_path: std::path::PathBuf,
}

pub struct MtlsApplicationListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    identity_registry_path: std::path::PathBuf,
    key_registry_path: std::path::PathBuf,
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
            let certificate_der = certificate.as_ref().to_vec();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let Ok(identity) = resolve_mtls_application_identity(
                &self.identity_registry_path,
                &self.key_registry_path,
                &certificate_der,
                now,
            ) else {
                continue;
            };
            return (
                tls,
                MtlsApplicationConnectInfo {
                    peer_address,
                    application_id: identity.application_id,
                    certificate_der: Arc::new(certificate_der),
                    identity_registry_path: self.identity_registry_path.clone(),
                    key_registry_path: self.key_registry_path.clone(),
                },
            );
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(MtlsApplicationConnectInfo {
            peer_address: self.listener.local_addr()?,
            application_id: String::new(),
            certificate_der: Arc::new(Vec::new()),
            identity_registry_path: self.identity_registry_path.clone(),
            key_registry_path: self.key_registry_path.clone(),
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
        identity_registry_path: mtls.application_identity_registry_path.clone(),
        key_registry_path: mtls.application_key_registry_path.clone(),
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
    let certificate_der = Arc::clone(&peer.certificate_der);
    let identity_registry_path = peer.identity_registry_path.clone();
    let key_registry_path = peer.key_registry_path.clone();
    let identity = tokio::task::spawn_blocking(move || {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        resolve_mtls_application_identity(
            identity_registry_path,
            key_registry_path,
            certificate_der.as_slice(),
            now,
        )
    })
    .await
    .map_err(|_| MtlsRouteError::forbidden("client certificate revalidation failed"))?
    .map_err(|_| MtlsRouteError::forbidden("client certificate is no longer authorized"))?;
    if identity.application_id != peer.application_id
        || identity.application_id != requested_application_id
    {
        return Err(MtlsRouteError::forbidden(
            "client certificate is not authorized for the requested application identity",
        ));
    }
    Ok(())
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
    use super::{
        authorize_application, build_application_mtls_listener, MtlsApplicationConnectInfo,
    };
    use crate::StandaloneServerConfig;
    use dasobjectstore_core::application_auth::{
        ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
        ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
        APPLICATION_AUTH_SCHEMA_VERSION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_daemon::runtime::{
        deactivate_application_key, upsert_application_identity, upsert_application_key,
    };
    use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, RootCertStore};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio_rustls::TlsConnector;

    #[tokio::test]
    async fn every_request_revalidates_identity_and_revocation() {
        let root = test_root("request-revalidation");
        let identities = root.join("identities.json");
        let keys = root.join("keys.json");
        let certificate_der = b"persistent-connection-client-certificate".to_vec();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("current time")
            .as_secs();
        upsert_application_identity(&identities, identity("synoptikon", now))
            .expect("register identity");
        upsert_application_key(&keys, key("synoptikon", "cert-1", &certificate_der, now))
            .expect("register certificate");
        let peer = MtlsApplicationConnectInfo {
            peer_address: "127.0.0.1:49152".parse().expect("address"),
            application_id: "synoptikon".to_string(),
            certificate_der: Arc::new(certificate_der),
            identity_registry_path: identities,
            key_registry_path: keys.clone(),
        };

        assert!(authorize_application(&peer, "synoptikon").await.is_ok());
        assert!(authorize_application(&peer, "monas").await.is_err());
        deactivate_application_key(&keys, "synoptikon", "cert-1").expect("revoke certificate");
        assert!(
            authorize_application(&peer, "synoptikon").await.is_err(),
            "a keepalive connection must not survive certificate revocation"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

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
        server.application_mtls.application_identity_registry_path = root.join("identities.json");
        server.application_mtls.application_key_registry_path = root.join("keys.json");
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

    fn identity(application_id: &str, now: u64) -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            owner: "operator".to_string(),
            purpose: "integration ingress".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::MtlsCertificate,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new("codex").expect("store")],
                prefixes: vec!["integration".to_string()],
                object_types: vec![ObjectType::Fastq],
                operations: vec![ApplicationOperation::Write],
                ingress_origin: IngressOrigin::Synoptikon,
                max_object_bytes: Some(1_000),
                max_total_bytes: Some(10_000),
            },
            issued_at_unix_seconds: now.saturating_sub(60),
            expires_at_unix_seconds: now + 3_600,
            active: true,
        }
    }

    fn key(
        application_id: &str,
        key_id: &str,
        certificate_der: &[u8],
        now: u64,
    ) -> ApplicationKeyDescriptor {
        use sha2::{Digest, Sha256};
        let fingerprint = Sha256::digest(certificate_der)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            key_id: key_id.to_string(),
            algorithm: ApplicationKeyAlgorithm::MtlsCertificate,
            public_key_fingerprint: format!("sha256:{fingerprint}"),
            public_key_material: None,
            issued_at_unix_seconds: now.saturating_sub(60),
            expires_at_unix_seconds: now + 3_600,
            active: true,
        }
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
