use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::store::StoreClass;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuiActionCatalog {
    pub actions: Vec<GuiActionDescriptor>,
}

impl GuiActionCatalog {
    pub fn stable() -> Self {
        Self {
            actions: vec![
                GuiActionDescriptor::new(
                    GuiActionKind::HealthCheck,
                    "Run health check",
                    GuiActionSafety::ReadOnly,
                    &[],
                ),
                GuiActionDescriptor::new(
                    GuiActionKind::ServiceStart,
                    "Start object service",
                    GuiActionSafety::ServiceLifecycle,
                    &["compose_file"],
                ),
                GuiActionDescriptor::new(
                    GuiActionKind::ServiceStop,
                    "Stop object service",
                    GuiActionSafety::ServiceLifecycle,
                    &["compose_file"],
                ),
                GuiActionDescriptor::new(
                    GuiActionKind::PoolImportReadOnly,
                    "Import pool read-only",
                    GuiActionSafety::ReadOnlyImport,
                    &["source_path", "recovery_metadata_dir", "recorded_at_utc"],
                ),
                GuiActionDescriptor::confirmation_required(
                    GuiActionKind::StoreCreate,
                    "Create ObjectStore",
                    GuiActionSafety::ConfigurationMutation,
                    &["store_id", "store_class"],
                ),
                GuiActionDescriptor::confirmation_required(
                    GuiActionKind::StoreConfigure,
                    "Configure ObjectStore policy",
                    GuiActionSafety::ConfigurationMutation,
                    &[
                        "store_id",
                        "store_class",
                        "store_copies",
                        "writer_group",
                        "capacity_behavior",
                        "retention",
                        "endpoint_export_mode",
                    ],
                ),
                GuiActionDescriptor::confirmation_required(
                    GuiActionKind::SubobjectCreate,
                    "Create SubObject",
                    GuiActionSafety::ConfigurationMutation,
                    &["subobject_name", "parent_store_id_or_parent_subobject_name"],
                ),
                GuiActionDescriptor::confirmation_required(
                    GuiActionKind::EnclosurePrepare,
                    "Prepare DAS enclosure",
                    GuiActionSafety::DestructiveStoragePreparation,
                    &[
                        "ssd_device",
                        "hdd_devices",
                        "allow_format",
                        "confirmation_phrase",
                    ],
                ),
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuiActionDescriptor {
    pub kind: GuiActionKind,
    pub label: String,
    pub safety: GuiActionSafety,
    pub required_fields: Vec<String>,
    pub confirmation_required: bool,
}

impl GuiActionDescriptor {
    fn new(
        kind: GuiActionKind,
        label: impl Into<String>,
        safety: GuiActionSafety,
        required_fields: &[&str],
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            safety,
            required_fields: required_fields
                .iter()
                .map(|field| (*field).to_string())
                .collect(),
            confirmation_required: false,
        }
    }

    fn confirmation_required(
        kind: GuiActionKind,
        label: impl Into<String>,
        safety: GuiActionSafety,
        required_fields: &[&str],
    ) -> Self {
        Self {
            confirmation_required: true,
            ..Self::new(kind, label, safety, required_fields)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiActionKind {
    HealthCheck,
    ServiceStart,
    ServiceStop,
    PoolImportReadOnly,
    StoreCreate,
    StoreConfigure,
    SubobjectCreate,
    EnclosurePrepare,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiActionSafety {
    ReadOnly,
    ServiceLifecycle,
    ReadOnlyImport,
    ConfigurationMutation,
    DestructiveStoragePreparation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuiActionPlanRequest {
    pub action: GuiActionKind,
    pub compose_file: Option<PathBuf>,
    pub project_directory: Option<PathBuf>,
    pub source_path: Option<PathBuf>,
    pub recovery_metadata_dir: Option<PathBuf>,
    pub recorded_at_utc: Option<String>,
    pub store_id: Option<String>,
    pub store_class: Option<String>,
    pub store_copies: Option<u8>,
    pub bucket: Option<String>,
    pub writer_group: Option<String>,
    pub ssd_root: Option<PathBuf>,
    pub public: Option<bool>,
    pub writeable: Option<bool>,
    pub capacity_behavior: Option<String>,
    pub retention: Option<String>,
    pub endpoint_export_mode: Option<String>,
    pub subobject_name: Option<String>,
    pub parent_store_id: Option<String>,
    pub parent_subobject_name: Option<String>,
    pub subobject_object_type: Option<String>,
    pub subobject_inherits_object_type: Option<bool>,
    pub subobject_s3_routing: Option<String>,
    pub ssd_device: Option<PathBuf>,
    #[serde(default)]
    pub hdd_devices: Vec<String>,
    pub mount_root: Option<PathBuf>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    #[serde(default)]
    pub allow_format: bool,
    #[serde(default)]
    pub existing_data_acknowledged: bool,
    pub confirmation_phrase: Option<String>,
}

impl Default for GuiActionPlanRequest {
    fn default() -> Self {
        Self {
            action: GuiActionKind::HealthCheck,
            compose_file: None,
            project_directory: None,
            source_path: None,
            recovery_metadata_dir: None,
            recorded_at_utc: None,
            store_id: None,
            store_class: None,
            store_copies: None,
            bucket: None,
            writer_group: None,
            ssd_root: None,
            public: None,
            writeable: None,
            capacity_behavior: None,
            retention: None,
            endpoint_export_mode: None,
            subobject_name: None,
            parent_store_id: None,
            parent_subobject_name: None,
            subobject_object_type: None,
            subobject_inherits_object_type: None,
            subobject_s3_routing: None,
            ssd_device: None,
            hdd_devices: Vec::new(),
            mount_root: None,
            filesystem: None,
            owner: None,
            allow_format: false,
            existing_data_acknowledged: false,
            confirmation_phrase: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuiActionPlan {
    pub action: GuiActionKind,
    pub execution: GuiActionExecution,
    pub argv: Vec<String>,
    pub mutates_pool: bool,
    pub writes_recovery_metadata: bool,
    pub confirmation_required: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiActionExecution {
    PlannedCli,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuiActionPlanError {
    pub action: GuiActionKind,
    pub missing_fields: Vec<String>,
}

pub fn action_catalog() -> GuiActionCatalog {
    GuiActionCatalog::stable()
}

pub fn plan_action(request: GuiActionPlanRequest) -> Result<GuiActionPlan, GuiActionPlanError> {
    match request.action {
        GuiActionKind::HealthCheck => Ok(GuiActionPlan {
            action: request.action,
            execution: GuiActionExecution::PlannedCli,
            argv: strings(["dasobjectstore", "health", "--json"]),
            mutates_pool: false,
            writes_recovery_metadata: false,
            confirmation_required: false,
        }),
        GuiActionKind::ServiceStart => plan_service_lifecycle(request, "up"),
        GuiActionKind::ServiceStop => plan_service_lifecycle(request, "down"),
        GuiActionKind::PoolImportReadOnly => plan_read_only_import(request),
        GuiActionKind::StoreCreate => plan_store_create(request),
        GuiActionKind::StoreConfigure => plan_store_configure(request),
        GuiActionKind::SubobjectCreate => plan_subobject_create(request),
        GuiActionKind::EnclosurePrepare => plan_enclosure_prepare(request),
    }
}

fn plan_service_lifecycle(
    request: GuiActionPlanRequest,
    command: &'static str,
) -> Result<GuiActionPlan, GuiActionPlanError> {
    let Some(compose_file) = request.compose_file else {
        return Err(missing_fields(request.action, &["compose_file"]));
    };

    let mut argv = strings(["dasobjectstore", "service", command, "--compose-file"]);
    argv.push(path_arg(compose_file));
    if let Some(project_directory) = request.project_directory {
        argv.push("--project-directory".to_string());
        argv.push(path_arg(project_directory));
    }

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: false,
        writes_recovery_metadata: false,
        confirmation_required: false,
    })
}

fn plan_store_create(request: GuiActionPlanRequest) -> Result<GuiActionPlan, GuiActionPlanError> {
    let mut missing = Vec::new();
    if request
        .store_id
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("store_id".to_string());
    }
    if request
        .store_class
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("store_class".to_string());
    }
    if !missing.is_empty() {
        return Err(GuiActionPlanError {
            action: request.action,
            missing_fields: missing,
        });
    }
    validate_store_configure_policy(&request)?;

    let mut argv = strings(["dasobjectstore", "store", "create"]);
    argv.push(request.store_id.expect("validated store id"));
    argv.push("--class".to_string());
    argv.push(request.store_class.expect("validated store class"));
    if let Some(copies) = request.store_copies {
        argv.push("--copies".to_string());
        argv.push(copies.to_string());
    }
    if let Some(bucket) = request.bucket {
        argv.push("--bucket".to_string());
        argv.push(bucket);
    }
    if let Some(writer_group) = request.writer_group {
        argv.push("--writer-group".to_string());
        argv.push(writer_group);
    }
    if let Some(ssd_root) = request.ssd_root {
        argv.push("--ssd-root".to_string());
        argv.push(path_arg(ssd_root));
    }
    argv.push("--json".to_string());

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: false,
        writes_recovery_metadata: false,
        confirmation_required: true,
    })
}

fn plan_store_configure(
    request: GuiActionPlanRequest,
) -> Result<GuiActionPlan, GuiActionPlanError> {
    let mut missing = Vec::new();
    if request
        .store_id
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("store_id".to_string());
    }
    if request
        .store_class
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("store_class".to_string());
    }
    if request.store_copies.is_none() {
        missing.push("store_copies".to_string());
    }
    if request
        .writer_group
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("writer_group".to_string());
    }
    if request
        .capacity_behavior
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("capacity_behavior".to_string());
    }
    if request
        .retention
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("retention".to_string());
    }
    if request
        .endpoint_export_mode
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("endpoint_export_mode".to_string());
    }
    if !missing.is_empty() {
        return Err(GuiActionPlanError {
            action: request.action,
            missing_fields: missing,
        });
    }
    validate_store_configure_policy(&request)?;

    let mut argv = strings(["dasobjectstore", "store", "configure"]);
    argv.push(request.store_id.expect("validated store id"));
    argv.push("--class".to_string());
    argv.push(request.store_class.expect("validated store class"));
    argv.push("--copies".to_string());
    argv.push(
        request
            .store_copies
            .expect("validated store copies")
            .to_string(),
    );
    argv.push("--writer-group".to_string());
    argv.push(request.writer_group.expect("validated writer group"));
    argv.push("--capacity-behavior".to_string());
    argv.push(
        request
            .capacity_behavior
            .expect("validated capacity behavior"),
    );
    argv.push("--retention".to_string());
    argv.push(request.retention.expect("validated retention"));
    argv.push("--export-mode".to_string());
    argv.push(
        request
            .endpoint_export_mode
            .expect("validated endpoint export mode"),
    );
    if let Some(public) = request.public {
        argv.push("--public".to_string());
        argv.push(public.to_string());
    }
    if let Some(writeable) = request.writeable {
        argv.push("--writeable".to_string());
        argv.push(writeable.to_string());
    }
    if let Some(ssd_root) = request.ssd_root {
        argv.push("--ssd-root".to_string());
        argv.push(path_arg(ssd_root));
    }
    argv.push("--json".to_string());

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: false,
        writes_recovery_metadata: false,
        confirmation_required: true,
    })
}

