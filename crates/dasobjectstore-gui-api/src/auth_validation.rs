//! Request validation and confirmation-marker policy for GUI admin routes.

use super::*;

pub(super) fn validate_create_local_group_request(
    request: CreateLocalGroupRequest,
) -> Result<StandaloneLocalGroupAdminDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let group_name = required_field("group_name", request.group_name)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker =
        validate_confirmation_marker(request.dry_run, request.confirmation_marker.as_deref())?;

    Ok(StandaloneLocalGroupAdminDaemonRequest {
        operation: StandaloneLocalGroupOperation::CreateGroup,
        group_name,
        username: None,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    })
}

pub(super) fn validate_assign_local_user_to_group_request(
    request: AssignLocalUserToGroupRequest,
) -> Result<StandaloneLocalGroupAdminDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let group_name = required_field("group_name", request.group_name)?;
    let username = required_field("username", request.username)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker =
        validate_confirmation_marker(request.dry_run, request.confirmation_marker.as_deref())?;

    Ok(StandaloneLocalGroupAdminDaemonRequest {
        operation: StandaloneLocalGroupOperation::AddUserToGroup,
        group_name,
        username: Some(username),
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    })
}

pub(super) fn validate_prepare_enclosure_request(
    request: PrepareEnclosureRequest,
) -> Result<StandaloneEnclosurePrepareDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let ssd_device = required_field("ssd_device", request.ssd_device)?;
    let mount_root = request
        .mount_root
        .map(|value| required_field("mount_root", value))
        .transpose()?
        .unwrap_or_else(|| "/srv/dasobjectstore".to_string());
    reject_known_managed_enclosure_mount_root(&mount_root)?;
    let filesystem = parse_prepare_enclosure_filesystem(request.filesystem.as_deref())?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let owner = request
        .owner
        .map(|value| required_field("owner", value))
        .transpose()?;
    let confirmation_marker = validate_prepare_enclosure_confirmation_marker(
        request.dry_run,
        request.allow_format,
        request.existing_data_acknowledged,
        request.confirmation_marker.as_deref(),
    )?;

    let mut hdd_devices = Vec::new();
    for hdd_device in request.hdd_devices {
        hdd_devices.push(PrepareEnclosureHddDeviceRequest {
            disk_id: required_field("hdd_devices.disk_id", hdd_device.disk_id)?,
            device_path: required_field("hdd_devices.device_path", hdd_device.device_path)?,
        });
    }
    if hdd_devices.is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "unsupported_das",
            "at least one eligible HDD device is required before enclosure preparation can be submitted",
        ));
    }

    Ok(StandaloneEnclosurePrepareDaemonRequest {
        ssd_device,
        hdd_devices,
        mount_root,
        filesystem,
        owner,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        allow_format: request.allow_format,
        existing_data_acknowledged: request.existing_data_acknowledged,
        confirmation_marker,
    })
}

pub(super) fn reject_known_managed_enclosure_mount_root(
    mount_root: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    let mount_root = PathBuf::from(mount_root);
    let ssd_marker = mount_root
        .join("ssd")
        .join(".dasobjectstore")
        .join("device.env");
    let hdd_root = mount_root.join("hdd");
    let hdd_marker_present = fs::read_dir(&hdd_root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path().join(".dasobjectstore").join("device.env"))
        .any(|marker| marker.exists());

    if ssd_marker.exists() || hdd_marker_present {
        return Err(route_error(
            StatusCode::CONFLICT,
            "enclosure_already_managed",
            "enclosure preparation through the Web UI is available only for unprepared DAS enclosures; this mount root is already known to DASObjectStore",
        ));
    }

    Ok(())
}

