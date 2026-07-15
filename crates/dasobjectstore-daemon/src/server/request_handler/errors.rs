use super::*;

#[derive(Debug)]
pub(super) enum RemoteEasyconnectRenewalDispatchError {
    InvalidRequest { message: String },
    InvalidClock { value: String },
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
}

impl Display for RemoteEasyconnectRenewalDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message } => formatter.write_str(message),
            Self::InvalidClock { value } => write!(
                formatter,
                "daemon clock value {value} is not a supported UTC timestamp"
            ),
            Self::SessionStore(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RemoteEasyconnectRenewalDispatchError {}

#[derive(Debug)]
pub(super) enum RemoteEasyconnectStoreInventoryError {
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
    MissingWriterGroup {
        object_store: String,
    },
    StoreNotRemoteWritable {
        object_store: String,
        export_policy: String,
    },
}

impl Display for RemoteEasyconnectStoreInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionStore(error) => Display::fmt(error, formatter),
            Self::MissingWriterGroup { object_store } => write!(
                formatter,
                "ObjectStore {object_store} cannot be listed for remote upload because it has no writer group"
            ),
            Self::StoreNotRemoteWritable {
                object_store,
                export_policy,
            } => write!(
                formatter,
                "ObjectStore {object_store} cannot be listed for remote upload because export policy {export_policy} is not S3"
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectStoreInventoryError {}

impl From<RemoteEasyconnectPairedSessionStoreError> for RemoteEasyconnectStoreInventoryError {
    fn from(error: RemoteEasyconnectPairedSessionStoreError) -> Self {
        Self::SessionStore(error)
    }
}

#[derive(Debug)]
pub(super) enum RemoteEasyconnectExchangeDispatchError {
    InvalidRequest { message: String },
    InvalidClock { value: String },
    PairingStore(RemoteEasyconnectPairingStoreError),
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
    ObjectService(ObjectServiceError),
}

impl Display for RemoteEasyconnectExchangeDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message } => formatter.write_str(message),
            Self::InvalidClock { value } => write!(
                formatter,
                "daemon clock value {value} is not a supported UTC timestamp"
            ),
            Self::PairingStore(error) => Display::fmt(error, formatter),
            Self::SessionStore(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RemoteEasyconnectExchangeDispatchError {}

#[derive(Debug)]
pub(super) enum IngestAuthorizationFailure {
    Authorization(DaemonAuthorizationError),
    ObjectService(ObjectServiceError),
    UnknownEndpoint {
        endpoint: StoreId,
        store_registry_path: PathBuf,
        subobject_registry_path: PathBuf,
    },
    AmbiguousEndpoint {
        endpoint: StoreId,
    },
    MissingStore {
        store_id: StoreId,
        store_registry_path: PathBuf,
    },
}

impl IngestAuthorizationFailure {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::Authorization(_) => "permission_denied",
            Self::ObjectService(_)
            | Self::UnknownEndpoint { .. }
            | Self::AmbiguousEndpoint { .. }
            | Self::MissingStore { .. } => "ingest_authorization_failed",
        }
    }
}

impl Display for IngestAuthorizationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
            Self::UnknownEndpoint {
                endpoint,
                store_registry_path,
                subobject_registry_path,
            } => write!(
                formatter,
                "ingest endpoint {endpoint} was not found in {} or {}",
                store_registry_path.display(),
                subobject_registry_path.display()
            ),
            Self::AmbiguousEndpoint { endpoint } => write!(
                formatter,
                "ingest endpoint {endpoint} is ambiguous; both an object store and a SubObject use that name"
            ),
            Self::MissingStore {
                store_id,
                store_registry_path,
            } => write!(
                formatter,
                "SubObject authorization references missing store {store_id} in {}",
                store_registry_path.display()
            ),
        }
    }
}

impl From<DaemonAuthorizationError> for IngestAuthorizationFailure {
    fn from(error: DaemonAuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

impl From<ObjectServiceError> for IngestAuthorizationFailure {
    fn from(error: ObjectServiceError) -> Self {
        Self::ObjectService(error)
    }
}

#[derive(Debug)]
pub(super) enum ApplianceTelemetryAccessFailure {
    MissingActor,
    ReadState { path: PathBuf, message: String },
    InvalidState { path: PathBuf, message: String },
}

impl ApplianceTelemetryAccessFailure {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::MissingActor => "permission_denied",
            Self::ReadState { .. } | Self::InvalidState { .. } => {
                "appliance_telemetry_state_failed"
            }
        }
    }
}

impl Display for ApplianceTelemetryAccessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActor => formatter
                .write_str("authenticated daemon actor is required to read appliance telemetry"),
            Self::ReadState { path, message } => write!(
                formatter,
                "read appliance telemetry state {}: {message}",
                path.display()
            ),
            Self::InvalidState { path, message } => write!(
                formatter,
                "parse appliance telemetry state {}: {message}",
                path.display()
            ),
        }
    }
}

#[derive(Debug)]
pub(super) enum ObjectBrowserAccessFailure {
    MissingActor,
    DelegationNotAllowed {
        peer_actor: String,
    },
    Authorization(DaemonAuthorizationError),
    ObjectService(ObjectServiceError),
    Endpoint(IngestAuthorizationFailure),
    MissingStore {
        store_id: StoreId,
        store_registry_path: PathBuf,
    },
}

impl ObjectBrowserAccessFailure {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::MissingActor | Self::DelegationNotAllowed { .. } | Self::Authorization(_) => {
                "permission_denied"
            }
            Self::ObjectService(_) | Self::Endpoint(_) | Self::MissingStore { .. } => {
                "object_browser_authorization_failed"
            }
        }
    }
}

impl Display for ObjectBrowserAccessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActor => formatter
                .write_str("authenticated daemon actor is required to browse ObjectStore metadata"),
            Self::DelegationNotAllowed { peer_actor } => write!(
                formatter,
                "actor {peer_actor} is not authorized to delegate ObjectStore browser access"
            ),
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
            Self::Endpoint(error) => Display::fmt(error, formatter),
            Self::MissingStore {
                store_id,
                store_registry_path,
            } => write!(
                formatter,
                "ObjectBrowser authorization references missing store {store_id} in {}",
                store_registry_path.display()
            ),
        }
    }
}

impl From<DaemonAuthorizationError> for ObjectBrowserAccessFailure {
    fn from(error: DaemonAuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

impl From<ObjectServiceError> for ObjectBrowserAccessFailure {
    fn from(error: ObjectServiceError) -> Self {
        Self::ObjectService(error)
    }
}