fn validate_store_configure_policy(
    request: &GuiActionPlanRequest,
) -> Result<(), GuiActionPlanError> {
    let mut invalid = Vec::new();
    if request
        .store_class
        .as_deref()
        .is_some_and(|value| StoreClass::from_str(value).is_err())
    {
        invalid.push("store_class".to_string());
    }
    if request
        .store_copies
        .is_some_and(|copies| copies == 0 || copies > 3)
    {
        invalid.push("store_copies".to_string());
    }
    if request.capacity_behavior.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "reject_writes"
                | "conservative"
                | "backpressure_by_priority"
                | "balanced"
                | "fill_lowest_fractional_usage"
                | "mark_redownload_required"
                | "reproducible_cache"
        )
    }) {
        invalid.push("capacity_behavior".to_string());
    }
    if request.retention.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "immediate_delete" | "tombstone_then_gc" | "standard" | "retain_until_deleted"
        )
    }) {
        invalid.push("retention".to_string());
    }
    if request
        .endpoint_export_mode
        .as_deref()
        .is_some_and(|value| {
            !matches!(
                value,
                "s3" | "s3_bucket"
                    | "read_only_file_export"
                    | "read_only_export"
                    | "disabled"
                    | "internal_only"
            )
        })
    {
        invalid.push("endpoint_export_mode".to_string());
    }

    if invalid.is_empty() {
        Ok(())
    } else {
        Err(GuiActionPlanError {
            action: request.action,
            missing_fields: invalid,
        })
    }
}

