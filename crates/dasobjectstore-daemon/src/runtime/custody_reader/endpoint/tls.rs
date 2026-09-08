//! Opt-in real TLS 1.3 transport. No listener installation or credential provisioning.
use super::*;
use dasobjectstore_object_service::custody::CustodyReadDeadline;
use dasobjectstore_object_service::custody_attestation::{
    CustodyOffNucJournal, CustodyOffNucReadAttemptV1,
};
use dasobjectstore_object_service::ObjectServiceError;
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection,
};
use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
    sync::Arc,
};
#[path = "tls_io.rs"]
mod io;
use io::Channel;
#[cfg(all(test, feature = "development-self-signing"))]
mod tests;

/// Provisioned identity material. Roots are CA trust, not a substitute for exact leaf pins.
pub struct TlsIdentity {
    /// Exact DER chain with end entity first.
    pub chain: Vec<CertificateDer<'static>>,
    /// Matching private key; never sourced from an HTTP message.
    pub key: PrivateKeyDer<'static>,
    /// Independently selected peer trust roots.
    pub peer_roots: RootCertStore,
}
fn own_leaf(identity: &TlsIdentity, pin: &str) -> Result<(), ReaderError> {
    if identity
        .chain
        .first()
        .map(|c| raw_sha256(c.as_ref()))
        .as_deref()
        != Some(pin)
    {
        return Err(ReaderError::Binding);
    }
    Ok(())
}
/// Private immutable mandatory-client-auth configuration, bound to one full reader binding.
pub struct ServerTls {
    config: Arc<ServerConfig>,
    binding_sha256: String,
    client_leaf: String,
}
impl ServerTls {
    /// Construct WebPKI client-CA authentication plus exact selected DER leaf pins.
    /// # Errors
    /// Denies malformed configuration, empty trust roots or mismatched local identity.
    pub fn new(binding: &ReaderBindingV1, identity: TlsIdentity) -> Result<Self, ReaderError> {
        let binding_sha256 = raw_sha256(&binding.encode()?);
        own_leaf(&identity, &binding.frontend_tls_peer_sha256)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(identity.peer_roots),
            provider.clone(),
        )
        .build()
        .map_err(|_| ReaderError::Binding)?;
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|_| ReaderError::Binding)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(identity.chain, identity.key)
            .map_err(|_| ReaderError::Binding)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        Ok(Self {
            config: Arc::new(config),
            binding_sha256,
            client_leaf: binding.verifier_tls_identity_sha256.clone(),
        })
    }
    fn accept(
        &self,
        stream: TcpStream,
        deadline: CustodyReadDeadline,
    ) -> Result<Channel, ReaderError> {
        Channel::authenticate(
            ServerConnection::new(self.config.clone())
                .map_err(|_| ReaderError::Binding)?
                .into(),
            stream,
            deadline,
            &self.client_leaf,
        )
    }
}
impl<R: ServiceCommandRunner> ExactObjectServer<'_, R> {
    /// Serve exactly one accepted connection using the actual loaded continuation.
    /// No second request dispatch, HTTP EOF assertion, listener or background task.
    /// # Errors
    /// TLS/framing/deadline failures disconnect. Authenticated application denials are fixed.
    pub fn serve_connection(
        &mut self,
        stream: TcpStream,
        tls: &ServerTls,
        limits: CustodyReadLimits,
    ) -> Result<(), ReaderError> {
        let deadline = limits.start().map_err(|_| ReaderError::Read)?;
        if raw_sha256(&self.reader.selection.binding.encode()?) != tls.binding_sha256 {
            return Err(ReaderError::Binding);
        }
        self.reader.recheck(&clock_now())?;
        let mut channel = tls.accept(stream, deadline)?;
        let request =
            channel.collect(|h| http::request_body_length(h, &self.policy.host), false)?;
        let response = match self.read_http_at(&request, limits, deadline) {
            Ok(response) => {
                self.reader.recheck(&clock_now())?;
                response
            }
            Err(_) => http::encode_denied(),
        };
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        channel
            .write_all(&response)
            .map_err(|_| ReaderError::Read)?;
        channel.finish()
    }
}

