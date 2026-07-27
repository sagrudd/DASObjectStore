use super::*;

pub(super) fn validate_submit_request(
    request: &SubmitWorkspaceOperationRequest,
) -> Result<(), WorkspaceOperationError> {
    validate_identifier("operation_id", &request.operation_id)?;
    validate_identifier("request_id", &request.request_id)?;
    validate_digest("request_digest", &request.request_digest)?;
    validate_stage(&request.initial_stage)?;
    validate_timestamp("created_at_utc", &request.created_at_utc)?;
    if request.max_attempts == 0 || request.max_attempts > 100 {
        return Err(invalid("max_attempts", "must be between 1 and 100"));
    }
    Ok(())
}

pub(super) fn validate_identifier(
    field: &'static str,
    value: &str,
) -> Result<(), WorkspaceOperationError> {
    if value.trim().is_empty() || value.len() > 255 {
        return Err(invalid(field, "must be non-blank and at most 255 bytes"));
    }
    Ok(())
}

pub(super) fn validate_stage(value: &str) -> Result<(), WorkspaceOperationError> {
    if value.trim().is_empty()
        || value.len() > MAX_STAGE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "stage",
            "must be a path-free identifier of at most 128 bytes",
        ));
    }
    Ok(())
}

pub(super) fn validate_digest(
    field: &'static str,
    value: &str,
) -> Result<(), WorkspaceOperationError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(field, "must be exactly 64 hexadecimal characters"));
    }
    Ok(())
}

pub(super) fn validate_timestamp(
    field: &'static str,
    value: &str,
) -> Result<(), WorkspaceOperationError> {
    if parse_canonical_utc_timestamp_seconds(value).is_none() {
        return Err(invalid(field, "must use canonical UTC whole-second format"));
    }
    Ok(())
}

pub(super) fn timestamp_seconds(value: &str) -> i64 {
    parse_canonical_utc_timestamp_seconds(value)
        .expect("caller must validate canonical timestamps before comparison")
}

pub(super) fn validate_checkpoint_json(value: &str) -> Result<(), WorkspaceOperationError> {
    if value.len() > MAX_CHECKPOINT_JSON_BYTES {
        return Err(invalid("checkpoint_json", "exceeds 64 KiB"));
    }
    let parsed: Value = serde_json::from_str(value)
        .map_err(|error| invalid("checkpoint_json", &format!("invalid JSON: {error}")))?;
    validate_path_free_json(&parsed)
}

pub(super) fn validate_path_free_json(value: &Value) -> Result<(), WorkspaceOperationError> {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let normalized = key.to_ascii_lowercase();
                if ["path", "root", "secret", "password", "token"]
                    .iter()
                    .any(|forbidden| normalized.contains(forbidden))
                {
                    return Err(invalid(
                        "checkpoint_json",
                        "must not contain paths or secret-bearing fields",
                    ));
                }
                validate_path_free_json(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_path_free_json(value)?;
            }
        }
        Value::String(value)
            if value.starts_with('/')
                || value.starts_with("\\\\")
                || value.split(['/', '\\']).any(|part| part == "..") =>
        {
            return Err(invalid(
                "checkpoint_json",
                "must not contain absolute or parent-relative paths",
            ));
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn validate_progress(
    current: &WorkspaceOperationSnapshot,
    completed_bytes: u64,
    completed_units: u64,
) -> Result<(), WorkspaceOperationError> {
    if completed_bytes < current.completed_bytes
        || completed_units < current.completed_units
        || current
            .total_bytes
            .is_some_and(|total| completed_bytes > total)
        || current
            .total_units
            .is_some_and(|total| completed_units > total)
    {
        return Err(invalid(
            "progress",
            "must be monotonic and cannot exceed declared totals",
        ));
    }
    Ok(())
}

pub(super) fn require_generation(
    current: &WorkspaceOperationSnapshot,
    expected: u64,
) -> Result<(), WorkspaceOperationError> {
    if current.generation != expected {
        return Err(WorkspaceOperationError::StaleGeneration {
            expected,
            actual: current.generation,
        });
    }
    Ok(())
}

pub(super) fn require_owned_running(
    current: &WorkspaceOperationSnapshot,
    owner: &str,
    expected_generation: u64,
) -> Result<(), WorkspaceOperationError> {
    require_generation(current, expected_generation)?;
    if current.state != WorkspaceOperationState::Running {
        return Err(WorkspaceOperationError::InvalidState {
            operation_id: current.operation_id.clone(),
            state: current.state,
        });
    }
    if current.lease_owner.as_deref() != Some(owner) {
        return Err(WorkspaceOperationError::LeaseOwnerMismatch {
            operation_id: current.operation_id.clone(),
        });
    }
    Ok(())
}

pub(super) fn parse_kind(value: &str) -> Result<WorkspaceOperationKind, WorkspaceOperationError> {
    match value {
        "provision" => Ok(WorkspaceOperationKind::Provision),
        "materialize" => Ok(WorkspaceOperationKind::Materialize),
        "promote" => Ok(WorkspaceOperationKind::Promote),
        "cleanup" => Ok(WorkspaceOperationKind::Cleanup),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "operation_kind",
            value: value.to_string(),
        }),
    }
}