fn plan_subobject_create(
    request: GuiActionPlanRequest,
) -> Result<GuiActionPlan, GuiActionPlanError> {
    let mut missing = Vec::new();
    if request
        .subobject_name
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        missing.push("subobject_name".to_string());
    }

    let parent_store_present = request
        .parent_store_id
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let parent_subobject_present = request
        .parent_subobject_name
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    if parent_store_present == parent_subobject_present {
        missing.push("parent_store_id_or_parent_subobject_name".to_string());
    }

    if !missing.is_empty() {
        return Err(GuiActionPlanError {
            action: request.action,
            missing_fields: missing,
        });
    }
    validate_subobject_review_policy(&request)?;

    let mut argv = strings(["dasobjectstore", "subobject", "create"]);
    argv.push(request.subobject_name.expect("validated SubObject name"));
    if let Some(store_id) = request.parent_store_id {
        argv.push("--store".to_string());
        argv.push(store_id);
    } else if let Some(parent) = request.parent_subobject_name {
        argv.push("--parent".to_string());
        argv.push(parent);
    }
    if let Some(ssd_root) = request.ssd_root {
        argv.push("--ssd-root".to_string());
        argv.push(path_arg(ssd_root));
    }

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: false,
        writes_recovery_metadata: false,
        confirmation_required: true,
    })
}