pub(super) fn validate_create_object_store_request(
    request: CreateObjectStoreRequest,
) -> Result<DaemonCreateObjectStoreRequest, (StatusCode, Json<AuthRouteError>)> {
    let store_id = required_field("store_id", request.store_id)?;
    let store_class = request
        .store_class
        .map(|value| required_field("store_class", value))
        .transpose()?
        .unwrap_or_else(|| "generated_data".to_string());
    let writer_group = required_field("writer_group", request.writer_group)?;
    let reader_group = request
        .reader_group
        .map(|value| required_field("reader_group", value))
        .transpose()?;
    let enclosure_id = request
        .enclosure_id
        .map(|value| required_field("enclosure_id", value))
        .transpose()?
        .ok_or_else(|| {
            route_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "enclosure_id is required for ObjectStore creation",
            )
        })?;
    let ssd_root = request
        .ssd_root
        .map(|value| required_field("ssd_root", value))
        .transpose()?
        .unwrap_or_else(|| "/srv/dasobjectstore/ssd".to_string());
    let object_type = request
        .object_type
        .map(|value| required_field("object_type", value))
        .transpose()?
        .unwrap_or_else(|| "naive".to_string());
    let capacity_behavior = request
        .capacity_behavior
        .map(|value| required_field("capacity_behavior", value))
        .transpose()?
        .unwrap_or_else(|| "backpressure_by_priority".to_string());
    let retention = request
        .retention
        .map(|value| required_field("retention", value))
        .transpose()?
        .unwrap_or_else(|| "retain_until_deleted".to_string());
    let endpoint_export_mode = request
        .endpoint_export_mode
        .map(|value| required_field("endpoint_export_mode", value))
        .transpose()?
        .unwrap_or_else(|| "s3_bucket".to_string());
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker = validate_object_store_create_confirmation_marker(
        request.dry_run,
        request.confirmation_marker.as_deref(),
    )?;
    let bucket = request
        .bucket
        .map(|value| required_field("bucket", value))
        .transpose()?
        .unwrap_or_else(|| derived_object_store_bucket_name(&store_id));

    let request = DaemonCreateObjectStoreRequest {
        store_id,
        store_class,
        required_copies: request.required_copies,
        bucket: Some(bucket),
        reader_group,
        writer_group,
        ssd_root: PathBuf::from(ssd_root),
        object_type,
        enclosure_id: Some(enclosure_id),
        public: request.public,
        writeable: request.writeable.unwrap_or(true),
        capacity_behavior,
        retention,
        endpoint_export_mode,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_objectstore_policy",
            err.to_string(),
        )
    })?;
    Ok(request)
}

pub(super) fn validate_object_store_ingest_policy_request(
    request: ObjectStoreIngestPolicyRequest,
) -> Result<DaemonUpdateObjectStoreIngestPolicyRequest, (StatusCode, Json<AuthRouteError>)> {
    let store_id = required_field("store_id", request.store_id)?;
    let ingest_mode = required_field("ingest_mode", request.ingest_mode)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker = request.confirmation_marker.unwrap_or_default();
    let request = DaemonUpdateObjectStoreIngestPolicyRequest {
        store_id,
        ingest_mode,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_object_store_ingest_policy",
            err.to_string(),
        )
    })?;
    Ok(request)
}

pub(super) fn validate_ingest_control_request(
    request: IngestControlRequest,
) -> Result<StandaloneIngestControlDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let reason = required_field("reason", request.reason)?;
    let confirmation_marker = request.confirmation_marker.unwrap_or_default();
    let action = match request.action {
        IngestControlAction::Pause => DaemonIngestControlAction::Pause,
        IngestControlAction::Throttle => DaemonIngestControlAction::Throttle,
        IngestControlAction::Resume => DaemonIngestControlAction::Resume,
    };
    let daemon_request = DaemonIngestControlRequest {
        action,
        reason,
        dry_run: request.dry_run,
        confirmation_marker,
    };
    daemon_request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_ingest_control",
            error.to_string(),
        )
    })?;
    Ok(StandaloneIngestControlDaemonRequest {
        action: daemon_request.action,
        reason: daemon_request.reason,
        dry_run: daemon_request.dry_run,
        confirmation_marker: daemon_request.confirmation_marker,
    })
}

