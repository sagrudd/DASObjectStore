use crate::api::{
    DaemonEndpointConnectionEvidence, DaemonEndpointConnectionOutcome,
    DaemonEndpointConnectionStage, DaemonEndpointKind, DaemonEndpointValidation,
    DaemonEndpointValidationState, TestEndpointConnectionResponse, UpsertEndpointInventoryRequest,
    ENDPOINT_CONNECTION_TEST_SCHEMA_VERSION,
};
use crate::runtime::DaemonServiceRuntimeError;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_ENDPOINT_REGISTRY_PATH: &str = "/opt/dasobjectstore/endpoints.json";
pub const ENDPOINT_REGISTRY_ENV: &str = "DASOBJECTSTORE_ENDPOINTS_PATH";
pub const ENDPOINT_REGISTRY_SCHEMA: &str = "dasobjectstore.endpoint_inventory_registry.v1";

static ENDPOINT_REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointRegistryUpsertSummary {
    pub registry_path: PathBuf,
    pub endpoint_id: String,
    pub endpoint_count: usize,
}

pub fn default_endpoint_registry_path() -> PathBuf {
    std::env::var_os(ENDPOINT_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_ENDPOINT_REGISTRY_PATH))
}

pub fn upsert_endpoint_inventory_record(
    path: impl AsRef<Path>,
    request: &UpsertEndpointInventoryRequest,
) -> Result<EndpointRegistryUpsertSummary, DaemonServiceRuntimeError> {
    let _guard = ENDPOINT_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("endpoint registry lock poisoned");
    let path = path.as_ref();
    let mut registry = read_endpoint_registry(path)?;
    registry.upsert(EndpointRegistryEntry::from_request(request));
    if !request.dry_run {
        write_endpoint_registry(path, &registry)?;
    }

    Ok(EndpointRegistryUpsertSummary {
        registry_path: path.to_path_buf(),
        endpoint_id: request.endpoint_id.clone(),
        endpoint_count: registry.endpoints.len(),
    })
}