fn validate_subobject_review_policy(
    request: &GuiActionPlanRequest,
) -> Result<(), GuiActionPlanError> {
    let mut invalid = Vec::new();
    if request
        .subobject_inherits_object_type
        .is_some_and(|inherits| !inherits)
        && request
            .subobject_object_type
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
    {
        invalid.push("subobject_object_type".to_string());
    }
    if request
        .subobject_object_type
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && ObjectType::from_str(value).is_err())
    {
        invalid.push("subobject_object_type".to_string());
    }
    if request
        .subobject_s3_routing
        .as_deref()
        .is_some_and(|value| {
            !matches!(
                value,
                "inherit_parent" | "dedicated_prefix" | "dedicated_bucket" | "disabled"
            )
        })
    {
        invalid.push("subobject_s3_routing".to_string());
    }

    if invalid.is_empty() {
        Ok(())
    } else {
        Err(GuiActionPlanError {
            action: request.action,
            missing_fields: invalid,
        })
    }
}

fn plan_enclosure_prepare(
    request: GuiActionPlanRequest,
) -> Result<GuiActionPlan, GuiActionPlanError> {
    let mut missing = Vec::new();
    if request.ssd_device.is_none() {
        missing.push("ssd_device".to_string());
    }
    if request.hdd_devices.is_empty() {
        missing.push("hdd_devices".to_string());
    }
    if !request.allow_format {
        missing.push("allow_format".to_string());
    }
    if !request.existing_data_acknowledged {
        missing.push("existing_data_acknowledged".to_string());
    }
    if request
        .confirmation_phrase
        .as_ref()
        .is_none_or(|phrase| phrase.trim() != "confirm prepare das")
    {
        missing.push("confirmation_phrase".to_string());
    }
    if !missing.is_empty() {
        return Err(GuiActionPlanError {
            action: request.action,
            missing_fields: missing,
        });
    }

    let mut argv = strings(["dasobjectstore", "disk", "prepare-das", "--ssd-device"]);
    argv.push(path_arg(request.ssd_device.expect("validated SSD device")));
    for hdd_device in request.hdd_devices {
        argv.push("--hdd-device".to_string());
        argv.push(hdd_device);
    }
    if let Some(mount_root) = request.mount_root {
        argv.push("--mount-root".to_string());
        argv.push(path_arg(mount_root));
    }
    if let Some(filesystem) = request.filesystem {
        argv.push("--filesystem".to_string());
        argv.push(filesystem);
    }
    if let Some(owner) = request.owner {
        argv.push("--owner".to_string());
        argv.push(owner);
    }
    if request.allow_format {
        argv.push("--allow-format".to_string());
    }
    if request.existing_data_acknowledged {
        argv.push("--acknowledge-existing-data".to_string());
    }
    argv.push("--confirm".to_string());
    argv.push(
        request
            .confirmation_phrase
            .expect("validated confirmation phrase"),
    );

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: true,
        writes_recovery_metadata: false,
        confirmation_required: true,
    })
}

