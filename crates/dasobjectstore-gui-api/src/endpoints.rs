use serde::{Deserialize, Serialize};

pub const ENDPOINT_INVENTORY_SCHEMA_VERSION: &str = "dasobjectstore.endpoint_inventory.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointInventoryView {
    pub schema_version: String,
    pub endpoint_count: usize,
    pub degraded_endpoint_count: usize,
    pub binding_count: usize,
    pub endpoints: Vec<EndpointInventoryItemView>,
    pub warnings: Vec<EndpointWarningView>,
}

impl EndpointInventoryView {
    pub fn from_endpoints(endpoints: Vec<EndpointInventoryItemView>) -> Self {
        let endpoint_count = endpoints.len();
        let degraded_endpoint_count = endpoints
            .iter()
            .filter(|endpoint| endpoint.validation.state.is_degraded())
            .count();
        let binding_count = endpoints
            .iter()
            .map(|endpoint| endpoint.active_bindings.len())
            .sum();
        let warnings = endpoints
            .iter()
            .flat_map(|endpoint| endpoint.warnings.clone())
            .collect();

        Self {
            schema_version: ENDPOINT_INVENTORY_SCHEMA_VERSION.to_string(),
            endpoint_count,
            degraded_endpoint_count,
            binding_count,
            endpoints,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointInventoryItemView {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: EndpointKindView,
    pub manager_product_id: String,
    pub object_service_url: String,
    pub validation: EndpointValidationView,
    pub active_bindings: Vec<EndpointBindingView>,
    pub warnings: Vec<EndpointWarningView>,
}

impl EndpointInventoryItemView {
    pub fn new(
        endpoint_id: impl Into<String>,
        display_name: impl Into<String>,
        kind: EndpointKindView,
        object_service_url: impl Into<String>,
        validation: EndpointValidationView,
    ) -> Self {
        let endpoint_id = endpoint_id.into();
        let warnings = endpoint_validation_warnings(&endpoint_id, validation.state);

        Self {
            endpoint_id,
            display_name: display_name.into(),
            kind,
            manager_product_id: "dasobjectstore".to_string(),
            object_service_url: object_service_url.into(),
            validation,
            active_bindings: Vec::new(),
            warnings,
        }
    }

    pub fn with_binding(mut self, binding: EndpointBindingView) -> Self {
        if !binding.readiness.is_ready() {
            self.warnings.push(EndpointWarningView::binding(
                self.endpoint_id.clone(),
                binding.binding_id.clone(),
                binding.readiness,
            ));
        }
        self.active_bindings.push(binding);
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKindView {
    DasobjectstoreDas,
    DasobjectstoreNfs,
    S3Compatible,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointValidationView {
    pub state: EndpointValidationStateView,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

impl EndpointValidationView {
    pub fn new(state: EndpointValidationStateView) -> Self {
        Self {
            state,
            checked_at_utc: None,
            message: None,
        }
    }

    pub fn with_check(
        mut self,
        checked_at_utc: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.checked_at_utc = Some(checked_at_utc.into());
        self.message = Some(message.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointValidationStateView {
    Draft,
    PendingValidation,
    Validated,
    Degraded,
    Rejected,
    Unknown,
}

impl EndpointValidationStateView {
    fn is_degraded(self) -> bool {
        matches!(self, Self::Degraded | Self::Rejected)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointBindingView {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: EndpointBindingReadinessView,
}

impl EndpointBindingView {
    pub fn new(
        binding_id: impl Into<String>,
        governance_domain: impl Into<String>,
        store_id: impl Into<String>,
        readiness: EndpointBindingReadinessView,
    ) -> Self {
        Self {
            binding_id: binding_id.into(),
            governance_domain: governance_domain.into(),
            store_id: store_id.into(),
            readiness,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointBindingReadinessView {
    Ready,
    Degraded,
    Blocked,
}

impl EndpointBindingReadinessView {
    fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointWarningView {
    pub code: String,
    pub severity: EndpointWarningSeverityView,
    pub endpoint_id: String,
    pub binding_id: Option<String>,
    pub message: String,
}

impl EndpointWarningView {
    fn endpoint(
        endpoint_id: impl Into<String>,
        code: impl Into<String>,
        severity: EndpointWarningSeverityView,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            endpoint_id: endpoint_id.into(),
            binding_id: None,
            message: message.into(),
        }
    }

    fn binding(
        endpoint_id: impl Into<String>,
        binding_id: impl Into<String>,
        readiness: EndpointBindingReadinessView,
    ) -> Self {
        let (code, severity, message) = match readiness {
            EndpointBindingReadinessView::Ready => (
                "endpoint_binding_ready",
                EndpointWarningSeverityView::Info,
                "Endpoint binding is active.",
            ),
            EndpointBindingReadinessView::Degraded => (
                "endpoint_binding_degraded",
                EndpointWarningSeverityView::Warning,
                "Endpoint binding is active but degraded.",
            ),
            EndpointBindingReadinessView::Blocked => (
                "endpoint_binding_blocked",
                EndpointWarningSeverityView::Critical,
                "Endpoint binding is blocked until endpoint validation succeeds.",
            ),
        };

        Self {
            code: code.to_string(),
            severity,
            endpoint_id: endpoint_id.into(),
            binding_id: Some(binding_id.into()),
            message: message.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointWarningSeverityView {
    Info,
    Warning,
    Critical,
}

fn endpoint_validation_warnings(
    endpoint_id: &str,
    state: EndpointValidationStateView,
) -> Vec<EndpointWarningView> {
    match state {
        EndpointValidationStateView::Draft | EndpointValidationStateView::PendingValidation => {
            vec![EndpointWarningView::endpoint(
                endpoint_id,
                "endpoint_validation_required",
                EndpointWarningSeverityView::Warning,
                "Endpoint must be validated before new Mneion bindings are trusted.",
            )]
        }
        EndpointValidationStateView::Degraded => vec![EndpointWarningView::endpoint(
            endpoint_id,
            "endpoint_degraded",
            EndpointWarningSeverityView::Warning,
            "Endpoint is degraded; existing bindings should be reviewed.",
        )],
        EndpointValidationStateView::Rejected => vec![EndpointWarningView::endpoint(
            endpoint_id,
            "endpoint_rejected",
            EndpointWarningSeverityView::Critical,
            "Endpoint validation failed and bindings should remain blocked.",
        )],
        EndpointValidationStateView::Unknown => vec![EndpointWarningView::endpoint(
            endpoint_id,
            "endpoint_validation_unknown",
            EndpointWarningSeverityView::Warning,
            "Endpoint validation status is unknown.",
        )],
        EndpointValidationStateView::Validated => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EndpointBindingReadinessView, EndpointBindingView, EndpointInventoryItemView,
        EndpointInventoryView, EndpointKindView, EndpointValidationStateView,
        EndpointValidationView, EndpointWarningSeverityView, ENDPOINT_INVENTORY_SCHEMA_VERSION,
    };

    #[test]
    fn builds_endpoint_inventory_summary() {
        let validated = EndpointInventoryItemView::new(
            "endpoint-das",
            "Lab DAS",
            EndpointKindView::DasobjectstoreDas,
            "https://127.0.0.1:9443",
            EndpointValidationView::new(EndpointValidationStateView::Validated),
        )
        .with_binding(EndpointBindingView::new(
            "binding-a",
            "synoptikon-dev",
            "raw-public",
            EndpointBindingReadinessView::Ready,
        ));
        let degraded = EndpointInventoryItemView::new(
            "endpoint-nfs",
            "NAS archive",
            EndpointKindView::DasobjectstoreNfs,
            "https://nas.example.test:9443",
            EndpointValidationView::new(EndpointValidationStateView::Degraded),
        )
        .with_binding(EndpointBindingView::new(
            "binding-b",
            "mneion",
            "derived",
            EndpointBindingReadinessView::Blocked,
        ));

        let view = EndpointInventoryView::from_endpoints(vec![validated, degraded]);

        assert_eq!(view.schema_version, ENDPOINT_INVENTORY_SCHEMA_VERSION);
        assert_eq!(view.endpoint_count, 2);
        assert_eq!(view.degraded_endpoint_count, 1);
        assert_eq!(view.binding_count, 2);
        assert_eq!(view.warnings[0].code, "endpoint_degraded");
        assert_eq!(view.warnings[1].code, "endpoint_binding_blocked");
    }

    #[test]
    fn serializes_endpoint_inventory_for_gui_contract() {
        let endpoint = EndpointInventoryItemView::new(
            "endpoint-nfs",
            "NAS archive",
            EndpointKindView::DasobjectstoreNfs,
            "https://nas.example.test:9443",
            EndpointValidationView::new(EndpointValidationStateView::PendingValidation)
                .with_check("2026-07-06T10:30:00Z", "Awaiting runtime probe."),
        );
        let view = EndpointInventoryView::from_endpoints(vec![endpoint]);

        let encoded = serde_json::to_value(view).expect("endpoint inventory serializes");

        assert_eq!(encoded["endpoints"][0]["kind"], "dasobjectstore_nfs");
        assert_eq!(
            encoded["endpoints"][0]["validation"]["state"],
            "pending_validation"
        );
        assert_eq!(encoded["warnings"][0]["severity"], "warning");
    }

    #[test]
    fn does_not_expose_raw_nfs_paths_in_gui_contract() {
        let endpoint = EndpointInventoryItemView::new(
            "endpoint-nfs",
            "NAS archive",
            EndpointKindView::DasobjectstoreNfs,
            "https://nas.example.test:9443",
            EndpointValidationView::new(EndpointValidationStateView::Validated),
        );
        let view = EndpointInventoryView::from_endpoints(vec![endpoint]);

        let encoded = serde_json::to_string(&view).expect("endpoint inventory serializes");

        assert!(!encoded.contains("nfs_server"));
        assert!(!encoded.contains("nfs_export_path"));
        assert!(!encoded.contains("mount_path"));
    }

    #[test]
    fn rejected_endpoint_raises_critical_warning() {
        let endpoint = EndpointInventoryItemView::new(
            "endpoint-bad",
            "Bad endpoint",
            EndpointKindView::S3Compatible,
            "https://bad.example.test:9443",
            EndpointValidationView::new(EndpointValidationStateView::Rejected),
        );

        assert_eq!(endpoint.warnings[0].code, "endpoint_rejected");
        assert_eq!(
            endpoint.warnings[0].severity,
            EndpointWarningSeverityView::Critical
        );
    }
}