/// Fixed destination, validated server name, mandatory client certificate and finite selection.
/// Construction consumes trusted installation facts; it does not mint their admission.
pub struct ExactObjectClient {
    config: Arc<ClientConfig>,
    address: SocketAddr,
    name: ServerName<'static>,
    server_leaf: String,
    policy: RequestPolicy,
}
/// Exact verified bytes and the actual independently journalled started attempt.
pub struct ExactRead {
    /// Full object bytes, checked against the selected existing receipt.
    pub bytes: Vec<u8>,
    /// Durable pre-read attempt for the existing attestation path; not a new signature.
    pub attempt: CustodyOffNucReadAttemptV1,
}
impl ExactObjectClient {
    /// Bind CA/name/pins and the complete finite existing receipt/template selection.
    /// `address` is fixed: no DNS discovery, redirects, proxy, or connection retries.
    /// # Errors
    /// Denies configuration or selected-record mismatches, never falling back to unpinned TLS.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        binding: &ReaderBindingV1,
        seal: &ReaderSealV1,
        identity: TlsIdentity,
        address: SocketAddr,
        server_name: String,
        host: String,
        authority_jcs: &[u8],
        selected: Vec<SelectedRead>,
    ) -> Result<Self, ReaderError> {
        binding.encode()?;
        own_leaf(&identity, &binding.verifier_tls_identity_sha256)?;
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .map_err(|_| ReaderError::Binding)?
                .with_root_certificates(identity.peer_roots)
                .with_client_auth_cert(identity.chain, identity.key)
                .map_err(|_| ReaderError::Binding)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        config.resumption = rustls::client::Resumption::disabled();
        config.enable_early_data = false;
        Ok(Self {
            config: Arc::new(config),
            address,
            name: ServerName::try_from(server_name).map_err(|_| ReaderError::Binding)?,
            server_leaf: binding.frontend_tls_peer_sha256.clone(),
            policy: RequestPolicy::new(binding, seal, host, authority_jcs, selected, &clock_now())?,
        })
    }
    /// Start the exact issued journal row before connecting or transmitting, once only.
    /// Fresh wall-clock validation is repeated after TLS and immediately before transmission.
    /// # Errors
    /// Any mismatch/loss/timeout denies success; neither journal nor transport retries.
    pub fn read(
        &mut self,
        journal: &CustodyOffNucJournal,
        raw_jcs: &[u8],
        limits: CustodyReadLimits,
    ) -> Result<ExactRead, ObjectServiceError> {
        let deny = || ObjectServiceError::InvalidConfiguration("custody TLS read denied".into());
        let deadline = limits.start().map_err(|_| deny())?;
        if raw_jcs.len() > 1_048_576 {
            return Err(deny());
        }
        let mut request = format!("POST /custody/v1/read-object HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", self.policy.host, raw_jcs.len()).into_bytes();
        request.extend_from_slice(raw_jcs);
        let now = clock_now();
        let decoded =
            http::decode_request(&request, &self.policy.host, &self.policy.authority, &now)
                .map_err(|_| deny())?;
        self.policy
            .claim(&decoded.record.body, &now)
            .map_err(|_| deny())?;
        let body = &decoded.record.body;
        let receipt = &self
            .policy
            .expected
            .get(&body.receipt_jcs_sha256)
            .ok_or_else(deny)?
            .receipt;
        if receipt.content_length > limits.maximum_bytes {
            return Err(deny());
        }
        let expected = ReaderResultV1 {
            schema: "das.custody.reader_result.v1".into(),
            request_sha256: raw_sha256(raw_jcs),
            receipt_jcs_sha256: body.receipt_jcs_sha256.clone(),
            configuration_sha256: receipt.configuration_sha256.clone(),
            ledger_head_sha256: body.ledger_head_sha256.clone(),
            content_length: receipt.content_length,
        };
        journal.perform_pre_read_exact(
            raw_jcs,
            &self.policy.authority,
            &now,
            deadline,
            |attempt, exact| {
                let stream = TcpStream::connect_timeout(
                    &self.address,
                    deadline.remaining().map_err(|_| deny())?,
                )
                .map_err(|_| deny())?;
                let connection = ClientConnection::new(self.config.clone(), self.name.clone())
                    .map_err(|_| deny())?;
                let mut channel =
                    Channel::authenticate(connection.into(), stream, deadline, &self.server_leaf)
                        .map_err(|_| deny())?;
                // Validate actual current time, not the caller's journal timestamp. Same exact bytes.
                let fresh = http::decode_request(
                    &request,
                    &self.policy.host,
                    &self.policy.authority,
                    &clock_now(),
                )
                .map_err(|_| deny())?;
                if fresh.raw_jcs != exact {
                    return Err(deny());
                }
                channel.write_all(&request).map_err(|_| deny())?;
                let maximum = usize::try_from(limits.maximum_bytes).map_err(|_| deny())?;
                let response = channel
                    .collect(|h| http::response_body_length(h, maximum), true)
                    .map_err(|_| deny())?;
                let bytes = http::decode_result(
                    &response,
                    true,
                    &expected,
                    &receipt.content_sha256,
                    maximum,
                )
                .map_err(|_| deny())?;
                http::decode_request(
                    &request,
                    &self.policy.host,
                    &self.policy.authority,
                    &clock_now(),
                )
                .map_err(|_| deny())?;
                deadline.remaining().map_err(|_| deny())?;
                Ok(ExactRead {
                    bytes: bytes.to_vec(),
                    attempt: attempt.clone(),
                })
            },
        )
    }
}