pub(super) fn validate_endpoint_inventory_upsert_request(
    request: EndpointInventoryUpsertRequest,
) -> Result<DaemonUpsertEndpointInventoryRequest, (StatusCode, Json<AuthRouteError>)> {
    let endpoint_id = required_field("endpoint_id", request.endpoint_id)?;
    let display_name = required_field("display_name", request.display_name)?;
    let object_service_url = required_field("object_service_url", request.object_service_url)?;
    let manager_product_id = required_field("manager_product_id", request.manager_product_id)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker = validate_endpoint_inventory_confirmation_marker(
        request.dry_run,
        request.confirmation_marker.as_deref(),
    )?;

    let mut active_bindings = Vec::new();
    for binding in request.active_bindings {
        active_bindings.push(DaemonEndpointBinding {
            binding_id: required_field("active_bindings.binding_id", binding.binding_id)?,
            governance_domain: required_field(
                "active_bindings.governance_domain",
                binding.governance_domain,
            )?,
            store_id: required_field("active_bindings.store_id", binding.store_id)?,
            readiness: parse_endpoint_binding_readiness(&binding.readiness)?,
        });
    }

    let request = DaemonUpsertEndpointInventoryRequest {
        endpoint_id,
        display_name,
        kind: parse_endpoint_kind(&request.kind)?,
        object_service_url,
        validation: DaemonEndpointValidation {
            state: parse_endpoint_validation_state(&request.validation.state)?,
            checked_at_utc: request
                .validation
                .checked_at_utc
                .map(|value| required_field("validation.checked_at_utc", value))
                .transpose()?,
            message: request
                .validation
                .message
                .map(|value| required_field("validation.message", value))
                .transpose()?,
        },
        manager_product_id,
        active_bindings,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker: Some(confirmation_marker),
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_endpoint_inventory",
            err.to_string(),
        )
    })?;
    Ok(request)
}

pub(super) fn validate_cancel_admin_job_request(
    job_id: String,
    request: CancelAdminJobRequest,
) -> Result<StandaloneAdminJobCancelDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let reason = request
        .reason
        .map(|value| required_field("reason", value))
        .transpose()?;

    Ok(StandaloneAdminJobCancelDaemonRequest {
        job_id: required_field("job_id", job_id)?,
        reason,
    })
}

pub(super) fn required_field(
    field: &'static str,
    value: String,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("{field} must not be blank"),
        ));
    }
    Ok(value)
}

pub(super) fn validate_client_request_id(
    client_request_id: Option<&str>,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    if client_request_id.is_some_and(|value| value.trim().is_empty()) {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_request_id must not be blank",
        ));
    }
    Ok(())
}

pub(super) fn validate_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run {
        return Ok(confirmation_marker
            .unwrap_or(LOCAL_ADMIN_CONFIRMATION_MARKER)
            .to_string());
    }

    if confirmation_marker == Some(LOCAL_ADMIN_CONFIRMATION_MARKER) {
        return Ok(LOCAL_ADMIN_CONFIRMATION_MARKER.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{LOCAL_ADMIN_CONFIRMATION_MARKER}`"),
    ))
}

pub(super) fn validate_prepare_enclosure_confirmation_marker(
    dry_run: bool,
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(ENCLOSURE_PREPARE_CONFIRMATION.to_string());
    }
    if !allow_format {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "format_allowance_required",
            "allow_format must be true before enclosure preparation can be submitted",
        ));
    }
    if !existing_data_acknowledged {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "existing_data_acknowledgement_required",
            "existing_data_acknowledged must be true before enclosure preparation can be submitted",
        ));
    }
    if confirmation_marker == Some(ENCLOSURE_PREPARE_CONFIRMATION) {
        return Ok(ENCLOSURE_PREPARE_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{ENCLOSURE_PREPARE_CONFIRMATION}`"),
    ))
}

pub(super) fn validate_object_store_create_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(OBJECT_STORE_CREATE_CONFIRMATION.to_string());
    }
    if confirmation_marker == Some(OBJECT_STORE_CREATE_CONFIRMATION) {
        return Ok(OBJECT_STORE_CREATE_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{OBJECT_STORE_CREATE_CONFIRMATION}`"),
    ))
}

pub(super) fn validate_endpoint_inventory_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(ENDPOINT_RECORD_CONFIRMATION.to_string());
    }
    if confirmation_marker == Some(ENDPOINT_RECORD_CONFIRMATION) {
        return Ok(ENDPOINT_RECORD_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{ENDPOINT_RECORD_CONFIRMATION}`"),
    ))
}
