use super::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointsWorkspaceResponse {
    pub inventory: EndpointInventoryResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointInventoryResponse {
    pub schema_version: String,
    pub endpoint_count: usize,
    pub degraded_endpoint_count: usize,
    pub binding_count: usize,
    pub endpoints: Vec<EndpointInventoryItemResponse>,
    pub warnings: Vec<EndpointWarningResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointInventoryItemResponse {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub manager_product_id: String,
    pub object_service_url: String,
    pub validation: EndpointValidationResponse,
    pub active_bindings: Vec<EndpointBindingResponse>,
    pub warnings: Vec<EndpointWarningResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointValidationResponse {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct EndpointConnectionTestRequest {
    pub endpoint_id: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct EndpointConnectionTestResponse {
    pub schema_version: String,
    pub endpoint_id: String,
    pub kind: String,
    pub state: String,
    pub checked_at_utc: String,
    pub duration_ms: u64,
    pub retryable: bool,
    pub evidence: Vec<EndpointConnectionEvidenceResponse>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub struct EndpointConnectionEvidenceResponse {
    pub stage: String,
    pub outcome: String,
    pub code: String,
    pub message: String,
    pub latency_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointBindingResponse {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointWarningResponse {
    pub code: String,
    pub severity: String,
    pub endpoint_id: String,
    pub binding_id: Option<String>,
    pub message: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointInventoryUpsertRequest {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub object_service_url: String,
    pub validation: EndpointValidationUpsertRequest,
    pub manager_product_id: String,
    pub active_bindings: Vec<EndpointBindingUpsertRequest>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointValidationUpsertRequest {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointBindingUpsertRequest {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EndpointInventoryUpsertResponse {
    pub accepted: EndpointInventoryAcceptedResponse,
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub validation_state: String,
    pub registry_path: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EndpointInventoryAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}