pub fn test_endpoint_connection(
    path: impl AsRef<Path>,
    endpoint_id: &str,
    checked_at_utc: &str,
) -> Result<TestEndpointConnectionResponse, DaemonServiceRuntimeError> {
    let _guard = ENDPOINT_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("endpoint registry lock poisoned");
    let path = path.as_ref();
    let registry = read_endpoint_registry(path)?;
    let entry = registry
        .endpoints
        .iter()
        .find(|entry| entry.endpoint_id == endpoint_id)
        .cloned()
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("unknown endpoint: {endpoint_id}"),
        })?;
    let started = Instant::now();
    let mut evidence = Vec::new();
    let parsed = reqwest::Url::parse(&entry.object_service_url).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("endpoint URL is invalid: {error}"),
        }
    })?;
    let host =
        parsed
            .host_str()
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "endpoint URL has no host".to_string(),
            })?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    evidence.push(connection_evidence(
        DaemonEndpointConnectionStage::Configuration,
        DaemonEndpointConnectionOutcome::Passed,
        "configuration_valid",
        "stored endpoint configuration is syntactically valid",
        None,
    ));
    let dns_started = Instant::now();
    let addresses = (host, port)
        .to_socket_addrs()
        .map(|items| items.collect::<Vec<_>>());
    let addresses = match addresses {
        Ok(addresses) if !addresses.is_empty() => {
            evidence.push(connection_evidence(
                DaemonEndpointConnectionStage::Dns,
                DaemonEndpointConnectionOutcome::Passed,
                "dns_resolved",
                "endpoint host resolved from the daemon network namespace",
                Some(elapsed_ms(dns_started)),
            ));
            addresses
        }
        Ok(_) | Err(_) => {
            evidence.push(connection_evidence(
                DaemonEndpointConnectionStage::Dns,
                DaemonEndpointConnectionOutcome::Failed,
                "dns_resolution_failed",
                "endpoint host did not resolve from the daemon network namespace",
                Some(elapsed_ms(dns_started)),
            ));
            return persist_test_result(
                path,
                registry,
                entry,
                checked_at_utc,
                started,
                true,
                evidence,
            );
        }
    };
    let tcp_started = Instant::now();
    if !addresses
        .iter()
        .any(|address| TcpStream::connect_timeout(address, Duration::from_secs(3)).is_ok())
    {
        evidence.push(connection_evidence(
            DaemonEndpointConnectionStage::Tcp,
            DaemonEndpointConnectionOutcome::Failed,
            "tcp_connection_failed",
            "no resolved address accepted a bounded TCP connection",
            Some(elapsed_ms(tcp_started)),
        ));
        return persist_test_result(
            path,
            registry,
            entry,
            checked_at_utc,
            started,
            true,
            evidence,
        );
    }
    evidence.push(connection_evidence(
        DaemonEndpointConnectionStage::Tcp,
        DaemonEndpointConnectionOutcome::Passed,
        "tcp_connected",
        "daemon established a bounded TCP connection",
        Some(elapsed_ms(tcp_started)),
    ));
    let http_started = Instant::now();
    let probe_url = endpoint_probe_url(&entry, &parsed);
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .and_then(|client| client.get(probe_url).send());
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            evidence.push(connection_evidence(
                DaemonEndpointConnectionStage::TlsHttp,
                if status.is_server_error() {
                    DaemonEndpointConnectionOutcome::Failed
                } else {
                    DaemonEndpointConnectionOutcome::Passed
                },
                if status.is_server_error() {
                    "http_service_error"
                } else {
                    "http_service_reached"
                },
                &format!("endpoint returned HTTP {}", status.as_u16()),
                Some(elapsed_ms(http_started)),
            ));
            append_kind_evidence(&entry, host, status.as_u16(), &body, &mut evidence);
        }
        Err(error) => evidence.push(connection_evidence(
            DaemonEndpointConnectionStage::TlsHttp,
            DaemonEndpointConnectionOutcome::Failed,
            if error.is_connect() {
                "tls_or_http_connection_failed"
            } else if error.is_timeout() {
                "http_timeout"
            } else {
                "http_protocol_failed"
            },
            "TLS/HTTP service validation failed; inspect daemon logs for transport detail",
            Some(elapsed_ms(http_started)),
        )),
    }
    let retryable = evidence.iter().any(|item| {
        item.outcome == DaemonEndpointConnectionOutcome::Failed
            && matches!(
                item.code.as_str(),
                "dns_resolution_failed" | "tcp_connection_failed" | "http_timeout"
            )
    });
    persist_test_result(
        path,
        registry,
        entry,
        checked_at_utc,
        started,
        retryable,
        evidence,
    )
}