pub(super) fn parse_state(value: &str) -> Result<WorkspaceOperationState, WorkspaceOperationError> {
    match value {
        "queued" => Ok(WorkspaceOperationState::Queued),
        "running" => Ok(WorkspaceOperationState::Running),
        "retry_wait" => Ok(WorkspaceOperationState::RetryWait),
        "succeeded" => Ok(WorkspaceOperationState::Succeeded),
        "failed" => Ok(WorkspaceOperationState::Failed),
        "needs_review" => Ok(WorkspaceOperationState::NeedsReview),
        "cancelled" => Ok(WorkspaceOperationState::Cancelled),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "operation_state",
            value: value.to_string(),
        }),
    }
}

pub(super) fn parse_disposition(
    value: &str,
) -> Result<WorkspaceRecoveryDisposition, WorkspaceOperationError> {
    match value {
        "retry_idempotent" => Ok(WorkspaceRecoveryDisposition::RetryIdempotent),
        "resume_checkpoint" => Ok(WorkspaceRecoveryDisposition::ResumeCheckpoint),
        "verify_external_effect" => Ok(WorkspaceRecoveryDisposition::VerifyExternalEffect),
        "terminal" => Ok(WorkspaceRecoveryDisposition::Terminal),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "recovery_disposition",
            value: value.to_string(),
        }),
    }
}

pub(super) fn invalid(field: &'static str, reason: &str) -> WorkspaceOperationError {
    WorkspaceOperationError::InvalidRequest {
        field,
        reason: reason.to_string(),
    }
}

pub(super) fn u64_to_i64(value: u64, field: &'static str) -> Result<i64, WorkspaceOperationError> {
    i64::try_from(value).map_err(|_| invalid(field, "exceeds SQLite integer range"))
}

pub(super) fn optional_u64_to_i64(
    value: Option<u64>,
    field: &'static str,
) -> Result<Option<i64>, WorkspaceOperationError> {
    value.map(|value| u64_to_i64(value, field)).transpose()
}

pub(super) fn stored_u64(value: i64, field: &'static str) -> Result<u64, WorkspaceOperationError> {
    u64::try_from(value).map_err(|_| WorkspaceOperationError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}

pub(super) fn stored_optional_u64(
    value: Option<i64>,
    field: &'static str,
) -> Result<Option<u64>, WorkspaceOperationError> {
    value.map(|value| stored_u64(value, field)).transpose()
}

pub(super) fn stored_u32(value: i64, field: &'static str) -> Result<u32, WorkspaceOperationError> {
    u32::try_from(value).map_err(|_| WorkspaceOperationError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}
