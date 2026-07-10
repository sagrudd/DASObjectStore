use super::{route_error, AuthRouteError};
use axum::{http::StatusCode, Json};
use dasobjectstore_daemon::{
    DaemonEndpointBindingReadiness, DaemonEndpointKind, DaemonEndpointValidationState,
    PrepareEnclosureFilesystem as DaemonPrepareEnclosureFilesystem,
};

pub(super) fn derived_object_store_bucket_name(store_id: &str) -> String {
    store_id
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub(super) fn parse_prepare_enclosure_filesystem(
    value: Option<&str>,
) -> Result<DaemonPrepareEnclosureFilesystem, (StatusCode, Json<AuthRouteError>)> {
    match value.unwrap_or("ext4").trim().to_ascii_lowercase().as_str() {
        "ext4" => Ok(DaemonPrepareEnclosureFilesystem::Ext4),
        "xfs" => Ok(DaemonPrepareEnclosureFilesystem::Xfs),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("filesystem must be ext4 or xfs: {other}"),
        )),
    }
}

pub(super) fn parse_endpoint_kind(
    value: &str,
) -> Result<DaemonEndpointKind, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "dasobjectstore_das" => Ok(DaemonEndpointKind::DasobjectstoreDas),
        "dasobjectstore_nfs" => Ok(DaemonEndpointKind::DasobjectstoreNfs),
        "s3_compatible" => Ok(DaemonEndpointKind::S3Compatible),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!(
                "kind must be dasobjectstore_das, dasobjectstore_nfs, or s3_compatible: {other}"
            ),
        )),
    }
}

pub(super) fn parse_endpoint_validation_state(
    value: &str,
) -> Result<DaemonEndpointValidationState, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "draft" => Ok(DaemonEndpointValidationState::Draft),
        "pending_validation" => Ok(DaemonEndpointValidationState::PendingValidation),
        "validated" => Ok(DaemonEndpointValidationState::Validated),
        "degraded" => Ok(DaemonEndpointValidationState::Degraded),
        "rejected" => Ok(DaemonEndpointValidationState::Rejected),
        "unknown" => Ok(DaemonEndpointValidationState::Unknown),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!(
                "validation.state must be draft, pending_validation, validated, degraded, rejected, or unknown: {other}"
            ),
        )),
    }
}

pub(super) fn parse_endpoint_binding_readiness(
    value: &str,
) -> Result<DaemonEndpointBindingReadiness, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ready" => Ok(DaemonEndpointBindingReadiness::Ready),
        "degraded" => Ok(DaemonEndpointBindingReadiness::Degraded),
        "blocked" => Ok(DaemonEndpointBindingReadiness::Blocked),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("active_bindings.readiness must be ready, degraded, or blocked: {other}"),
        )),
    }
}