fn append_kind_evidence(
    entry: &EndpointRegistryEntry,
    host: &str,
    status: u16,
    body: &str,
    evidence: &mut Vec<DaemonEndpointConnectionEvidence>,
) {
    match entry.kind {
        DaemonEndpointKind::DasobjectstoreDas => evidence.push(connection_evidence(
            DaemonEndpointConnectionStage::ServiceIdentity,
            if status == 200
                && body.contains("appliance_id")
                && body.contains("dasobjectstore.remote_easyconnect")
            {
                DaemonEndpointConnectionOutcome::Passed
            } else {
                DaemonEndpointConnectionOutcome::Failed
            },
            if status == 200
                && body.contains("appliance_id")
                && body.contains("dasobjectstore.remote_easyconnect")
            {
                "dasobjectstore_identity_verified"
            } else {
                "dasobjectstore_identity_unverified"
            },
            "DASObjectStore EasyConnect discovery identity was checked without accepting browser reachability as evidence",
            None,
        )),
        DaemonEndpointKind::S3Compatible => evidence.push(connection_evidence(
            DaemonEndpointConnectionStage::Authentication,
            if matches!(status, 200 | 401 | 403)
                && (body.contains("ListAllMyBucketsResult")
                    || body.contains("<Error>")
                    || body.contains("<Code>"))
            {
                DaemonEndpointConnectionOutcome::Degraded
            } else {
                DaemonEndpointConnectionOutcome::Failed
            },
            "s3_credential_probe_required",
            "S3 service semantics are reachable; no daemon-custodied endpoint credential is registered for an authenticated bucket probe",
            None,
        )),
        DaemonEndpointKind::DasobjectstoreNfs => {
            let nfs_started = Instant::now();
            let nfs_reachable = (host, 2049)
                .to_socket_addrs()
                .ok()
                .into_iter()
                .flatten()
                .any(|address| {
                    TcpStream::connect_timeout(&address, Duration::from_secs(3)).is_ok()
                });
            evidence.push(connection_evidence(
                DaemonEndpointConnectionStage::NfsExportPolicy,
                if nfs_reachable {
                    DaemonEndpointConnectionOutcome::Degraded
                } else {
                    DaemonEndpointConnectionOutcome::Failed
                },
                if nfs_reachable {
                    "nfs_export_policy_not_attached"
                } else {
                    "nfs_service_unreachable"
                },
                if nfs_reachable {
                    "NFS service is reachable, but this endpoint record has no typed export and host-policy descriptor to validate"
                } else {
                    "NFS service did not accept a bounded connection on port 2049"
                },
                Some(elapsed_ms(nfs_started)),
            ));
        }
    }
}

fn endpoint_probe_url(entry: &EndpointRegistryEntry, parsed: &reqwest::Url) -> reqwest::Url {
    let mut url = parsed.clone();
    match entry.kind {
        DaemonEndpointKind::DasobjectstoreDas => {
            url.set_path("/products/dasobjectstore/api/v1/remote/easyconnect/discovery");
            url.set_query(None);
        }
        DaemonEndpointKind::S3Compatible => {
            url.set_query(Some("list-type=2&max-keys=0"));
        }
        DaemonEndpointKind::DasobjectstoreNfs => {}
    }
    url
}

fn persist_test_result(
    path: &Path,
    mut registry: EndpointRegistryFile,
    entry: EndpointRegistryEntry,
    checked_at_utc: &str,
    started: Instant,
    retryable: bool,
    evidence: Vec<DaemonEndpointConnectionEvidence>,
) -> Result<TestEndpointConnectionResponse, DaemonServiceRuntimeError> {
    let failed = evidence
        .iter()
        .any(|item| item.outcome == DaemonEndpointConnectionOutcome::Failed);
    let degraded = evidence
        .iter()
        .any(|item| item.outcome == DaemonEndpointConnectionOutcome::Degraded);
    let state = if failed {
        DaemonEndpointValidationState::Rejected
    } else if degraded {
        DaemonEndpointValidationState::Degraded
    } else {
        DaemonEndpointValidationState::Validated
    };
    let response = TestEndpointConnectionResponse {
        schema_version: ENDPOINT_CONNECTION_TEST_SCHEMA_VERSION.to_string(),
        endpoint_id: entry.endpoint_id.clone(),
        kind: entry.kind,
        state,
        checked_at_utc: checked_at_utc.to_string(),
        duration_ms: elapsed_ms(started),
        retryable,
        evidence,
    };
    if let Some(stored) = registry
        .endpoints
        .iter_mut()
        .find(|stored| stored.endpoint_id == entry.endpoint_id)
    {
        stored.validation = DaemonEndpointValidation {
            state,
            checked_at_utc: Some(checked_at_utc.to_string()),
            message: response.evidence.last().map(|item| item.message.clone()),
        };
        stored.last_connection_test = Some(response.clone());
    }
    write_endpoint_registry(path, &registry)?;
    Ok(response)
}

