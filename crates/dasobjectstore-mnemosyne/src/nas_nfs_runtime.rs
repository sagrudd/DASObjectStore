use crate::{NasNfsEndpointValidationStatus, ValidatedNasNfsEndpointDefinition};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION: &str =
    "dasobjectstore.nas_nfs_runtime_validation_plan.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NasNfsMountMode {
    ReadOnlyProbe,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NasNfsMountScope {
    RuntimeValidationOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NasNfsRuntimeProbeStep {
    ResolveServer,
    MountReadOnly,
    VerifyExportRoot,
    ProbeObjectService,
    ConfirmObjectContractBoundary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NasNfsRuntimeValidationPlan {
    pub schema_version: String,
    pub endpoint_id: String,
    pub display_name: String,
    pub endpoint_status: NasNfsEndpointValidationStatus,
    pub mount_probe: NasNfsMountProbePlan,
    pub object_service_probe: NasNfsObjectServiceProbePlan,
    pub tenant_contract: NasNfsTenantContractBoundary,
    pub required_steps: Vec<NasNfsRuntimeProbeStep>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NasNfsMountProbePlan {
    pub mount_mode: NasNfsMountMode,
    pub mount_scope: NasNfsMountScope,
    pub nfs_server: String,
    pub nfs_export_path: String,
    pub credential_reference: String,
    pub ephemeral_mount_root: String,
    pub required_mount_options: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NasNfsObjectServiceProbePlan {
    pub object_service_endpoint: String,
    pub tls_ca_reference: Option<String>,
    pub tls_server_name: Option<String>,
    pub expected_health_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NasNfsTenantContractBoundary {
    pub endpoint_id: String,
    pub object_service_endpoint: String,
    pub object_contract: String,
    pub raw_nfs_paths_are_tenant_facing: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NasNfsRuntimeValidationPlanError {
    RejectedEndpoint,
}

impl Display for NasNfsRuntimeValidationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RejectedEndpoint => {
                formatter.write_str("cannot build runtime validation plan for rejected endpoint")
            }
        }
    }
}

impl std::error::Error for NasNfsRuntimeValidationPlanError {}

pub fn plan_nas_nfs_runtime_validation(
    validated: &ValidatedNasNfsEndpointDefinition,
) -> Result<NasNfsRuntimeValidationPlan, NasNfsRuntimeValidationPlanError> {
    let definition = &validated.definition;
    if definition.status == NasNfsEndpointValidationStatus::Rejected {
        return Err(NasNfsRuntimeValidationPlanError::RejectedEndpoint);
    }

    Ok(NasNfsRuntimeValidationPlan {
        schema_version: NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION.to_string(),
        endpoint_id: definition.identifier.clone(),
        display_name: definition.display_name.clone(),
        endpoint_status: definition.status,
        mount_probe: NasNfsMountProbePlan {
            mount_mode: NasNfsMountMode::ReadOnlyProbe,
            mount_scope: NasNfsMountScope::RuntimeValidationOnly,
            nfs_server: definition.nfs_server.clone(),
            nfs_export_path: definition.nfs_export_path.clone(),
            credential_reference: definition.credential_reference.clone(),
            ephemeral_mount_root: format!("/run/dasobjectstore/nas-nfs/{}", definition.identifier),
            required_mount_options: vec![
                "ro".to_string(),
                "nosuid".to_string(),
                "nodev".to_string(),
                "noexec".to_string(),
            ],
        },
        object_service_probe: NasNfsObjectServiceProbePlan {
            object_service_endpoint: definition.object_service_endpoint.clone(),
            tls_ca_reference: definition.tls_ca_reference.clone(),
            tls_server_name: definition.tls_server_name.clone(),
            expected_health_path: "/health".to_string(),
        },
        tenant_contract: NasNfsTenantContractBoundary {
            endpoint_id: definition.identifier.clone(),
            object_service_endpoint: definition.object_service_endpoint.clone(),
            object_contract: "object_style".to_string(),
            raw_nfs_paths_are_tenant_facing: false,
        },
        required_steps: vec![
            NasNfsRuntimeProbeStep::ResolveServer,
            NasNfsRuntimeProbeStep::MountReadOnly,
            NasNfsRuntimeProbeStep::VerifyExportRoot,
            NasNfsRuntimeProbeStep::ProbeObjectService,
            NasNfsRuntimeProbeStep::ConfirmObjectContractBoundary,
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::{
        plan_nas_nfs_runtime_validation, NasNfsMountMode, NasNfsMountScope, NasNfsRuntimeProbeStep,
        NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION,
    };
    use crate::{
        validate_nas_nfs_endpoint_definition, NasNfsEndpointDefinition,
        NasNfsEndpointValidationStatus, NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION,
    };

    const ENDPOINT_UUID: &str = "ad255a8f-0058-4790-a640-758c573f2db1";

    #[test]
    fn plans_read_only_runtime_mount_probe_for_validated_nas_endpoint() {
        let validated =
            validate_nas_nfs_endpoint_definition(&valid_definition()).expect("endpoint validates");

        let plan = plan_nas_nfs_runtime_validation(&validated).expect("plan builds");

        assert_eq!(
            plan.schema_version,
            NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION
        );
        assert_eq!(plan.endpoint_id, ENDPOINT_UUID);
        assert_eq!(plan.mount_probe.mount_mode, NasNfsMountMode::ReadOnlyProbe);
        assert_eq!(
            plan.mount_probe.mount_scope,
            NasNfsMountScope::RuntimeValidationOnly
        );
        assert_eq!(plan.mount_probe.nfs_server, "nas-01.local");
        assert_eq!(plan.mount_probe.nfs_export_path, "/exports/bioinformatics");
        assert_eq!(
            plan.mount_probe.ephemeral_mount_root,
            format!("/run/dasobjectstore/nas-nfs/{ENDPOINT_UUID}")
        );
        assert!(plan
            .mount_probe
            .required_mount_options
            .contains(&"ro".to_string()));
        assert!(plan
            .required_steps
            .contains(&NasNfsRuntimeProbeStep::MountReadOnly));
    }

    #[test]
    fn keeps_raw_nfs_paths_out_of_tenant_contract_boundary() {
        let validated =
            validate_nas_nfs_endpoint_definition(&valid_definition()).expect("endpoint validates");

        let plan = plan_nas_nfs_runtime_validation(&validated).expect("plan builds");
        let tenant_contract =
            serde_json::to_value(plan.tenant_contract).expect("tenant contract serializes");

        assert_eq!(tenant_contract["endpoint_id"], ENDPOINT_UUID);
        assert_eq!(tenant_contract["object_contract"], "object_style");
        assert_eq!(tenant_contract["raw_nfs_paths_are_tenant_facing"], false);
        assert!(tenant_contract.get("nfs_server").is_none());
        assert!(tenant_contract.get("nfs_export_path").is_none());
    }

    #[test]
    fn serializes_runtime_probe_contract_with_internal_mount_scope() {
        let validated =
            validate_nas_nfs_endpoint_definition(&valid_definition()).expect("endpoint validates");

        let encoded =
            serde_json::to_value(plan_nas_nfs_runtime_validation(&validated).expect("plan builds"))
                .expect("plan serializes");

        assert_eq!(
            encoded["schema_version"],
            NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION
        );
        assert_eq!(
            encoded["mount_probe"]["mount_scope"],
            "runtime_validation_only"
        );
        assert_eq!(
            encoded["object_service_probe"]["tls_ca_reference"],
            "secret://dasobjectstore/ca/nas"
        );
        assert_eq!(
            encoded["required_steps"][4],
            "confirm_object_contract_boundary"
        );
    }

    fn valid_definition() -> NasNfsEndpointDefinition {
        NasNfsEndpointDefinition {
            schema_version: NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION.to_string(),
            identifier: ENDPOINT_UUID.to_string(),
            display_name: "Shared NAS".to_string(),
            nfs_server: "nas-01.local".to_string(),
            nfs_export_path: "/exports/bioinformatics".to_string(),
            object_service_endpoint: "https://nas-gateway.local:3900".to_string(),
            credential_reference: "secret://dasobjectstore/nas/shared".to_string(),
            tls_ca_reference: Some("secret://dasobjectstore/ca/nas".to_string()),
            tls_server_name: Some("nas-gateway.local".to_string()),
            status: NasNfsEndpointValidationStatus::Validated,
        }
    }
}