fn plan_read_only_import(
    request: GuiActionPlanRequest,
) -> Result<GuiActionPlan, GuiActionPlanError> {
    let mut missing = Vec::new();
    if request.source_path.is_none() {
        missing.push("source_path".to_string());
    }
    if request.recovery_metadata_dir.is_none() {
        missing.push("recovery_metadata_dir".to_string());
    }
    if request.recorded_at_utc.is_none() {
        missing.push("recorded_at_utc".to_string());
    }
    if !missing.is_empty() {
        return Err(GuiActionPlanError {
            action: request.action,
            missing_fields: missing,
        });
    }

    let mut argv = strings([
        "dasobjectstore",
        "pool",
        "import",
        "--read-only",
        "--source-path",
    ]);
    argv.push(path_arg(
        request.source_path.expect("validated source path"),
    ));
    argv.push("--recovery-metadata-dir".to_string());
    argv.push(path_arg(
        request
            .recovery_metadata_dir
            .expect("validated recovery metadata dir"),
    ));
    argv.push("--recorded-at-utc".to_string());
    argv.push(request.recorded_at_utc.expect("validated timestamp"));

    Ok(GuiActionPlan {
        action: request.action,
        execution: GuiActionExecution::PlannedCli,
        argv,
        mutates_pool: false,
        writes_recovery_metadata: true,
        confirmation_required: false,
    })
}