fn connection_evidence(
    stage: DaemonEndpointConnectionStage,
    outcome: DaemonEndpointConnectionOutcome,
    code: &str,
    message: &str,
    latency_ms: Option<u64>,
) -> DaemonEndpointConnectionEvidence {
    DaemonEndpointConnectionEvidence {
        stage,
        outcome,
        code: code.to_string(),
        message: message.to_string(),
        latency_ms,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EndpointRegistryFile {
    schema_version: String,
    endpoints: Vec<EndpointRegistryEntry>,
}

impl Default for EndpointRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: ENDPOINT_REGISTRY_SCHEMA.to_string(),
            endpoints: Vec::new(),
        }
    }
}

impl EndpointRegistryFile {
    fn upsert(&mut self, entry: EndpointRegistryEntry) {
        match self
            .endpoints
            .iter_mut()
            .find(|existing| existing.endpoint_id == entry.endpoint_id)
        {
            Some(existing) => {
                let last_connection_test = existing.last_connection_test.clone();
                *existing = entry;
                existing.last_connection_test = last_connection_test;
            }
            None => self.endpoints.push(entry),
        }
        self.endpoints
            .sort_by(|left, right| left.endpoint_id.cmp(&right.endpoint_id));
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct EndpointRegistryEntry {
    endpoint_id: String,
    display_name: String,
    kind: crate::api::DaemonEndpointKind,
    object_service_url: String,
    validation: crate::api::DaemonEndpointValidation,
    manager_product_id: String,
    active_bindings: Vec<crate::api::DaemonEndpointBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_connection_test: Option<TestEndpointConnectionResponse>,
}

impl EndpointRegistryEntry {
    fn from_request(request: &UpsertEndpointInventoryRequest) -> Self {
        Self {
            endpoint_id: request.endpoint_id.clone(),
            display_name: request.display_name.clone(),
            kind: request.kind,
            object_service_url: request.object_service_url.clone(),
            validation: request.validation.clone(),
            manager_product_id: request.manager_product_id.clone(),
            active_bindings: request.active_bindings.clone(),
            last_connection_test: None,
        }
    }
}

fn read_endpoint_registry(path: &Path) -> Result<EndpointRegistryFile, DaemonServiceRuntimeError> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EndpointRegistryFile::default());
        }
        Err(error) => {
            return Err(DaemonServiceRuntimeError::EndpointRegistryIo {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };
    serde_json::from_str(&data).map_err(|error| {
        DaemonServiceRuntimeError::InvalidEndpointRegistryJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })
}

fn write_endpoint_registry(
    path: &Path,
    registry: &EndpointRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: path.to_path_buf(),
            message: "endpoint registry has no parent".to_string(),
        })?;
    fs::create_dir_all(parent).map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })?;
    let data = serde_json::to_vec_pretty(registry).map_err(|error| {
        DaemonServiceRuntimeError::InvalidEndpointRegistryJson {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("endpoints"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: temporary.clone(),
            message: error.to_string(),
        })?;
    file.write_all(&data)
        .and_then(|_| file.sync_all())
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: temporary.clone(),
            message: error.to_string(),
        })?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| {
        DaemonServiceRuntimeError::EndpointRegistryIo {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| DaemonServiceRuntimeError::EndpointRegistryIo {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::{test_endpoint_connection, upsert_endpoint_inventory_record};
    use crate::api::{
        DaemonEndpointKind, DaemonEndpointValidation, DaemonEndpointValidationState,
        UpsertEndpointInventoryRequest, ENDPOINT_RECORD_CONFIRMATION,
    };
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn upserts_endpoint_record_without_overwriting_other_records() {
        let root = temp_root("endpoint-registry-upsert");
        let path = root.join("endpoints.json");

        upsert_endpoint_inventory_record(&path, &request("endpoint-b", "Endpoint B", false))
            .expect("first endpoint upserts");
        upsert_endpoint_inventory_record(&path, &request("endpoint-a", "Endpoint A", false))
            .expect("second endpoint upserts");
        upsert_endpoint_inventory_record(&path, &request("endpoint-b", "Endpoint B2", false))
            .expect("existing endpoint updates");

        let data = fs::read_to_string(&path).expect("registry reads");

        assert!(data.contains("\"schema_version\""));
        assert!(data.contains("Endpoint A"));
        assert!(data.contains("Endpoint B2"));
        assert!(!data.contains("Endpoint B\""));

        let entries = fs::read_dir(path.parent().expect("parent"))
            .expect("read parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert_eq!(entries.len(), 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn concurrent_endpoint_upserts_preserve_both_records() {
        let root = temp_root("endpoint-registry-concurrent");
        let path = root.join("endpoints.json");
        let left_path = path.clone();
        let left = std::thread::spawn(move || {
            upsert_endpoint_inventory_record(&left_path, &request("endpoint-a", "A", false))
                .expect("left endpoint")
        });
        let right_path = path.clone();
        let right = std::thread::spawn(move || {
            upsert_endpoint_inventory_record(&right_path, &request("endpoint-b", "B", false))
                .expect("right endpoint")
        });
        left.join().expect("left joins");
        right.join().expect("right joins");

        let data = fs::read_to_string(&path).expect("registry reads");
        assert!(data.contains("endpoint-a"));
        assert!(data.contains("endpoint-b"));
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn dry_run_does_not_write_registry() {
        let root = temp_root("endpoint-registry-dry-run");
        let path = root.join("endpoints.json");

        let summary = upsert_endpoint_inventory_record(&path, &request("endpoint-a", "A", true))
            .expect("dry run computes");

        assert_eq!(summary.endpoint_count, 1);
        assert!(!path.exists());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn daemon_probe_records_typed_evidence_and_updates_validation() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                if attempt == 1 {
                    let mut request = [0_u8; 1024];
                    let _ = stream.read(&mut request);
                    let body = br#"{"schema_version":"dasobjectstore.remote_easyconnect.discovery.v1","appliance_id":"test-appliance"}"#;
                    stream
                        .write_all(
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
                                .as_bytes(),
                        )
                        .and_then(|_| stream.write_all(body))
                        .expect("respond");
                }
            }
        });
        let root = temp_root("endpoint-registry-probe");
        let path = root.join("endpoints.json");
        let mut endpoint = request("endpoint-a", "A", false);
        endpoint.kind = DaemonEndpointKind::DasobjectstoreDas;
        endpoint.object_service_url = format!("http://{address}");
        upsert_endpoint_inventory_record(&path, &endpoint).expect("endpoint upserts");

        let result = test_endpoint_connection(&path, "endpoint-a", "2026-07-26T12:00:00Z")
            .expect("probe completes");

        assert_eq!(result.state, DaemonEndpointValidationState::Validated);
        assert!(result
            .evidence
            .iter()
            .any(|item| item.code == "tcp_connected"));
        assert!(result
            .evidence
            .iter()
            .any(|item| item.code == "dasobjectstore_identity_verified"));
        let registry = fs::read_to_string(&path).expect("registry reads");
        assert!(registry.contains("2026-07-26T12:00:00Z"));
        server.join().expect("server joins");
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn request(
        endpoint_id: &str,
        display_name: &str,
        dry_run: bool,
    ) -> UpsertEndpointInventoryRequest {
        UpsertEndpointInventoryRequest {
            endpoint_id: endpoint_id.to_string(),
            display_name: display_name.to_string(),
            kind: DaemonEndpointKind::DasobjectstoreNfs,
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: DaemonEndpointValidation {
                state: DaemonEndpointValidationState::Validated,
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: None,
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run,
            client_request_id: None,
            administrator_actor: Some("admin".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-daemon-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
