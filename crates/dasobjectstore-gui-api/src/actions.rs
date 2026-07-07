use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
                    "Create or update ObjectStore",
                    GuiActionSafety::ConfigurationMutation,
                    &["store_id", "store_class"],
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
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiActionSafety {
    ReadOnly,
    ServiceLifecycle,
    ReadOnlyImport,
    ConfigurationMutation,
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
    use std::path::PathBuf;

    #[test]
    fn catalog_lists_safe_web_actions() {
        let catalog = action_catalog();

        assert_eq!(catalog.actions.len(), 5);
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
}