fn missing_fields(action: GuiActionKind, fields: &[&str]) -> GuiActionPlanError {
    GuiActionPlanError {
        action,
        missing_fields: fields.iter().map(|field| (*field).to_string()).collect(),
    }
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn path_arg(path: PathBuf) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        action_catalog, plan_action, strings, GuiActionKind, GuiActionPlanRequest, GuiActionSafety,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::{
        create_subobject_definition, SubObjectDefinition, SubObjectParent,
    };
    use std::path::PathBuf;

    #[test]
    fn catalog_lists_safe_web_actions() {
        let catalog = action_catalog();

        assert_eq!(catalog.actions.len(), 8);
        assert_eq!(catalog.actions[0].kind, GuiActionKind::HealthCheck);
        assert_eq!(catalog.actions[1].safety, GuiActionSafety::ServiceLifecycle);
        assert_eq!(
            catalog.actions[3].required_fields,
            ["source_path", "recovery_metadata_dir", "recorded_at_utc"]
        );
        assert_eq!(catalog.actions[4].kind, GuiActionKind::StoreCreate);
        assert_eq!(
            catalog.actions[4].safety,
            GuiActionSafety::ConfigurationMutation
        );
        assert!(catalog.actions[4].confirmation_required);
        assert_eq!(
            catalog.actions[4].required_fields,
            ["store_id", "store_class"]
        );
        assert_eq!(catalog.actions[5].kind, GuiActionKind::StoreConfigure);
        assert_eq!(
            catalog.actions[5].required_fields,
            [
                "store_id",
                "store_class",
                "store_copies",
                "writer_group",
                "capacity_behavior",
                "retention",
                "endpoint_export_mode"
            ]
        );
        assert_eq!(catalog.actions[6].kind, GuiActionKind::SubobjectCreate);
        assert_eq!(
            catalog.actions[6].required_fields,
            ["subobject_name", "parent_store_id_or_parent_subobject_name"]
        );
        assert!(catalog.actions[6].confirmation_required);
        assert_eq!(catalog.actions[7].kind, GuiActionKind::EnclosurePrepare);
        assert_eq!(
            catalog.actions[7].safety,
            GuiActionSafety::DestructiveStoragePreparation
        );
        assert_eq!(
            catalog.actions[7].required_fields,
            [
                "ssd_device",
                "hdd_devices",
                "allow_format",
                "confirmation_phrase"
            ]
        );
        assert!(catalog.actions[6].confirmation_required);
    }

    #[test]
    fn plans_health_check_without_required_inputs() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::HealthCheck,
            ..GuiActionPlanRequest::default()
        })
        .expect("health plan");

        assert_eq!(plan.argv, strings(["dasobjectstore", "health", "--json"]));
        assert!(!plan.mutates_pool);
        assert!(!plan.confirmation_required);
    }

    #[test]
    fn plans_service_start_with_compose_file() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::ServiceStart,
            compose_file: Some(PathBuf::from("/tmp/compose.yaml")),
            project_directory: Some(PathBuf::from("/tmp/project")),
            ..GuiActionPlanRequest::default()
        })
        .expect("service start plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "service",
                "up",
                "--compose-file",
                "/tmp/compose.yaml",
                "--project-directory",
                "/tmp/project"
            ])
        );
        assert!(!plan.mutates_pool);
    }

    #[test]
    fn rejects_service_stop_without_compose_file() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::ServiceStop,
            ..GuiActionPlanRequest::default()
        })
        .expect_err("compose file is required");

        assert_eq!(err.missing_fields, ["compose_file"]);
    }

    #[test]
    fn plans_read_only_import_without_pool_mutation() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::PoolImportReadOnly,
            source_path: Some(PathBuf::from("/Volumes/das")),
            recovery_metadata_dir: Some(PathBuf::from("/tmp/recovered")),
            recorded_at_utc: Some("2026-01-05T00:00:00Z".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("read-only import plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "pool",
                "import",
                "--read-only",
                "--source-path",
                "/Volumes/das",
                "--recovery-metadata-dir",
                "/tmp/recovered",
                "--recorded-at-utc",
                "2026-01-05T00:00:00Z"
            ])
        );
        assert!(!plan.mutates_pool);
        assert!(plan.writes_recovery_metadata);
    }

    #[test]
    fn plans_store_create_with_json_output_and_optional_policy_fields() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreCreate,
            store_id: Some("generated-data".to_string()),
            store_class: Some("generated_data".to_string()),
            store_copies: Some(2),
            bucket: Some("generated-data".to_string()),
            writer_group: Some("mnemosyne".to_string()),
            ssd_root: Some(PathBuf::from("/srv/dasobjectstore/ssd")),
            ..GuiActionPlanRequest::default()
        })
        .expect("store create plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "store",
                "create",
                "generated-data",
                "--class",
                "generated_data",
                "--copies",
                "2",
                "--bucket",
                "generated-data",
                "--writer-group",
                "mnemosyne",
                "--ssd-root",
                "/srv/dasobjectstore/ssd",
                "--json"
            ])
        );
        assert!(!plan.mutates_pool);
        assert!(!plan.writes_recovery_metadata);
        assert!(plan.confirmation_required);
    }

    #[test]
    fn rejects_store_create_without_required_fields() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreCreate,
            ..GuiActionPlanRequest::default()
        })
        .expect_err("store id and class are required");

        assert_eq!(err.missing_fields, ["store_id", "store_class"]);
    }

    #[test]
    fn rejects_store_create_with_invalid_policy_values() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreCreate,
            store_id: Some("generated-data".to_string()),
            store_class: Some("unknown".to_string()),
            store_copies: Some(4),
            bucket: Some("generated-data".to_string()),
            writer_group: Some("mnemosyne".to_string()),
            capacity_behavior: Some("fast".to_string()),
            retention: Some("forever".to_string()),
            endpoint_export_mode: Some("ftp".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("invalid create policy values are rejected");

        assert_eq!(
            err.missing_fields,
            [
                "store_class",
                "store_copies",
                "capacity_behavior",
                "retention",
                "endpoint_export_mode"
            ]
        );
    }

    #[test]
    fn plans_store_configure_with_policy_fields() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreConfigure,
            store_id: Some("generated-data".to_string()),
            store_class: Some("generated_data".to_string()),
            store_copies: Some(2),
            writer_group: Some("mnemosyne".to_string()),
            ssd_root: Some(PathBuf::from("/srv/dasobjectstore/ssd")),
            public: Some(false),
            writeable: Some(true),
            capacity_behavior: Some("backpressure_by_priority".to_string()),
            retention: Some("tombstone_then_gc".to_string()),
            endpoint_export_mode: Some("s3".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("store configure plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "store",
                "configure",
                "generated-data",
                "--class",
                "generated_data",
                "--copies",
                "2",
                "--writer-group",
                "mnemosyne",
                "--capacity-behavior",
                "backpressure_by_priority",
                "--retention",
                "tombstone_then_gc",
                "--export-mode",
                "s3",
                "--public",
                "false",
                "--writeable",
                "true",
                "--ssd-root",
                "/srv/dasobjectstore/ssd",
                "--json"
            ])
        );
        assert!(!plan.mutates_pool);
        assert!(!plan.writes_recovery_metadata);
        assert!(plan.confirmation_required);
    }

    #[test]
    fn rejects_store_configure_without_policy_fields() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreConfigure,
            store_id: Some("generated-data".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("configuration policy fields are required");

        assert_eq!(
            err.missing_fields,
            [
                "store_class",
                "store_copies",
                "writer_group",
                "capacity_behavior",
                "retention",
                "endpoint_export_mode"
            ]
        );
    }

    #[test]
    fn rejects_store_configure_with_invalid_policy_values() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::StoreConfigure,
            store_id: Some("generated-data".to_string()),
            store_class: Some("unknown".to_string()),
            store_copies: Some(4),
            writer_group: Some("mnemosyne".to_string()),
            capacity_behavior: Some("fast".to_string()),
            retention: Some("forever".to_string()),
            endpoint_export_mode: Some("ftp".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("invalid policy values are rejected");

        assert_eq!(
            err.missing_fields,
            [
                "store_class",
                "store_copies",
                "capacity_behavior",
                "retention",
                "endpoint_export_mode"
            ]
        );
    }

    #[test]
    fn plans_top_level_subobject_create() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("Xenognostikon".to_string()),
            parent_store_id: Some("ENA".to_string()),
            ssd_root: Some(PathBuf::from("/srv/dasobjectstore/ssd")),
            ..GuiActionPlanRequest::default()
        })
        .expect("SubObject create plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "subobject",
                "create",
                "Xenognostikon",
                "--store",
                "ENA",
                "--ssd-root",
                "/srv/dasobjectstore/ssd"
            ])
        );
        assert!(!plan.mutates_pool);
        assert!(!plan.writes_recovery_metadata);
        assert!(plan.confirmation_required);
    }

    #[test]
    fn plans_subobject_create_with_review_policy_fields() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("pod5-raw".to_string()),
            parent_store_id: Some("generated-data".to_string()),
            ssd_root: Some(PathBuf::from("/srv/dasobjectstore/ssd")),
            subobject_inherits_object_type: Some(false),
            subobject_object_type: Some("pod5".to_string()),
            subobject_s3_routing: Some("dedicated_prefix".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("SubObject create plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "subobject",
                "create",
                "pod5-raw",
                "--store",
                "generated-data",
                "--ssd-root",
                "/srv/dasobjectstore/ssd"
            ])
        );
        assert!(plan.confirmation_required);
    }

    #[test]
    fn planned_subobject_create_matches_cli_registry_definition_shape() {
        let registry_root = std::env::temp_dir().join(format!(
            "dasobjectstore-subobject-web-parity-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let registry_path = registry_root.join("subobjects.json");
        let store_id = StoreId::new("generated-data").expect("store id");
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("pod5-raw".to_string()),
            parent_store_id: Some(store_id.to_string()),
            ssd_root: Some(PathBuf::from("/srv/dasobjectstore/ssd")),
            subobject_inherits_object_type: Some(false),
            subobject_object_type: Some("pod5".to_string()),
            subobject_s3_routing: Some("dedicated_prefix".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("SubObject create plan");

        let report = create_subobject_definition(
            &registry_path,
            "pod5-raw",
            SubObjectParent::Store {
                store_id: store_id.clone(),
            },
        )
        .expect("CLI registry definition");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "subobject",
                "create",
                "pod5-raw",
                "--store",
                "generated-data",
                "--ssd-root",
                "/srv/dasobjectstore/ssd"
            ])
        );
        assert_eq!(
            report.definition,
            SubObjectDefinition {
                name: "pod5-raw".to_string(),
                store_id,
                parent: SubObjectParent::Store {
                    store_id: StoreId::new("generated-data").expect("store id"),
                },
                path: vec!["pod5-raw".to_string()],
            }
        );
        assert_eq!(report.definition.object_prefix(), "generated-data/pod5-raw");

        let _ = std::fs::remove_dir_all(registry_root);
    }

    #[test]
    fn plans_nested_subobject_create() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("Vervet".to_string()),
            parent_subobject_name: Some("Xenognostikon".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("nested SubObject create plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "subobject",
                "create",
                "Vervet",
                "--parent",
                "Xenognostikon"
            ])
        );
        assert!(plan.confirmation_required);
    }

    #[test]
    fn rejects_subobject_create_with_invalid_review_policy_fields() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("pod5-raw".to_string()),
            parent_store_id: Some("generated-data".to_string()),
            subobject_inherits_object_type: Some(false),
            subobject_object_type: Some("not_a_real_type".to_string()),
            subobject_s3_routing: Some("ftp".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("invalid SubObject review policy rejected");

        assert_eq!(
            err.missing_fields,
            ["subobject_object_type", "subobject_s3_routing"]
        );
    }

    #[test]
    fn rejects_subobject_create_without_required_object_type_override() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("pod5-raw".to_string()),
            parent_store_id: Some("generated-data".to_string()),
            subobject_inherits_object_type: Some(false),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("object type override is required when inheritance is disabled");

        assert_eq!(err.missing_fields, ["subobject_object_type"]);
    }

    #[test]
    fn rejects_subobject_create_without_required_fields() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            ..GuiActionPlanRequest::default()
        })
        .expect_err("SubObject name and parent are required");

        assert_eq!(
            err.missing_fields,
            ["subobject_name", "parent_store_id_or_parent_subobject_name"]
        );
    }

    #[test]
    fn rejects_subobject_create_with_ambiguous_parent_fields() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::SubobjectCreate,
            subobject_name: Some("Vervet".to_string()),
            parent_store_id: Some("ENA".to_string()),
            parent_subobject_name: Some("Xenognostikon".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("exactly one parent is required");

        assert_eq!(
            err.missing_fields,
            ["parent_store_id_or_parent_subobject_name"]
        );
    }

    #[test]
    fn plans_enclosure_prepare_with_confirmed_devices() {
        let plan = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::EnclosurePrepare,
            ssd_device: Some(PathBuf::from("/dev/disk/by-id/nvme-Samsung_SSD_visual")),
            hdd_devices: vec![
                "qnap-1057=/dev/disk/by-id/usb-qnap-1057".to_string(),
                "qnap-1058=/dev/disk/by-id/usb-qnap-1058".to_string(),
            ],
            mount_root: Some(PathBuf::from("/srv/dasobjectstore")),
            filesystem: Some("xfs".to_string()),
            owner: Some("stephen".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_phrase: Some("confirm prepare das".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect("enclosure prepare plan");

        assert_eq!(
            plan.argv,
            strings([
                "dasobjectstore",
                "disk",
                "prepare-das",
                "--ssd-device",
                "/dev/disk/by-id/nvme-Samsung_SSD_visual",
                "--hdd-device",
                "qnap-1057=/dev/disk/by-id/usb-qnap-1057",
                "--hdd-device",
                "qnap-1058=/dev/disk/by-id/usb-qnap-1058",
                "--mount-root",
                "/srv/dasobjectstore",
                "--filesystem",
                "xfs",
                "--owner",
                "stephen",
                "--allow-format",
                "--acknowledge-existing-data",
                "--confirm",
                "confirm prepare das"
            ])
        );
        assert!(plan.mutates_pool);
        assert!(plan.confirmation_required);
    }

    #[test]
    fn rejects_enclosure_prepare_without_confirmation_phrase() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::EnclosurePrepare,
            ssd_device: Some(PathBuf::from("/dev/nvme0n1")),
            hdd_devices: vec!["qnap-1057=/dev/sda".to_string()],
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_phrase: Some("wrong".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("confirmation phrase is required");

        assert_eq!(err.missing_fields, ["confirmation_phrase"]);
    }

    #[test]
    fn rejects_enclosure_prepare_without_existing_data_acknowledgement() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::EnclosurePrepare,
            ssd_device: Some(PathBuf::from("/dev/nvme0n1")),
            hdd_devices: vec!["qnap-1057=/dev/sda".to_string()],
            allow_format: true,
            confirmation_phrase: Some("confirm prepare das".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("existing data acknowledgement is required");

        assert_eq!(err.missing_fields, ["existing_data_acknowledged"]);
    }

    #[test]
    fn rejects_enclosure_prepare_without_format_allowance() {
        let err = plan_action(GuiActionPlanRequest {
            action: GuiActionKind::EnclosurePrepare,
            ssd_device: Some(PathBuf::from("/dev/nvme0n1")),
            hdd_devices: vec!["qnap-1057=/dev/sda".to_string()],
            existing_data_acknowledged: true,
            confirmation_phrase: Some("confirm prepare das".to_string()),
            ..GuiActionPlanRequest::default()
        })
        .expect_err("format allowance is required");

        assert_eq!(err.missing_fields, ["allow_format"]);
    }
}
