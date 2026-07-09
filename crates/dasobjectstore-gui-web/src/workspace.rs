#[cfg(any(target_arch = "wasm32", test))]
use crate::api::BioinformaticsWorkspaceResponse;
#[cfg(target_arch = "wasm32")]
use crate::api::ObjectBrowserResponse;
use crate::api::{
    ActivityWorkspaceResponse, EnclosureDriveSlotResponse, EnclosuresPageResponse,
    HomeDashboardResponse, ObjectBrowserPlacementResponse, ObjectStoresPageResponse,
    UsersGroupsWorkspaceResponse,
};
#[cfg(target_arch = "wasm32")]
use crate::api::{
    AddEnclosureAffordanceResponse, AdminJobCancelRequest, AdminJobCancelResponse,
    AdminJobStatusResponse, AdminJobSummary, AssignLocalUserToGroupRequest,
    CreateLocalGroupRequest, CreateObjectStoreRequest, CreateObjectStoreResponse,
    DasEnclosureCardResponse, DasEnclosureDetailResponse, EnclosurePrepareHddDevice,
    EnclosurePrepareRequest, EnclosurePrepareResponse, GuiActionPlanRequest, GuiActionPlanResponse,
    LocalGroupAdminResponse, ObjectStoreCardResponse,
};
#[cfg(test)]
use crate::api::{
    AdminJobCancelResponse, AdminJobStatusResponse, AdminJobSummary, DasEnclosureCardResponse,
    EnclosurePrepareResponse,
};
#[cfg(any(target_arch = "wasm32", test))]
use crate::api::{ObjectBrowserFileNodeResponse, ObjectBrowserFolderNodeResponse};
use crate::mount::FrontendHost;
#[cfg(target_arch = "wasm32")]
use gloo_timers::callback::Timeout;

pub const HOME_WORKSPACE_ROUTE: &str = "dashboard/home";
pub const ENCLOSURES_WORKSPACE_ROUTE: &str = "dashboard/enclosures";
pub const OBJECTSTORES_WORKSPACE_ROUTE: &str = "dashboard/object-stores";
pub const ACTIVITY_WORKSPACE_ROUTE: &str = crate::activity::ACTIVITY_WORKSPACE_ROUTE;
pub const ENDPOINTS_WORKSPACE_ROUTE: &str = crate::endpoints::ENDPOINTS_WORKSPACE_ROUTE;
pub const BIOINFORMATICS_WORKSPACE_ROUTE: &str = "workspaces/bioinformatics";
pub const USERS_GROUPS_WORKSPACE_ROUTE: &str = crate::users_groups::USERS_GROUPS_WORKSPACE_ROUTE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspacePage {
    Home,
    Enclosures,
    ObjectStores,
    Activity,
    Endpoints,
    UsersGroups,
    Bioinformatics,
}

impl WorkspacePage {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Enclosures => "enclosures",
            Self::ObjectStores => "objectstores",
            Self::Activity => "activity",
            Self::Endpoints => "endpoints",
            Self::UsersGroups => "users-groups",
            Self::Bioinformatics => "bioinformatics",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Activity => "Activity",
            Self::Endpoints => "Endpoints",
            Self::UsersGroups => "Capabilities",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Activity => "Activity",
            Self::Endpoints => "Endpoints",
            Self::UsersGroups => "Local Capability Mapping",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub fn api_path(self, api_base_path: &str) -> String {
        match self {
            Self::Home => home_workspace_api_path(api_base_path),
            Self::Enclosures => enclosures_workspace_api_path(api_base_path),
            Self::ObjectStores => objectstores_workspace_api_path(api_base_path),
            Self::Activity => activity_workspace_api_path(api_base_path),
            Self::Endpoints => endpoints_workspace_api_path(api_base_path),
            Self::UsersGroups => users_groups_workspace_api_path(api_base_path),
            Self::Bioinformatics => bioinformatics_workspace_api_path(api_base_path),
        }
    }
}

pub const PRIMARY_NAVIGATION: [WorkspacePage; 7] = [
    WorkspacePage::Home,
    WorkspacePage::Enclosures,
    WorkspacePage::ObjectStores,
    WorkspacePage::Endpoints,
    WorkspacePage::Activity,
    WorkspacePage::UsersGroups,
    WorkspacePage::Bioinformatics,
];

pub const INTEGRATED_PRIMARY_NAVIGATION: [WorkspacePage; 5] = [
    WorkspacePage::Home,
    WorkspacePage::Enclosures,
    WorkspacePage::ObjectStores,
    WorkspacePage::Activity,
    WorkspacePage::Bioinformatics,
];

pub fn primary_navigation_for_host(host: FrontendHost) -> &'static [WorkspacePage] {
    match host {
        FrontendHost::Standalone => &PRIMARY_NAVIGATION,
        FrontendHost::Monas | FrontendHost::Synoptikon => &INTEGRATED_PRIMARY_NAVIGATION,
    }
}

pub fn home_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        HOME_WORKSPACE_ROUTE
    )
}

pub fn enclosures_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENCLOSURES_WORKSPACE_ROUTE
    )
}

pub fn objectstores_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        OBJECTSTORES_WORKSPACE_ROUTE
    )
}

pub fn activity_workspace_api_path(api_base_path: &str) -> String {
    crate::activity::activity_workspace_api_path(api_base_path)
}

pub fn endpoints_workspace_api_path(api_base_path: &str) -> String {
    crate::endpoints::endpoints_workspace_api_path(api_base_path)
}

pub fn bioinformatics_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        BIOINFORMATICS_WORKSPACE_ROUTE
    )
}

pub fn users_groups_workspace_api_path(api_base_path: &str) -> String {
    crate::users_groups::users_groups_workspace_api_path(api_base_path)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApiLoadState<T> {
    Loading,
    Success(T),
    Empty(String),
    PermissionDenied(String),
    TransportError(String),
    StaleData { value: T, message: String },
}

impl<T> ApiLoadState<T> {
    pub fn success(value: T) -> Self {
        Self::Success(value)
    }

    pub fn empty(message: impl Into<String>) -> Self {
        Self::Empty(message.into())
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied(message.into())
    }

    pub fn transport_error(message: impl Into<String>) -> Self {
        Self::TransportError(message.into())
    }

    pub fn stale_data(value: T, message: impl Into<String>) -> Self {
        Self::StaleData {
            value,
            message: message.into(),
        }
    }

    pub const fn state_name(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Success(_) => "success",
            Self::Empty(_) => "empty",
            Self::PermissionDenied(_) => "permission-denied",
            Self::TransportError(_) => "transport-error",
            Self::StaleData { .. } => "stale-data",
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn page_load_state_from_result<T, F>(
    result: Result<T, crate::api::ApiError>,
    empty_message: F,
) -> ApiLoadState<T>
where
    F: FnOnce(&T) -> Option<String>,
{
    match result {
        Ok(view) => match empty_message(&view) {
            Some(message) => ApiLoadState::empty(message),
            None => ApiLoadState::success(view),
        },
        Err(error) if error.is_permission_denied() => {
            ApiLoadState::permission_denied(error.message)
        }
        Err(error) => ApiLoadState::transport_error(error.message),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardMetric {
    pub label: String,
    pub value: String,
    pub detail: String,
    pub state: String,
}

impl DashboardMetric {
    fn new(label: &str, value: impl Into<String>, detail: impl Into<String>, state: &str) -> Self {
        Self {
            label: label.to_string(),
            value: value.into(),
            detail: detail.into(),
            state: state.to_string(),
        }
    }
}

pub fn home_dashboard_metrics(view: &HomeDashboardResponse) -> Vec<DashboardMetric> {
    vec![
        DashboardMetric::new(
            "Drives",
            view.drives.total.to_string(),
            format!(
                "{} mounted; {} healthy; {} watch; {} suspect; {} failed",
                view.drives.mounted,
                view.drives.healthy,
                view.drives.watch,
                view.drives.suspect,
                view.drives.failed
            ),
            &view.health.label,
        ),
        DashboardMetric::new(
            "DAS enclosures",
            format!("{} mounted", view.mounted_enclosures.len()),
            "Supported enclosure inventory from daemon dashboard API",
            &view.health.label,
        ),
        DashboardMetric::new(
            "Capacity",
            format!("{} TiB free", view.capacity.free_tib),
            format!(
                "{} TiB used of {} TiB total",
                view.capacity.used_tib, view.capacity.total_tib
            ),
            &format!(
                "{:.1}% used",
                f64::from(view.capacity.used_percent_basis_points) / 100.0
            ),
        ),
        DashboardMetric::new(
            "7-day throughput",
            format!("{} TiB ingest", view.throughput_7d.ingest_tib),
            format!(
                "{} MiB/s write avg; {} MiB/s read avg",
                view.throughput_7d.avg_write_mib_s, view.throughput_7d.avg_read_mib_s
            ),
            &format!("{} days", view.throughput_7d.window_days),
        ),
        DashboardMetric::new(
            "Memory stress",
            format!("{}%", view.memory_stress.pressure_percent),
            format!(
                "{}% swap; {} TiB page cache",
                view.memory_stress.swap_used_percent, view.memory_stress.page_cache_tib
            ),
            &view.memory_stress.state,
        ),
        DashboardMetric::new(
            "SMART warnings",
            view.smart_warnings.warning_count.to_string(),
            format!(
                "{} affected drive(s)",
                view.smart_warnings.affected_drive_count
            ),
            if view.smart_warnings.warning_count == 0 {
                "clear"
            } else {
                "review"
            },
        ),
        DashboardMetric::new(
            "ObjectStores",
            view.object_stores.len().to_string(),
            "Registered object stores visible to this appliance",
            &view.health.label,
        ),
    ]
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DashboardAttentionItem {
    pub title: String,
    pub detail: String,
    pub state: String,
}

impl DashboardAttentionItem {
    fn new(title: impl Into<String>, detail: impl Into<String>, state: &str) -> Self {
        Self {
            title: title.into(),
            detail: detail.into(),
            state: state.to_string(),
        }
    }
}

pub fn home_dashboard_attention(view: &HomeDashboardResponse) -> Vec<DashboardAttentionItem> {
    let mut items = Vec::new();
    if view.health.action_count > 0
        || view.health.warning_count > 0
        || view.health.critical_count > 0
    {
        items.push(DashboardAttentionItem::new(
            "Appliance attention",
            format!(
                "{} required action(s), {} warning(s), {} critical condition(s)",
                view.health.action_count, view.health.warning_count, view.health.critical_count
            ),
            &view.health.state,
        ));
    }
    if view.drives.failed > 0 || view.drives.suspect > 0 {
        items.push(DashboardAttentionItem::new(
            "Drive health",
            format!(
                "{} failed drive(s), {} suspect drive(s), {} watch drive(s)",
                view.drives.failed, view.drives.suspect, view.drives.watch
            ),
            if view.drives.failed > 0 {
                "critical"
            } else {
                "warning"
            },
        ));
    }
    if let Some(capacity_item) = capacity_attention_item(view) {
        items.push(capacity_item);
    }
    if let Some(ingest) = &view.ingest {
        if ingest.failed_jobs > 0
            || ingest.active_jobs > 0
            || ingest.queued_jobs > 0
            || ingest.pressure != "normal"
            || !ingest.warnings.is_empty()
        {
            let detail = ingest
                .warnings
                .first()
                .map(|warning| warning.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{} queued, {} active, {} failed ingest job(s).",
                        ingest.queued_jobs, ingest.active_jobs, ingest.failed_jobs
                    )
                });
            items.push(DashboardAttentionItem::new(
                "Ingest queue",
                detail,
                queue_attention_state(&ingest.pressure, ingest.failed_jobs),
            ));
        }
    }
    if let Some(destage) = &view.destage {
        if destage.pending_objects > 0
            || destage.copying_objects > 0
            || !destage.warnings.is_empty()
        {
            let detail = destage
                .warnings
                .first()
                .map(|warning| warning.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{} pending, {} copying, {} verified destage object(s).",
                        destage.pending_objects, destage.copying_objects, destage.verified_objects
                    )
                });
            items.push(DashboardAttentionItem::new(
                "Destage queue",
                detail,
                if destage.warnings.is_empty() {
                    "watch"
                } else {
                    "warning"
                },
            ));
        }
    }
    if let Some(warning) = &view.memory_stress.warning {
        items.push(DashboardAttentionItem::new(
            "Memory stress",
            warning.message.clone(),
            &view.memory_stress.state,
        ));
    }
    for enclosure in view.mounted_enclosures.iter().filter(|enclosure| {
        !enclosure.warnings.is_empty() || !matches_health_clear(&enclosure.health)
    }) {
        let state = if !enclosure.warnings.is_empty() && matches_health_clear(&enclosure.health) {
            "warning"
        } else {
            enclosure.health.as_str()
        };
        let detail = enclosure
            .warnings
            .first()
            .map(|warning| warning.message.clone())
            .unwrap_or_else(|| {
                format!(
                    "{} reports {} health at {}.",
                    enclosure.display_name, enclosure.health, enclosure.mount_path
                )
            });
        items.push(DashboardAttentionItem::new(
            format!("Enclosure {}", enclosure.display_name),
            detail,
            state,
        ));
    }
    for warning in view.smart_warnings.warnings.iter().take(3) {
        items.push(DashboardAttentionItem::new(
            format!("SMART {}", warning.drive_id),
            format!("{}: {}", warning.attribute, warning.message),
            &warning.severity,
        ));
    }
    for store in view.object_stores.iter().filter(|store| {
        !store.warnings.is_empty()
            || !matches_health_clear(&store.health)
            || store.endpoint_export_mode.is_none()
    }) {
        let state = if (!store.warnings.is_empty() || store.endpoint_export_mode.is_none())
            && matches_health_clear(&store.health)
        {
            "warning"
        } else {
            store.health.as_str()
        };
        let detail = store
            .warnings
            .first()
            .map(|warning| warning.message.clone())
            .unwrap_or_else(|| {
                if store.endpoint_export_mode.is_none() {
                    format!(
                        "{} has no daemon-reported object-service export mode.",
                        store.display_name
                    )
                } else {
                    format!(
                        "{} reports {} health with {} object(s).",
                        store.display_name, store.health, store.object_count
                    )
                }
            });
        items.push(DashboardAttentionItem::new(
            format!("ObjectStore {}", store.display_name),
            detail,
            state,
        ));
    }
    if view.mounted_enclosures.is_empty() {
        items.push(DashboardAttentionItem::new(
            "Enclosure inventory",
            "The daemon dashboard payload did not report any mounted supported DAS enclosures.",
            "watch",
        ));
    }
    if view.object_stores.is_empty() {
        items.push(DashboardAttentionItem::new(
            "ObjectStore inventory",
            "The daemon dashboard payload did not report any object stores visible to this user.",
            "watch",
        ));
    }
    if items.is_empty() {
        items.push(DashboardAttentionItem::new(
            "No operator attention required",
            "The daemon dashboard payload reports clear drive, ingest, destage, capacity, memory, SMART, enclosure, and ObjectStore state.",
            "clear",
        ));
    }
    items
}

fn capacity_attention_item(view: &HomeDashboardResponse) -> Option<DashboardAttentionItem> {
    if view.capacity.total_tib == "0.0" && view.health.action_count > 0 {
        return Some(DashboardAttentionItem::new(
            "Capacity telemetry",
            "The daemon dashboard payload has not reported usable capacity yet.",
            "watch",
        ));
    }

    match view.capacity.used_percent_basis_points {
        9000..=u16::MAX => Some(DashboardAttentionItem::new(
            "Capacity pressure",
            format!(
                "{} TiB used of {} TiB total; {} TiB remains free.",
                view.capacity.used_tib, view.capacity.total_tib, view.capacity.free_tib
            ),
            "critical",
        )),
        8000..=8999 => Some(DashboardAttentionItem::new(
            "Capacity pressure",
            format!(
                "{} TiB used of {} TiB total; {} TiB remains free.",
                view.capacity.used_tib, view.capacity.total_tib, view.capacity.free_tib
            ),
            "warning",
        )),
        _ => None,
    }
}

fn matches_health_clear(health: &str) -> bool {
    matches!(health, "healthy" | "nominal" | "clear")
}

fn queue_attention_state(pressure: &str, failed_jobs: usize) -> &'static str {
    if failed_jobs > 0 || pressure == "critical" {
        "critical"
    } else if pressure == "normal" {
        "watch"
    } else {
        "warning"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityCategorySummary {
    pub kind: String,
    pub label: String,
    pub description: String,
    pub active_count: usize,
    pub waiting_count: usize,
    pub failed_count: usize,
    pub complete_count: usize,
    pub state: String,
}

pub fn activity_category_summaries(
    view: &ActivityWorkspaceResponse,
) -> Vec<ActivityCategorySummary> {
    view.categories
        .iter()
        .map(|category| {
            let matching_tasks = view.tasks.iter().filter(|task| task.kind == category.kind);
            let mut active_count = 0;
            let mut waiting_count = 0;
            let mut failed_count = 0;
            let mut complete_count = 0;

            for task in matching_tasks {
                match task.state.as_str() {
                    "failed" => failed_count += 1,
                    "complete" | "cancelled" => complete_count += 1,
                    "queued" | "waiting" => waiting_count += 1,
                    _ => active_count += 1,
                }
            }

            let state = if failed_count > 0 {
                "critical"
            } else if active_count > 0 {
                "running"
            } else if waiting_count > 0 {
                "waiting"
            } else {
                "idle"
            };

            ActivityCategorySummary {
                kind: category.kind.clone(),
                label: category.label.clone(),
                description: category.description.clone(),
                active_count,
                waiting_count,
                failed_count,
                complete_count,
                state: state.to_string(),
            }
        })
        .collect()
}

pub fn activity_queue_summary(view: &ActivityWorkspaceResponse) -> Vec<DashboardMetric> {
    let ingest = view
        .ingest
        .as_ref()
        .map(|ingest| {
            DashboardMetric::new(
                "Ingest",
                format!("{} active", ingest.active_jobs),
                format!(
                    "{} queued; {} failed; pressure {}",
                    ingest.queued_jobs, ingest.failed_jobs, ingest.pressure
                ),
                queue_attention_state(&ingest.pressure, ingest.failed_jobs),
            )
        })
        .unwrap_or_else(|| {
            DashboardMetric::new(
                "Ingest",
                "No queue",
                "No daemon ingest queue payload has been reported.",
                "idle",
            )
        });
    let destage = view
        .destage
        .as_ref()
        .map(|destage| {
            DashboardMetric::new(
                "Destage",
                format!("{} copying", destage.copying_objects),
                format!(
                    "{} pending; {} verified",
                    destage.pending_objects, destage.verified_objects
                ),
                if destage.copying_objects > 0 {
                    "running"
                } else if destage.pending_objects > 0 {
                    "waiting"
                } else {
                    "idle"
                },
            )
        })
        .unwrap_or_else(|| {
            DashboardMetric::new(
                "Destage",
                "No queue",
                "No daemon destage queue payload has been reported.",
                "idle",
            )
        });

    vec![ingest, destage]
}

#[cfg(target_arch = "wasm32")]
fn activity_task_kind_label(kind: &str) -> String {
    match kind {
        "ingest" => "Ingest",
        "destage" => "Destage",
        "system_administration" => "Administrator",
        "enclosure_preparation" => "Enclosure",
        "object_store_creation" => "ObjectStore",
        "sub_object_creation" => "SubObject",
        "repair" => "Repair",
        "health_check" => "Health",
        "disk_drain" => "Disk drain",
        "disk_replace" => "Disk replace",
        "endpoint_validation" => "Endpoint",
        other => other,
    }
    .to_string()
}

#[cfg(target_arch = "wasm32")]
fn activity_task_state_label(state: &str) -> String {
    match state {
        "queued" => "Queued",
        "running" => "Running",
        "waiting" => "Waiting",
        "complete" => "Complete",
        "failed" => "Failed",
        "cancelled" => "Cancelled",
        other => other,
    }
    .to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosureCardSummary {
    pub id: String,
    pub label: String,
    pub name: String,
    pub health: String,
    pub drives: String,
    pub capacity: String,
    pub mount_path: String,
    pub warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosurePrepareCandidate {
    pub enclosure_id: String,
    pub display_name: String,
    pub ssd_devices: Vec<EnclosurePrepareDevice>,
    pub hdd_devices: Vec<EnclosurePrepareDevice>,
}

impl EnclosurePrepareCandidate {
    pub fn ready(&self) -> bool {
        !self.ssd_devices.is_empty() && !self.hdd_devices.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosurePrepareDevice {
    pub disk_id: String,
    pub device_path: String,
    pub label: String,
}

pub fn enclosure_prepare_candidate(
    view: &EnclosuresPageResponse,
    active_id: &str,
) -> Option<EnclosurePrepareCandidate> {
    let enclosure = view
        .enclosures
        .iter()
        .find(|enclosure| enclosure.enclosure_id == active_id)
        .or_else(|| view.enclosures.first())?;
    let detail = view
        .details
        .as_ref()
        .filter(|detail| detail.enclosure_id == enclosure.enclosure_id)?;
    let mut ssd_devices = Vec::new();
    let mut hdd_devices = Vec::new();

    for slot in &detail.slots {
        let Some(device) = prepare_device_from_slot(slot) else {
            continue;
        };
        if slot_is_ssd(slot) {
            ssd_devices.push(device);
        } else if slot_is_hdd(slot) {
            hdd_devices.push(device);
        }
    }

    Some(EnclosurePrepareCandidate {
        enclosure_id: enclosure.enclosure_id.clone(),
        display_name: enclosure.display_name.clone(),
        ssd_devices,
        hdd_devices,
    })
}

fn prepare_device_from_slot(slot: &EnclosureDriveSlotResponse) -> Option<EnclosurePrepareDevice> {
    let device_path = slot
        .device_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())?;
    let role = slot.role.as_deref().unwrap_or("unassigned");
    Some(EnclosurePrepareDevice {
        disk_id: slot.drive_id.clone(),
        device_path: device_path.to_string(),
        label: format!(
            "{} · {} · {} TiB · {}",
            slot.drive_id, role, slot.size_tib, device_path
        ),
    })
}

fn slot_is_ssd(slot: &EnclosureDriveSlotResponse) -> bool {
    slot.role
        .as_deref()
        .is_some_and(|role| role.eq_ignore_ascii_case("ssd"))
        || slot.slot_number == 0
}

fn slot_is_hdd(slot: &EnclosureDriveSlotResponse) -> bool {
    slot.role
        .as_deref()
        .is_some_and(|role| role.to_ascii_lowercase().starts_with("hdd"))
}

pub fn enclosure_card_summaries(view: &EnclosuresPageResponse) -> Vec<EnclosureCardSummary> {
    view.enclosures
        .iter()
        .map(|enclosure| {
            let label = format!(
                "{} / {} / {}",
                enclosure.connection.bus,
                enclosure.connection.protocol,
                enclosure.connection.link_speed
            );
            let drives = format!(
                "{} mounted of {} drive(s); {} healthy; {} watch; {} suspect; {} failed",
                enclosure.drive_count.mounted,
                enclosure.drive_count.total,
                enclosure.drive_count.healthy,
                enclosure.drive_count.watch,
                enclosure.drive_count.suspect,
                enclosure.drive_count.failed
            );
            let capacity = format!(
                "{} TiB free of {} TiB",
                enclosure.capacity.free_tib, enclosure.capacity.total_tib
            );

            EnclosureCardSummary {
                id: enclosure.enclosure_id.clone(),
                label,
                name: enclosure.display_name.clone(),
                health: enclosure.health.clone(),
                drives,
                capacity,
                mount_path: enclosure.mount_path.clone(),
                warning_count: enclosure.warnings.len(),
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectStoreCardSummary {
    pub id: String,
    pub label: String,
    pub name: String,
    pub health: String,
    pub object_type: String,
    pub access: String,
    pub policy: String,
    pub capacity: String,
    pub objects: String,
    pub writer_group: String,
    pub endpoint: String,
    pub warning_count: usize,
    pub last_ingested: String,
    pub writer_policy: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBrowserFolderSummary {
    pub name: String,
    pub prefix: String,
    pub objects: String,
    pub size: String,
    pub readiness: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBrowserFileSummary {
    pub object_id: String,
    pub name: String,
    pub path: String,
    pub object_type: String,
    pub size: String,
    pub modified: String,
    pub readiness: String,
    pub lifecycle: String,
    pub copies: String,
    pub placement_summary: String,
    pub placements: Vec<ObjectBrowserPlacementResponse>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
enum ObjectBrowserDownloadState {
    Idle,
    Starting { label: String },
    Started { filename: String, detail: String },
    PermissionDenied { message: String },
    Error { message: String },
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObjectStoreCreateFormState {
    open: bool,
    store_id: String,
    writer_group: String,
    enclosure_id: String,
    object_type: String,
    required_copies: u8,
    public: bool,
    writeable: bool,
    store_class: String,
    capacity_behavior: String,
    retention: String,
    endpoint_export_mode: String,
    bucket: String,
    ssd_root: String,
    planning: bool,
    plan: Option<GuiActionPlanResponse>,
    confirmation_phrase: String,
    submitting: bool,
    submitted: Option<CreateObjectStoreResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl ObjectStoreCreateFormState {
    fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        let default_store_class = view
            .map(|view| view.create_object_store.defaults.store_class.clone())
            .unwrap_or_else(|| "generated_data".to_string());
        let default_copies = view
            .map(|view| view.create_object_store.defaults.required_copies)
            .unwrap_or(1);
        let endpoint_export_mode = view
            .map(|view| {
                view.create_object_store
                    .defaults
                    .endpoint_export_mode
                    .clone()
            })
            .unwrap_or_else(|| "s3_bucket".to_string());
        let writer_group = view
            .and_then(|view| view.groups.first())
            .map(|group| group.group_name.clone())
            .unwrap_or_default();
        let selected_enclosure = view.and_then(|view| view.mounted_enclosures.first());
        let enclosure_id = selected_enclosure
            .map(|enclosure| enclosure.enclosure_id.clone())
            .unwrap_or_default();
        let ssd_root = selected_enclosure
            .map(enclosure_ssd_root)
            .unwrap_or_else(|| "/srv/dasobjectstore/ssd".to_string());

        Self {
            open: false,
            store_id: String::new(),
            writer_group,
            enclosure_id,
            object_type: "naive".to_string(),
            required_copies: default_copies,
            public: false,
            writeable: true,
            store_class: default_store_class,
            capacity_behavior: "backpressure_by_priority".to_string(),
            retention: "retain_until_deleted".to_string(),
            endpoint_export_mode,
            bucket: String::new(),
            ssd_root,
            planning: false,
            plan: None,
            confirmation_phrase: String::new(),
            submitting: false,
            submitted: None,
            error: None,
        }
    }

    fn reset_plan(&mut self) {
        self.plan = None;
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObjectStoreConfigureFormState {
    open: bool,
    selected_store_id: String,
    store_class: String,
    required_copies: u8,
    writer_group: String,
    public: bool,
    writeable: bool,
    capacity_behavior: String,
    retention: String,
    endpoint_export_mode: String,
    ssd_root: String,
    planning: bool,
    plan: Option<GuiActionPlanResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl ObjectStoreConfigureFormState {
    fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        let selected = view.and_then(|view| view.stores.first());
        Self {
            open: false,
            selected_store_id: selected
                .map(|store| store.store_id.clone())
                .unwrap_or_default(),
            store_class: selected
                .and_then(|store| store.store_class.clone())
                .or_else(|| view.map(|view| view.create_object_store.defaults.store_class.clone()))
                .unwrap_or_else(|| "generated_data".to_string()),
            required_copies: selected
                .and_then(|store| store.required_copies)
                .or_else(|| view.map(|view| view.create_object_store.defaults.required_copies))
                .unwrap_or(1),
            writer_group: selected
                .and_then(|store| store.writer_group.clone())
                .or_else(|| {
                    view.and_then(|view| view.groups.first())
                        .map(|group| group.group_name.clone())
                })
                .unwrap_or_default(),
            public: selected.and_then(|store| store.public).unwrap_or(false),
            writeable: selected.and_then(|store| store.writeable).unwrap_or(true),
            capacity_behavior: "backpressure_by_priority".to_string(),
            retention: "tombstone_then_gc".to_string(),
            endpoint_export_mode: selected
                .and_then(|store| store.endpoint_export_mode.clone())
                .or_else(|| {
                    view.map(|view| {
                        view.create_object_store
                            .defaults
                            .endpoint_export_mode
                            .clone()
                    })
                })
                .unwrap_or_else(|| "s3".to_string()),
            ssd_root: "/srv/dasobjectstore/ssd".to_string(),
            planning: false,
            plan: None,
            error: None,
        }
    }

    fn apply_store(&mut self, store: &ObjectStoreCardResponse) {
        self.selected_store_id = store.store_id.clone();
        self.store_class = store
            .store_class
            .clone()
            .unwrap_or_else(|| "generated_data".to_string());
        self.required_copies = store.required_copies.unwrap_or(1);
        self.writer_group = store.writer_group.clone().unwrap_or_default();
        self.public = store.public.unwrap_or(false);
        self.writeable = store.writeable.unwrap_or(true);
        self.endpoint_export_mode = store
            .endpoint_export_mode
            .clone()
            .unwrap_or_else(|| "s3".to_string());
        self.reset_plan();
    }

    fn reset_plan(&mut self) {
        self.plan = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SubObjectFormState {
    open: bool,
    subobject_name: String,
    parent_kind: String,
    parent_store_id: String,
    parent_subobject_name: String,
    object_type_mode: String,
    object_type: String,
    s3_routing: String,
    ssd_root: String,
    planning: bool,
    plan: Option<GuiActionPlanResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl SubObjectFormState {
    fn from_view(view: Option<&ObjectStoresPageResponse>) -> Self {
        Self {
            open: false,
            subobject_name: String::new(),
            parent_kind: "store".to_string(),
            parent_store_id: view
                .and_then(|view| view.stores.first())
                .map(|store| store.store_id.clone())
                .unwrap_or_default(),
            parent_subobject_name: String::new(),
            object_type_mode: "inherit".to_string(),
            object_type: "naive".to_string(),
            s3_routing: "inherit_parent".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".to_string(),
            planning: false,
            plan: None,
            error: None,
        }
    }

    fn reset_plan(&mut self) {
        self.plan = None;
        self.error = None;
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_store_bucket_default(store_id: &str) -> String {
    store_id
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(any(target_arch = "wasm32", test))]
fn enclosure_ssd_root(enclosure: &DasEnclosureCardResponse) -> String {
    let mount_path = enclosure.mount_path.trim_end_matches('/');
    if let Some(root) = mount_path.strip_suffix("/hdd") {
        format!("{root}/ssd")
    } else {
        format!("{mount_path}/ssd")
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_store_creation_fields_ready(
    store_id: &str,
    writer_group: &str,
    enclosure_id: &str,
) -> bool {
    !store_id.trim().is_empty()
        && !writer_group.trim().is_empty()
        && !enclosure_id.trim().is_empty()
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_store_create_confirmation_matches(value: &str) -> bool {
    value.trim() == "confirm create objectstore"
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_store_configure_review_from_values(
    store_id: &str,
    required_copies: u8,
    writer_group: &str,
    capacity_behavior: &str,
    retention: &str,
    endpoint_export_mode: &str,
    public: bool,
    writeable: bool,
) -> String {
    format!(
        "{} · {} copy/copies · writer group {} · capacity {} · retention {} · export {} · {} · {}",
        if store_id.trim().is_empty() {
            "no store selected"
        } else {
            store_id.trim()
        },
        required_copies,
        if writer_group.trim().is_empty() {
            "pending"
        } else {
            writer_group.trim()
        },
        capacity_behavior,
        retention,
        endpoint_export_mode,
        if public { "public" } else { "private" },
        if writeable { "writeable" } else { "read-only" }
    )
}

#[cfg(target_arch = "wasm32")]
fn object_store_configure_review(state: &ObjectStoreConfigureFormState) -> String {
    object_store_configure_review_from_values(
        &state.selected_store_id,
        state.required_copies,
        &state.writer_group,
        &state.capacity_behavior,
        &state.retention,
        &state.endpoint_export_mode,
        state.public,
        state.writeable,
    )
}

#[cfg(any(target_arch = "wasm32", test))]
fn subobject_registry_preview_from_values(
    subobject_name: &str,
    parent_kind: &str,
    parent_store_id: &str,
    parent_subobject_name: &str,
    object_type_mode: &str,
    object_type: &str,
    s3_routing: &str,
) -> String {
    let name = if subobject_name.trim().is_empty() {
        "unnamed-subobject"
    } else {
        subobject_name.trim()
    };
    let parent = if parent_kind == "subobject" {
        if parent_subobject_name.trim().is_empty() {
            "subobject:pending"
        } else {
            parent_subobject_name.trim()
        }
    } else if parent_store_id.trim().is_empty() {
        "store:pending"
    } else {
        parent_store_id.trim()
    };
    let object_type_label = if object_type_mode == "override" {
        format!("object type {object_type}")
    } else {
        "inherits object type".to_string()
    };

    format!(
        "{} under {} · prefix {}/{} · {} · S3 routing {}",
        name, parent, parent, name, object_type_label, s3_routing
    )
}

#[cfg(target_arch = "wasm32")]
fn subobject_registry_preview(state: &SubObjectFormState) -> String {
    subobject_registry_preview_from_values(
        &state.subobject_name,
        &state.parent_kind,
        &state.parent_store_id,
        &state.parent_subobject_name,
        &state.object_type_mode,
        &state.object_type,
        &state.s3_routing,
    )
}

#[cfg(target_arch = "wasm32")]
fn object_store_create_request_from_state(
    state: &ObjectStoreCreateFormState,
) -> CreateObjectStoreRequest {
    let bucket = if state.bucket.trim().is_empty() {
        object_store_bucket_default(&state.store_id)
    } else {
        state.bucket.trim().to_string()
    };
    CreateObjectStoreRequest {
        store_id: state.store_id.trim().to_string(),
        store_class: state.store_class.clone(),
        required_copies: state.required_copies,
        bucket: (!bucket.is_empty()).then_some(bucket),
        writer_group: state.writer_group.trim().to_string(),
        ssd_root: state.ssd_root.trim().to_string(),
        object_type: state.object_type.clone(),
        enclosure_id: (!state.enclosure_id.trim().is_empty())
            .then(|| state.enclosure_id.trim().to_string()),
        public: state.public,
        writeable: true,
        capacity_behavior: state.capacity_behavior.clone(),
        retention: state.retention.clone(),
        endpoint_export_mode: state.endpoint_export_mode.clone(),
        dry_run: false,
        client_request_id: None,
        confirmation_marker: Some("confirm create objectstore".to_string()),
    }
}

pub fn object_store_card_summaries(view: &ObjectStoresPageResponse) -> Vec<ObjectStoreCardSummary> {
    view.stores
        .iter()
        .map(|store| {
            let store_class = store
                .store_class
                .as_deref()
                .unwrap_or("unclassified")
                .to_string();
            let copies = store
                .required_copies
                .map(|copies| format!("{copies} required copy/copies"))
                .unwrap_or_else(|| "copy policy pending".to_string());
            let capacity = store
                .capacity
                .as_ref()
                .map(|capacity| {
                    format!(
                        "{} TiB used; {} TiB free",
                        capacity.used_tib, capacity.free_tib
                    )
                })
                .unwrap_or_else(|| "capacity pending".to_string());

            ObjectStoreCardSummary {
                id: store.store_id.clone(),
                label: store_class,
                name: store.display_name.clone(),
                health: store.health.clone(),
                object_type: store.object_type.as_deref().unwrap_or("naive").to_string(),
                access: format!(
                    "{} / {}",
                    if store.public.unwrap_or(false) {
                        "public"
                    } else {
                        "private"
                    },
                    if store.writeable.unwrap_or(false) {
                        "writeable"
                    } else {
                        "read-only"
                    }
                ),
                policy: format!(
                    "{}; {}",
                    copies,
                    store
                        .placement_policy
                        .as_deref()
                        .unwrap_or("placement pending")
                ),
                capacity,
                objects: format!("{} object(s)", store.object_count),
                writer_group: store
                    .writer_group
                    .as_deref()
                    .unwrap_or("writer group pending")
                    .to_string(),
                writer_policy: store
                    .writer_policy
                    .as_ref()
                    .map(|policy| policy.message.clone())
                    .unwrap_or_else(|| "Writer policy readiness pending".to_string()),
                endpoint: store
                    .endpoint_export_mode
                    .as_deref()
                    .unwrap_or("endpoint pending")
                    .to_string(),
                warning_count: store.warnings.len(),
                last_ingested: store
                    .last_ingested_at_utc
                    .as_deref()
                    .unwrap_or("no ingest recorded")
                    .to_string(),
            }
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_initial_endpoint(view: &ObjectStoresPageResponse) -> Option<String> {
    view.selected_store_id
        .as_ref()
        .filter(|store_id| !store_id.trim().is_empty())
        .cloned()
        .or_else(|| view.stores.first().map(|store| store.store_id.clone()))
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn object_browser_folder_summaries(
    folders: &[ObjectBrowserFolderNodeResponse],
) -> Vec<ObjectBrowserFolderSummary> {
    folders
        .iter()
        .map(|folder| ObjectBrowserFolderSummary {
            name: folder.name.clone(),
            prefix: folder.prefix.clone(),
            objects: folder
                .object_count
                .map(|count| format!("{count} object(s)"))
                .unwrap_or_else(|| "object count pending".to_string()),
            size: folder
                .total_size_bytes
                .map(format_browser_bytes)
                .unwrap_or_else(|| "size pending".to_string()),
            readiness: labelize_state(&folder.readiness),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn object_browser_file_summaries(
    files: &[ObjectBrowserFileNodeResponse],
) -> Vec<ObjectBrowserFileSummary> {
    files
        .iter()
        .map(|file| ObjectBrowserFileSummary {
            object_id: file.object_id.clone(),
            name: file.name.clone(),
            path: file.path.clone(),
            object_type: labelize_state(&file.object_type),
            size: format_browser_bytes(file.size_bytes),
            modified: file
                .modified_at_utc
                .as_deref()
                .unwrap_or("not recorded")
                .to_string(),
            readiness: labelize_state(&file.readiness),
            lifecycle: labelize_state(&file.lifecycle_state),
            copies: format!("{} copy/copies", file.copy_count),
            placement_summary: object_browser_placement_summary(&file.placements),
            placements: file.placements.clone(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
fn format_browser_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= TIB {
        format!("{:.1} TiB", bytes / TIB)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn labelize_state(value: &str) -> String {
    let normalized = value.replace('-', "_");
    normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .flat_map(split_camel_token)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(target_arch = "wasm32", test))]
fn split_camel_token(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if character.is_uppercase() && !current.is_empty() {
            words.push(titlecase_word(&current));
            current.clear();
        }
        current.push(character);
    }
    if !current.is_empty() {
        words.push(titlecase_word(&current));
    }
    words
}

#[cfg(any(target_arch = "wasm32", test))]
fn titlecase_word(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_placement_summary(placements: &[ObjectBrowserPlacementResponse]) -> String {
    if placements.is_empty() {
        return "placement pending".to_string();
    }
    let ssd = placements
        .iter()
        .filter(|placement| placement.location == "ssd_landing")
        .count();
    let hdd = placements
        .iter()
        .filter(|placement| placement.location == "hdd_settled")
        .count();
    let external = placements
        .iter()
        .filter(|placement| placement.location == "external_endpoint")
        .count();
    let degraded_or_missing = placements
        .iter()
        .filter(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
        .count();
    let pending = placements
        .iter()
        .filter(|placement| placement.state == "pending")
        .count();
    let verified_hdd = placements
        .iter()
        .filter(|placement| placement.location == "hdd_settled" && placement.state == "verified")
        .count();

    let mut parts = Vec::new();
    if ssd > 0 {
        parts.push(format!("{ssd} SSD landing"));
    }
    if hdd > 0 {
        parts.push(format!("{hdd} HDD settled"));
    }
    if external > 0 {
        parts.push(format!("{external} external endpoint"));
    }
    if verified_hdd > 1 {
        parts.push(format!("{verified_hdd} verified HDD copies"));
    }
    if degraded_or_missing > 0 {
        parts.push(format!("{degraded_or_missing} degraded/missing"));
    }
    if pending > 0 {
        parts.push(format!("{pending} pending"));
    }
    parts.join(" · ")
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_state_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(' ', "_")
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use web_sys::{
    Blob, BlobPropertyBag, DragEvent, File, HtmlAnchorElement, HtmlInputElement, HtmlSelectElement,
    Url,
};
#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct HomeDashboardProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(HomeDashboard)]
pub fn home_dashboard(props: &HomeDashboardProps) -> Html {
    let api_path = WorkspacePage::Home.api_path(&props.api_base_path);
    let dashboard_state = use_state(|| ApiLoadState::<HomeDashboardResponse>::Loading);

    {
        let api_path = api_path.clone();
        let dashboard_state = dashboard_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                dashboard_state.set(page_load_state_from_result(
                    crate::api::get_home_dashboard(&path).await,
                    |_| None,
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="home" data-api-route={api_path}>
            <PageHeader
                eyebrow="Appliance"
                title="Home"
                summary="Current operating posture for local storage, ingress, and object service."
            />
            { render_home_dashboard_state(&*dashboard_state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_home_dashboard_state(state: &ApiLoadState<HomeDashboardResponse>) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <>
                <div class="dos-metric-grid">
                    { for home_dashboard_loading_cards().into_iter().map(render_loading_metric_card) }
                </div>
                <section class="dos-card dos-wide-card dos-loading-card">
                    <span class="dos-card-label">{ "Loading" }</span>
                    <h2>{ "Loading live dashboard telemetry." }</h2>
                    <p>{ "The Web console is requesting daemon-backed drive, capacity, throughput, memory, and SMART state." }</p>
                </section>
            </>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => html! {
            <>
                <div class="dos-metric-grid">
                    { for home_dashboard_metrics(view).into_iter().map(render_metric_card) }
                </div>
                <div class="dos-attention-grid">
                    { for home_dashboard_attention(view).into_iter().map(render_attention_card) }
                </div>
            </>
        },
        ApiLoadState::Empty(message) => {
            render_home_state_message("Empty", "No dashboard data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_home_state_message(
            "Permission denied",
            "Home dashboard requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_home_state_message("Error", "Unable to load Home dashboard", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn home_dashboard_loading_cards() -> Vec<&'static str> {
    vec![
        "Drive inventory",
        "DAS enclosures",
        "Capacity",
        "7-day throughput",
        "Memory stress",
        "SMART warnings",
        "ObjectStores",
    ]
}

#[cfg(target_arch = "wasm32")]
fn render_loading_metric_card(label: &'static str) -> Html {
    html! {
        <section class="dos-card dos-metric-card dos-loading-card" data-state="loading">
            <div class="dos-card-row">
                <span class="dos-card-label">{ label }</span>
                <span class="dos-status-pill">{ "Loading" }</span>
            </div>
            <strong>{ "..." }</strong>
            <p>{ "Awaiting live daemon payload." }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_metric_card(metric: DashboardMetric) -> Html {
    html! {
        <section class="dos-card dos-metric-card" data-state={metric.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.label }</span>
                <span class="dos-status-pill">{ metric.state }</span>
            </div>
            <strong>{ metric.value }</strong>
            <p>{ metric.detail }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_attention_card(item: DashboardAttentionItem) -> Html {
    html! {
        <section class="dos-card dos-wide-card" data-state={item.state.clone()}>
            <span class="dos-card-label">{ "Attention" }</span>
            <h2>{ item.title }</h2>
            <p>{ item.detail }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_home_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct EnclosuresPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(EnclosuresPage)]
pub fn enclosures_page(props: &EnclosuresPageProps) -> Html {
    let api_path = WorkspacePage::Enclosures.api_path(&props.api_base_path);
    let selected_id = use_state(String::new);
    let enclosures_state = use_state(|| ApiLoadState::<EnclosuresPageResponse>::Loading);
    let wizard_state = use_state(EnclosureWizardState::default);

    {
        let api_path = api_path.clone();
        let enclosures_state = enclosures_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                enclosures_state.set(page_load_state_from_result(
                    crate::api::get_enclosures_dashboard(&path).await,
                    |view| {
                        view.enclosures.is_empty().then(|| {
                            view.warnings
                                .first()
                                .map(|warning| warning.message.clone())
                                .unwrap_or_else(|| {
                                    "No supported DAS enclosures reported.".to_string()
                                })
                        })
                    },
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="enclosures" data-api-route={api_path}>
            <PageHeader
                eyebrow="Storage hardware"
                title="Enclosures"
                summary="Physical shelves and landing media grouped for operator review."
            />
            { render_enclosures_state(&*enclosures_state, selected_id, wizard_state, props.api_base_path.clone()) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosures_state(
    state: &ApiLoadState<EnclosuresPageResponse>,
    selected_id: UseStateHandle<String>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-two-column">
                <div class="dos-card-list">
                    { render_add_enclosure_card(
                        &AddEnclosureAffordanceResponse::checking(),
                        None,
                        wizard_state,
                        api_base_path,
                    ) }
                    { render_enclosures_state_message(
                        "Loading",
                        "Loading enclosure inventory",
                        "The Web console is requesting daemon-backed DAS enclosure, drive, mount, capacity, and warning state.",
                    ) }
                </div>
                <section class="dos-card dos-detail-panel">
                    <span class="dos-card-label">{ "Enclosure detail" }</span>
                    <h2>{ "Waiting for daemon inventory" }</h2>
                    <p>{ "Drive cards, SMART warnings, bay mapping, mount state, and administrator actions will appear here once a supported enclosure is detected." }</p>
                </section>
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_enclosure_inventory(view, selected_id, wizard_state, api_base_path)
        }
        ApiLoadState::Empty(message) => {
            render_enclosures_state_message("Inventory", "No live enclosures reported yet", message)
        }
        ApiLoadState::PermissionDenied(message) => render_enclosures_state_message(
            "Permission denied",
            "Enclosure inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_enclosures_state_message("Error", "Unable to load enclosure inventory", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_inventory(
    view: &EnclosuresPageResponse,
    selected_id: UseStateHandle<String>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let active_id = if selected_id.is_empty() {
        view.selected_enclosure_id
            .as_deref()
            .or_else(|| {
                view.enclosures
                    .first()
                    .map(|enclosure| enclosure.enclosure_id.as_str())
            })
            .unwrap_or_default()
            .to_string()
    } else {
        (*selected_id).clone()
    };

    html! {
        <div class="dos-two-column">
            <div class="dos-card-list">
                if view.add_enclosure.enabled {
                    { render_add_enclosure_card(
                        &view.add_enclosure,
                        enclosure_prepare_candidate(view, &active_id),
                        wizard_state,
                        api_base_path,
                    ) }
                }
                { for enclosure_card_summaries(view).into_iter().map(|summary| {
                    render_enclosure_card(summary, &active_id, selected_id.clone())
                }) }
            </div>
            { render_enclosure_detail_panel(view, &active_id) }
        </div>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct EnclosureWizardState {
    open: bool,
    selected_ssd: String,
    selected_hdds: Vec<String>,
    mount_root: String,
    filesystem: String,
    owner: String,
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_phrase: String,
    submitting: bool,
    job: Option<EnclosurePrepareResponse>,
    job_status: Option<AdminJobStatusResponse>,
    job_polling: bool,
    job_status_error: Option<String>,
    cancelling: bool,
    cancellation: Option<AdminJobCancelResponse>,
    cancel_error: Option<String>,
    error: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl Default for EnclosureWizardState {
    fn default() -> Self {
        Self {
            open: false,
            selected_ssd: String::new(),
            selected_hdds: Vec::new(),
            mount_root: "/srv/dasobjectstore".to_string(),
            filesystem: "ext4".to_string(),
            owner: String::new(),
            allow_format: false,
            existing_data_acknowledged: false,
            confirmation_phrase: String::new(),
            submitting: false,
            job: None,
            job_status: None,
            job_polling: false,
            job_status_error: None,
            cancelling: false,
            cancellation: None,
            cancel_error: None,
            error: None,
        }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn clear_enclosure_job_monitor(state: &mut EnclosureWizardState) {
    state.job = None;
    state.job_status = None;
    state.job_polling = false;
    state.job_status_error = None;
    state.cancelling = false;
    state.cancellation = None;
    state.cancel_error = None;
    state.error = None;
}

#[cfg(any(target_arch = "wasm32", test))]
fn admin_job_state_is_terminal(state: &str) -> bool {
    matches!(state, "complete" | "failed" | "cancelled")
}

#[cfg(any(target_arch = "wasm32", test))]
fn admin_job_percent(job: &AdminJobSummary) -> Option<u8> {
    job.percent_complete.or_else(|| {
        (job.progress.work_units_total > 0).then(|| {
            ((job.progress.work_units_done.saturating_mul(100) / job.progress.work_units_total)
                .min(100)) as u8
        })
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn admin_job_progress_text(job: &AdminJobSummary) -> String {
    if job.progress.work_bytes_total > 0 {
        return format!(
            "{} / {} byte(s)",
            job.progress.work_bytes_done, job.progress.work_bytes_total
        );
    }
    if job.progress.work_units_total > 0 {
        return format!(
            "{} / {} step(s)",
            job.progress.work_units_done, job.progress.work_units_total
        );
    }
    "Progress pending".to_string()
}

#[cfg(any(target_arch = "wasm32", test))]
fn enclosure_prepare_confirmed(
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_phrase: &str,
) -> bool {
    allow_format
        && existing_data_acknowledged
        && confirmation_phrase.trim() == "confirm prepare das"
}

#[cfg(any(target_arch = "wasm32", test))]
fn enclosure_retry_clears_job_state(state: &mut EnclosureWizardState) {
    clear_enclosure_job_monitor(state);
}

#[cfg(target_arch = "wasm32")]
fn enclosure_wizard_job_id(state: &EnclosureWizardState) -> Option<String> {
    state.job.as_ref().map(|job| job.accepted.job_id.clone())
}

#[cfg(target_arch = "wasm32")]
fn schedule_enclosure_job_status_poll(
    api_base_path: String,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    job_id: String,
    delay_ms: u32,
) {
    Timeout::new(delay_ms, move || {
        let api_base_path = api_base_path.clone();
        let wizard_state = wizard_state.clone();
        let job_id = job_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::get_admin_job_status(&api_base_path, &job_id).await;
            let mut should_continue = false;
            let mut next = (*wizard_state).clone();
            if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                return;
            }
            next.job_polling = false;
            match result {
                Ok(status) => {
                    should_continue = !admin_job_state_is_terminal(&status.job.state);
                    next.job_status = Some(status);
                    next.job_status_error = None;
                    if should_continue {
                        next.job_polling = true;
                    }
                }
                Err(error) => {
                    next.job_status_error = Some(error.message);
                }
            }
            wizard_state.set(next);
            if should_continue {
                schedule_enclosure_job_status_poll(api_base_path, wizard_state, job_id, 2_000);
            }
        });
    })
    .forget();
}

#[cfg(target_arch = "wasm32")]
fn render_add_enclosure_card(
    affordance: &AddEnclosureAffordanceResponse,
    candidate: Option<EnclosurePrepareCandidate>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let state_label = match affordance.state.as_str() {
        "ready" => "Ready",
        "already_managed" => "Already managed",
        "admin_required" => "Admin required",
        "unsupported_or_absent" => "No supported DAS",
        "daemon_unavailable" => "Daemon not ready",
        "checking" => "Checking",
        _ => "Unavailable",
    };
    let body = affordance
        .blocked_reason
        .as_deref()
        .unwrap_or("Administrator workflow: detect supported DAS hardware, identify SSD/HDD media, review format risk, then submit the daemon preparation job.");
    let candidate_ready = candidate
        .as_ref()
        .is_some_and(EnclosurePrepareCandidate::ready);
    let can_open = affordance.enabled && candidate_ready;
    let open_wizard = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            next.open = true;
            next.error = None;
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };

    html! {
        <section
            class={classes!(
                "dos-card",
                "dos-create-card",
                affordance.enabled.then_some("is-enabled"),
                (!affordance.enabled).then_some("is-disabled"),
            )}
            data-action={affordance.action_kind.clone()}
            data-state={affordance.state.clone()}
            aria-disabled={(!affordance.enabled).to_string()}
        >
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ affordance.label.clone() }</h2>
            <p>{ body }</p>
            <p class="dos-create-next-step">{ affordance.next_step.clone() }</p>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!can_open}
                onclick={open_wizard}
            >
                { "Plan preparation" }
            </button>
            <div class="dos-card-row dos-create-gates">
                <span class="dos-status-pill">{ state_label }</span>
                <span>{ if affordance.administrator { "admin verified" } else { "admin pending" } }</span>
                <span>{ if affordance.supported_enclosure_detected { "supported DAS visible" } else { "DAS not detected" } }</span>
                <span>{ if affordance.daemon_ready { "daemon ready" } else { "daemon pending" } }</span>
            </div>
            { render_enclosure_wizard(candidate, wizard_state, api_base_path) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_wizard(
    candidate: Option<EnclosurePrepareCandidate>,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let state = (*wizard_state).clone();
    if !state.open {
        return html! {};
    }
    let Some(candidate) = candidate else {
        return html! {};
    };

    let selected_ssd = if state.selected_ssd.is_empty() {
        candidate
            .ssd_devices
            .first()
            .map(|device| device.device_path.clone())
            .unwrap_or_default()
    } else {
        state.selected_ssd.clone()
    };
    let selected_hdds = if state.selected_hdds.is_empty() {
        candidate
            .hdd_devices
            .iter()
            .map(|device| device.device_path.clone())
            .collect::<Vec<_>>()
    } else {
        state.selected_hdds.clone()
    };
    let selected_hdd_devices = selected_hdds
        .iter()
        .filter_map(|path| {
            candidate
                .hdd_devices
                .iter()
                .find(|device| &device.device_path == path)
                .map(|device| EnclosurePrepareHddDevice {
                    disk_id: device.disk_id.clone(),
                    device_path: device.device_path.clone(),
                })
        })
        .collect::<Vec<_>>();
    let confirmed = enclosure_prepare_confirmed(
        state.allow_format,
        state.existing_data_acknowledged,
        &state.confirmation_phrase,
    );
    let can_submit = !state.submitting
        && state.job.is_none()
        && !selected_ssd.is_empty()
        && !selected_hdds.is_empty()
        && confirmed;

    let close = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            next.open = false;
            wizard_state.set(next);
        })
    };
    let on_ssd = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.selected_ssd = input.value();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let on_mount_root = string_input_callback(wizard_state.clone(), |state, value| {
        state.mount_root = value;
    });
    let on_filesystem = string_change_callback(wizard_state.clone(), |state, value| {
        state.filesystem = value;
    });
    let on_owner = string_input_callback(wizard_state.clone(), |state, value| {
        state.owner = value;
    });
    let on_confirmation = string_input_callback(wizard_state.clone(), |state, value| {
        state.confirmation_phrase = value;
    });
    let on_allow_format = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.allow_format = input.checked();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let on_existing_data_acknowledged = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*wizard_state).clone();
            next.existing_data_acknowledged = input.checked();
            clear_enclosure_job_monitor(&mut next);
            wizard_state.set(next);
        })
    };
    let submit = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let selected_ssd = selected_ssd.clone();
        let selected_hdds = selected_hdds.clone();
        let selected_hdd_devices = selected_hdd_devices.clone();
        Callback::from(move |_| {
            let mut pending = (*wizard_state).clone();
            pending.submitting = true;
            pending.error = None;
            clear_enclosure_job_monitor(&mut pending);
            wizard_state.set(pending.clone());

            let wizard_state = wizard_state.clone();
            let api_base_path = api_base_path.clone();
            let selected_ssd = selected_ssd.clone();
            let selected_hdds = selected_hdds.clone();
            let selected_hdd_devices = selected_hdd_devices.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = EnclosurePrepareRequest {
                    ssd_device: selected_ssd,
                    hdd_devices: selected_hdd_devices,
                    mount_root: (!pending.mount_root.trim().is_empty())
                        .then(|| pending.mount_root.clone()),
                    filesystem: (!pending.filesystem.trim().is_empty())
                        .then(|| pending.filesystem.clone()),
                    owner: (!pending.owner.trim().is_empty()).then(|| pending.owner.clone()),
                    dry_run: false,
                    client_request_id: None,
                    allow_format: pending.allow_format,
                    existing_data_acknowledged: pending.existing_data_acknowledged,
                    confirmation_marker: Some(pending.confirmation_phrase.clone()),
                };
                let selected_ssd = request.ssd_device.clone();
                let result = crate::api::submit_enclosure_prepare(&api_base_path, &request).await;
                let mut next = (*wizard_state).clone();
                next.submitting = false;
                match result {
                    Ok(job) => {
                        let job_id = job.accepted.job_id.clone();
                        next.selected_ssd = selected_ssd;
                        next.selected_hdds = selected_hdds;
                        next.job = Some(job);
                        next.job_status = None;
                        next.job_polling = true;
                        next.job_status_error = None;
                        next.cancellation = None;
                        next.cancel_error = None;
                        next.error = None;
                        wizard_state.set(next);
                        schedule_enclosure_job_status_poll(
                            api_base_path.clone(),
                            wizard_state.clone(),
                            job_id,
                            0,
                        );
                        return;
                    }
                    Err(error) => {
                        clear_enclosure_job_monitor(&mut next);
                        next.error = Some(error.message);
                    }
                }
                wizard_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-enclosure-wizard" data-workflow="enclosure_add">
            <header class="dos-wizard-header">
                <span class="dos-card-label">{ "Preparation wizard" }</span>
                <h3>{ format!("Prepare {}", candidate.display_name) }</h3>
                <button type="button" onclick={close}>{ "Close" }</button>
            </header>
            <ol class="dos-wizard-steps">
                <li>{ "Supported DAS detected from daemon inventory." }</li>
                <li>{ "Select SSD landing media and HDD settlement media." }</li>
                <li>{ "Review the destructive format plan." }</li>
                <li>{ "Submit the daemon-owned preparation plan." }</li>
            </ol>
            <label class="dos-form-field">
                <span>{ "SSD landing device" }</span>
                <select onchange={on_ssd} value={selected_ssd.clone()}>
                    { for candidate.ssd_devices.iter().map(|device| html! {
                        <option value={device.device_path.clone()}>{ device.label.clone() }</option>
                    }) }
                </select>
            </label>
            <div class="dos-form-field">
                <span>{ "HDD settlement devices" }</span>
                <div class="dos-checkbox-list">
                    { for candidate.hdd_devices.iter().map(|device| {
                        let checked = selected_hdds.contains(&device.device_path);
                        let device_path = device.device_path.clone();
                        let wizard_state = wizard_state.clone();
                        html! {
                            <label>
                                <input
                                    type="checkbox"
                                    checked={checked}
                                    onchange={Callback::from(move |event: Event| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*wizard_state).clone();
                                        if input.checked() {
                                            if !next.selected_hdds.contains(&device_path) {
                                                next.selected_hdds.push(device_path.clone());
                                            }
                                        } else {
                                            next.selected_hdds.retain(|path| path != &device_path);
                                        }
                                        clear_enclosure_job_monitor(&mut next);
                                        wizard_state.set(next);
                                    })}
                                />
                                <span>{ device.label.clone() }</span>
                            </label>
                        }
                    }) }
                </div>
            </div>
            <div class="dos-form-grid">
                <label class="dos-form-field">
                    <span>{ "Mount root" }</span>
                    <input value={state.mount_root.clone()} oninput={on_mount_root} />
                </label>
                <label class="dos-form-field">
                    <span>{ "Filesystem" }</span>
                    <select onchange={on_filesystem} value={state.filesystem.clone()}>
                        <option value="ext4">{ "ext4" }</option>
                        <option value="xfs">{ "xfs" }</option>
                    </select>
                </label>
                <label class="dos-form-field">
                    <span>{ "Mounted-root owner" }</span>
                    <input placeholder="optional local user" value={state.owner.clone()} oninput={on_owner} />
                </label>
            </div>
            <section class="dos-risk-review">
                <span class="dos-card-label">{ "Data-loss review" }</span>
                <p>{ "Preparing this enclosure formats the selected SSD and HDD devices, creates DASObjectStore mount roots, and delegates execution to the daemon-side storage authority." }</p>
                <label>
                    <input type="checkbox" checked={state.allow_format} onchange={on_allow_format} />
                    <span>{ "I allow formatting of the selected devices." }</span>
                </label>
                <label>
                    <input type="checkbox" checked={state.existing_data_acknowledged} onchange={on_existing_data_acknowledged} />
                    <span>{ "I acknowledge existing data on selected devices may be destroyed." }</span>
                </label>
                <label class="dos-form-field">
                    <span>{ "Confirmation phrase" }</span>
                    <input
                        placeholder="confirm prepare das"
                        value={state.confirmation_phrase.clone()}
                        oninput={on_confirmation}
                    />
                </label>
            </section>
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
            { render_enclosure_job_monitor(&state, wizard_state.clone(), api_base_path.clone()) }
            <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                { if state.submitting { "Submitting..." } else { "Submit preparation job" } }
            </button>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_job_monitor(
    state: &EnclosureWizardState,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    api_base_path: String,
) -> Html {
    let Some(job) = &state.job else {
        return html! {};
    };
    let job_id = job.accepted.job_id.clone();
    let latest = state.job_status.as_ref().map(|status| &status.job);
    let status_label = latest
        .map(|job| job.state.clone())
        .unwrap_or_else(|| "accepted".to_string());
    let terminal = latest.is_some_and(|job| admin_job_state_is_terminal(&job.state));
    let can_cancel = !terminal && !state.cancelling;
    let can_refresh = !state.job_polling && !state.cancelling;
    let refresh = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let job_id = job_id.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                return;
            }
            next.job_polling = true;
            next.job_status_error = None;
            wizard_state.set(next);
            schedule_enclosure_job_status_poll(
                api_base_path.clone(),
                wizard_state.clone(),
                job_id.clone(),
                0,
            );
        })
    };
    let cancel = {
        let wizard_state = wizard_state.clone();
        let api_base_path = api_base_path.clone();
        let job_id = job_id.clone();
        Callback::from(move |_| {
            let mut pending = (*wizard_state).clone();
            if enclosure_wizard_job_id(&pending).as_deref() != Some(job_id.as_str()) {
                return;
            }
            pending.cancelling = true;
            pending.cancel_error = None;
            wizard_state.set(pending);

            let wizard_state = wizard_state.clone();
            let api_base_path = api_base_path.clone();
            let job_id = job_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = AdminJobCancelRequest {
                    reason: Some("cancelled from Enclosures Web preparation wizard".to_string()),
                };
                let result = crate::api::cancel_admin_job(&api_base_path, &job_id, &request).await;
                let mut next = (*wizard_state).clone();
                if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                    return;
                }
                next.cancelling = false;
                match result {
                    Ok(cancelled) => {
                        next.cancellation = Some(cancelled);
                        next.cancel_error = None;
                        next.job_polling = true;
                        wizard_state.set(next);
                        schedule_enclosure_job_status_poll(api_base_path, wizard_state, job_id, 0);
                    }
                    Err(error) => {
                        next.cancel_error = Some(error.message);
                        wizard_state.set(next);
                    }
                }
            });
        })
    };
    let retry = {
        let wizard_state = wizard_state.clone();
        Callback::from(move |_| {
            let mut next = (*wizard_state).clone();
            enclosure_retry_clears_job_state(&mut next);
            wizard_state.set(next);
        })
    };

    html! {
        <section class="dos-plan-result" data-job-state={status_label.clone()}>
            <span class="dos-card-label">{ "Daemon job" }</span>
            <h3>{ admin_job_monitor_title(latest, state.job_status_error.as_deref()) }</h3>
            <p>{ format!("Job {} · {} · dry run {}", job.accepted.job_id, job.accepted.kind, job.accepted.dry_run) }</p>
            <code>{ format!("{} -> {} HDD device(s)", job.ssd_device, job.hdd_devices.len()) }</code>
            { render_admin_job_progress(latest) }
            <div class="dos-job-meta">
                <span>{ format!("State: {status_label}") }</span>
                <span>{ format!("Submitted: {}", latest.map(|job| job.submitted_at_utc.as_str()).unwrap_or(job.accepted.accepted_at_utc.as_str())) }</span>
                <span>{ format!("Updated: {}", latest.map(|job| job.updated_at_utc.as_str()).unwrap_or("pending")) }</span>
                <span>{ if state.job_polling { "Status: polling daemon" } else { "Status: current" } }</span>
            </div>
            if let Some(message) = &state.job_status_error {
                <div class="dos-auth-error" role="alert">{ format!("Status refresh failed: {message}") }</div>
            }
            if let Some(message) = &state.cancel_error {
                <div class="dos-auth-error" role="alert">{ format!("Cancellation failed: {message}") }</div>
            }
            if let Some(cancelled) = &state.cancellation {
                <p class="dos-job-message">{ format!("Cancellation request {} with daemon state {}.", if cancelled.accepted { "accepted" } else { "not accepted" }, cancelled.state) }</p>
            }
            <div class="dos-job-actions">
                <button type="button" onclick={refresh} disabled={!can_refresh}>
                    { if state.job_polling { "Refreshing..." } else { "Refresh status" } }
                </button>
                <button type="button" onclick={cancel} disabled={!can_cancel}>
                    { if state.cancelling { "Cancelling..." } else { "Cancel job" } }
                </button>
                <button type="button" onclick={retry} disabled={!terminal && state.job_status_error.is_none()}>
                    { "Retry preparation" }
                </button>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn admin_job_monitor_title(job: Option<&AdminJobSummary>, status_error: Option<&str>) -> String {
    if status_error.is_some() {
        return "Preparation status needs attention.".to_string();
    }
    match job.map(|job| job.state.as_str()) {
        Some("complete") => "Preparation completed by dasobjectstored.".to_string(),
        Some("failed") => "Preparation failed in dasobjectstored.".to_string(),
        Some("cancelled") => "Preparation cancelled.".to_string(),
        Some("running") => "Preparation is running.".to_string(),
        Some("waiting") => "Preparation is waiting.".to_string(),
        Some("queued") => "Preparation is queued.".to_string(),
        Some(_) => "Preparation state reported by dasobjectstored.".to_string(),
        None => "Preparation submitted to dasobjectstored.".to_string(),
    }
}

#[cfg(target_arch = "wasm32")]
fn render_admin_job_progress(job: Option<&AdminJobSummary>) -> Html {
    let Some(job) = job else {
        return html! {
            <div class="dos-job-progress">
                <div class="dos-job-progress-bar"><span style="width: 0%"></span></div>
                <p>{ "Waiting for daemon progress." }</p>
            </div>
        };
    };
    let percent = admin_job_percent(job);
    let width = format!("width: {}%", percent.unwrap_or(0));
    html! {
        <div class="dos-job-progress">
            <div class="dos-job-progress-bar" aria-label="Administrator job progress">
                <span style={width}></span>
            </div>
            <p>
                { format!(
                    "{} · {} · {}",
                    percent.map(|value| format!("{value}%")).unwrap_or_else(|| "Percent pending".to_string()),
                    job.progress.stage,
                    admin_job_progress_text(job)
                ) }
            </p>
            if let Some(message) = &job.progress.message {
                <p class="dos-job-message">{ message.clone() }</p>
            }
            if let Some(message) = &job.failure_message {
                <div class="dos-auth-error" role="alert">{ message.clone() }</div>
            }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn string_input_callback<F>(
    wizard_state: UseStateHandle<EnclosureWizardState>,
    update: F,
) -> Callback<InputEvent>
where
    F: Fn(&mut EnclosureWizardState, String) + 'static,
{
    Callback::from(move |event: InputEvent| {
        let input: HtmlInputElement = event.target_unchecked_into();
        let mut next = (*wizard_state).clone();
        update(&mut next, input.value());
        clear_enclosure_job_monitor(&mut next);
        wizard_state.set(next);
    })
}

#[cfg(target_arch = "wasm32")]
fn string_change_callback<F>(
    wizard_state: UseStateHandle<EnclosureWizardState>,
    update: F,
) -> Callback<Event>
where
    F: Fn(&mut EnclosureWizardState, String) + 'static,
{
    Callback::from(move |event: Event| {
        let input: HtmlSelectElement = event.target_unchecked_into();
        let mut next = (*wizard_state).clone();
        update(&mut next, input.value());
        clear_enclosure_job_monitor(&mut next);
        wizard_state.set(next);
    })
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_card(
    summary: EnclosureCardSummary,
    active_id: &str,
    selected_id: UseStateHandle<String>,
) -> Html {
    let is_selected = summary.id == active_id;
    let enclosure_id = summary.id.clone();
    html! {
        <button
            type="button"
            class={classes!("dos-card", "dos-enclosure-card", is_selected.then_some("is-selected"))}
            data-enclosure-id={summary.id.clone()}
            aria-pressed={is_selected.to_string()}
            onclick={Callback::from(move |_| selected_id.set(enclosure_id.clone()))}
        >
            <div class="dos-card-row">
                <span class="dos-card-label">{ summary.label }</span>
                <span class="dos-status-pill">{ summary.health }</span>
            </div>
            <strong>{ summary.name }</strong>
            <p>{ summary.drives }</p>
            <p>{ summary.capacity }</p>
            <p>{ format!("{} warning(s) · {}", summary.warning_count, summary.mount_path) }</p>
        </button>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_detail_panel(view: &EnclosuresPageResponse, active_id: &str) -> Html {
    let enclosure = view
        .enclosures
        .iter()
        .find(|enclosure| enclosure.enclosure_id == active_id);
    let detail = view
        .details
        .as_ref()
        .filter(|detail| detail.enclosure_id == active_id);

    html! {
        <section class="dos-card dos-detail-panel">
            { match (enclosure, detail) {
                (Some(enclosure), Some(detail)) => render_enclosure_detail(enclosure, detail),
                (Some(enclosure), None) => render_enclosure_summary_detail(enclosure),
                _ => html! {
                    <>
                        <span class="dos-card-label">{ "Enclosure detail" }</span>
                        <h2>{ "Select an enclosure" }</h2>
                        <p>{ "Drive cards, SMART warnings, bay mapping, mount state, and administrator actions will appear here once a supported enclosure is detected." }</p>
                    </>
                },
            } }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_detail(
    enclosure: &DasEnclosureCardResponse,
    detail: &DasEnclosureDetailResponse,
) -> Html {
    html! {
        <>
            <span class="dos-card-label">{ "Enclosure detail" }</span>
            <h2>{ &enclosure.display_name }</h2>
            <dl class="dos-detail-list">
                <div><dt>{ "Vendor" }</dt><dd>{ &detail.vendor }</dd></div>
                <div><dt>{ "Model" }</dt><dd>{ &detail.model }</dd></div>
                <div><dt>{ "Serial" }</dt><dd>{ &detail.serial }</dd></div>
                <div><dt>{ "Firmware" }</dt><dd>{ detail.firmware.as_deref().unwrap_or("unknown") }</dd></div>
                <div><dt>{ "Mount" }</dt><dd>{ &enclosure.mount_path }</dd></div>
                <div><dt>{ "Connection" }</dt><dd>{ format!("{} / {} / {}", enclosure.connection.bus, enclosure.connection.protocol, enclosure.connection.link_speed) }</dd></div>
                <div><dt>{ "Capacity" }</dt><dd>{ format!("{} TiB free of {} TiB", enclosure.capacity.free_tib, enclosure.capacity.total_tib) }</dd></div>
                <div><dt>{ "Warnings" }</dt><dd>{ enclosure.warnings.len().to_string() }</dd></div>
            </dl>
            <div class="dos-slot-list">
                { for detail.slots.iter().map(render_drive_slot_card) }
            </div>
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_drive_slot_card(slot: &EnclosureDriveSlotResponse) -> Html {
    let bay_label = if slot.slot_number == 0 {
        "SSD".to_string()
    } else {
        format!("Bay {}", slot.slot_number)
    };
    let role = slot.role.as_deref().unwrap_or("unassigned");
    let mount = slot
        .mount_path
        .as_deref()
        .unwrap_or("mount path unavailable");
    let filesystem = slot.filesystem.as_deref().unwrap_or("filesystem unknown");
    let device = slot.device_path.as_deref().unwrap_or("device unknown");
    let actions = if slot.actions_available.is_empty() {
        "Actions unavailable".to_string()
    } else {
        slot.actions_available.join(", ")
    };

    html! {
        <article class="dos-drive-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ bay_label }</span>
                <span class="dos-status-pill">{ &slot.health }</span>
            </div>
            <strong>{ &slot.drive_id }</strong>
            <div class="dos-drive-meta">
                <span>{ format!("Role: {}", role) }</span>
                <span>{ format!("Capacity: {} TiB", slot.size_tib) }</span>
                <span>{ if slot.mounted { "Mounted" } else { "Not mounted" } }</span>
                <span>{ format!("SMART warnings: {}", slot.smart_warning_count) }</span>
            </div>
            <dl class="dos-drive-detail-list">
                <div><dt>{ "Mount" }</dt><dd>{ mount }</dd></div>
                <div><dt>{ "Device" }</dt><dd>{ device }</dd></div>
                <div><dt>{ "Filesystem" }</dt><dd>{ filesystem }</dd></div>
                <div><dt>{ "Actions" }</dt><dd>{ actions }</dd></div>
            </dl>
        </article>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_summary_detail(enclosure: &DasEnclosureCardResponse) -> Html {
    html! {
        <>
            <span class="dos-card-label">{ "Enclosure detail" }</span>
            <h2>{ &enclosure.display_name }</h2>
            <dl class="dos-detail-list">
                <div><dt>{ "Health" }</dt><dd>{ &enclosure.health }</dd></div>
                <div><dt>{ "Mount" }</dt><dd>{ &enclosure.mount_path }</dd></div>
                <div><dt>{ "Connection" }</dt><dd>{ format!("{} / {} / {}", enclosure.connection.bus, enclosure.connection.protocol, enclosure.connection.link_speed) }</dd></div>
                <div><dt>{ "Drives" }</dt><dd>{ format!("{} mounted of {}", enclosure.drive_count.mounted, enclosure.drive_count.total) }</dd></div>
                <div><dt>{ "Capacity" }</dt><dd>{ format!("{} TiB free of {} TiB", enclosure.capacity.free_tib, enclosure.capacity.total_tib) }</dd></div>
                <div><dt>{ "Last seen" }</dt><dd>{ &enclosure.last_seen_at_utc }</dd></div>
            </dl>
            <p>{ format!("{} warning(s) reported for this enclosure.", enclosure.warnings.len()) }</p>
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosures_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct ObjectStoresPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(ObjectStoresPage)]
pub fn object_stores_page(props: &ObjectStoresPageProps) -> Html {
    let api_path = WorkspacePage::ObjectStores.api_path(&props.api_base_path);
    let object_stores_state = use_state(|| ApiLoadState::<ObjectStoresPageResponse>::Loading);
    let create_state = use_state(|| ObjectStoreCreateFormState::from_view(None));
    let configure_state = use_state(|| ObjectStoreConfigureFormState::from_view(None));
    let subobject_state = use_state(|| SubObjectFormState::from_view(None));
    let browser_endpoint = use_state(String::new);
    let browser_prefix = use_state(String::new);
    let browser_search = use_state(String::new);
    let browser_sort = use_state(|| "name_asc".to_string());
    let browser_state =
        use_state(|| ApiLoadState::<ObjectBrowserResponse>::Empty("Select an ObjectStore.".into()));
    let browser_download_state = use_state(|| ObjectBrowserDownloadState::Idle);

    {
        let api_path = api_path.clone();
        let object_stores_state = object_stores_state.clone();
        let browser_endpoint = browser_endpoint.clone();
        let browser_prefix = browser_prefix.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::get_object_stores_dashboard(&path).await;
                if let Ok(view) = &result {
                    if browser_endpoint.trim().is_empty() {
                        if let Some(endpoint) = object_browser_initial_endpoint(view) {
                            browser_endpoint.set(endpoint);
                            browser_prefix.set(String::new());
                        }
                    }
                }
                object_stores_state.set(page_load_state_from_result(result, |view| {
                    view.stores.is_empty().then(|| {
                        view.warnings
                            .first()
                            .map(|warning| warning.message.clone())
                            .unwrap_or_else(|| "No object stores reported.".to_string())
                    })
                }));
            });
            || ()
        });
    }

    {
        let api_base_path = props.api_base_path.clone();
        let browser_state = browser_state.clone();
        let endpoint = (*browser_endpoint).clone();
        let prefix = (*browser_prefix).clone();
        let search = (*browser_search).clone();
        let sort = (*browser_sort).clone();
        use_effect_with(
            (api_base_path, endpoint, prefix, search, sort),
            move |(api_base_path, endpoint, prefix, search, sort)| {
                let endpoint = endpoint.clone();
                if endpoint.trim().is_empty() {
                    browser_state.set(ApiLoadState::empty("Select an ObjectStore."));
                } else {
                    let path = crate::api::object_browser_api_path(
                        api_base_path,
                        &endpoint,
                        prefix,
                        search,
                        sort,
                        true,
                    );
                    browser_state.set(ApiLoadState::Loading);
                    wasm_bindgen_futures::spawn_local(async move {
                        browser_state.set(page_load_state_from_result(
                            crate::api::get_object_browser(&path).await,
                            |view| {
                                (view.folders.is_empty() && view.files.is_empty()).then(|| {
                                    "No folders or objects match this browser view.".to_string()
                                })
                            },
                        ));
                    });
                }
                || ()
            },
        );
    }

    html! {
        <section class="dos-page" data-page="objectstores" data-api-route={api_path}>
            <PageHeader
                eyebrow="Managed stores"
                title="ObjectStores"
                summary="Operational view of store policies, capacity, and service state."
            />
            { render_object_stores_state(
                &*object_stores_state,
                create_state,
                configure_state,
                subobject_state,
                props.api_base_path.clone(),
                browser_state,
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_stores_state(
    state: &ApiLoadState<ObjectStoresPageResponse>,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
    browser_state: UseStateHandle<ApiLoadState<ObjectBrowserResponse>>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None, create_state, api_base_path) }
                { render_object_stores_state_message(
                    "Loading",
                    "Loading object-store inventory",
                    "The Web console is requesting daemon-backed store registry, policy, capacity, endpoint, and warning state.",
                ) }
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_object_store_inventory(
                view,
                create_state,
                configure_state,
                subobject_state,
                api_base_path,
                browser_state,
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
            )
        }
        ApiLoadState::Empty(message) => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None, create_state, api_base_path) }
                { render_object_stores_state_message("Inventory", "No object stores reported yet", message) }
            </div>
        },
        ApiLoadState::PermissionDenied(message) => render_object_stores_state_message(
            "Permission denied",
            "ObjectStore inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => render_object_stores_state_message(
            "Error",
            "Unable to load ObjectStore inventory",
            message,
        ),
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_inventory(
    view: &ObjectStoresPageResponse,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
    browser_state: UseStateHandle<ApiLoadState<ObjectBrowserResponse>>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
) -> Html {
    html! {
        <div class="dos-store-grid">
            { render_object_store_create_card(Some(view), create_state, api_base_path.clone()) }
            { render_subobject_create_card(view, subobject_state, api_base_path.clone()) }
            { render_object_store_configure_card(view, configure_state, api_base_path.clone()) }
            { for object_store_card_summaries(view).into_iter().map(render_object_store_card) }
            { render_object_browser_panel(
                view,
                &*browser_state,
                api_base_path.clone(),
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
            ) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_panel(
    view: &ObjectStoresPageResponse,
    browser_state: &ApiLoadState<ObjectBrowserResponse>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
) -> Html {
    let selected_endpoint = (*browser_endpoint).clone();
    let search_value = (*browser_search).clone();
    let sort_value = (*browser_sort).clone();
    let on_endpoint_change = {
        let browser_endpoint = browser_endpoint.clone();
        let browser_prefix = browser_prefix.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            browser_endpoint.set(input.value());
            browser_prefix.set(String::new());
        })
    };
    let on_search = {
        let browser_search = browser_search.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            browser_search.set(input.value());
        })
    };
    let on_sort = {
        let browser_sort = browser_sort.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            browser_sort.set(input.value());
        })
    };

    html! {
        <section class="dos-card dos-wide-card dos-object-browser" data-state={browser_state.state_name()}>
            <div class="dos-card-row">
                <div>
                    <span class="dos-card-label">{ "Browse objects" }</span>
                    <h2>{ "ObjectStore contents" }</h2>
                </div>
                <span class="dos-status-pill">{ browser_state.state_name() }</span>
            </div>
            <div class="dos-object-browser-controls">
                <label>
                    <span>{ "Endpoint" }</span>
                    <select onchange={on_endpoint_change} value={selected_endpoint}>
                        { for view.stores.iter().map(|store| {
                            html! {
                                <option value={store.store_id.clone()}>{ store.display_name.clone() }</option>
                            }
                        }) }
                    </select>
                </label>
                <label>
                    <span>{ "Search" }</span>
                    <input
                        type="search"
                        value={search_value}
                        oninput={on_search}
                        placeholder="Object name or path"
                    />
                </label>
                <label>
                    <span>{ "Sort" }</span>
                    <select onchange={on_sort} value={sort_value}>
                        <option value="name_asc">{ "Name A-Z" }</option>
                        <option value="name_desc">{ "Name Z-A" }</option>
                        <option value="size_desc">{ "Size largest" }</option>
                        <option value="size_asc">{ "Size smallest" }</option>
                        <option value="modified_desc">{ "Modified newest" }</option>
                        <option value="modified_asc">{ "Modified oldest" }</option>
                    </select>
                </label>
            </div>
            { render_object_browser_download_state(&*browser_download_state) }
            { render_object_browser_state(
                browser_state,
                browser_prefix,
                api_base_path,
                browser_download_state,
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_state(
    state: &ApiLoadState<ObjectBrowserResponse>,
    browser_prefix: UseStateHandle<String>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_object_browser_message(
            "Loading",
            "Requesting daemon-authorized object metadata.",
        ),
        ApiLoadState::Empty(message) => render_object_browser_message("Empty", message),
        ApiLoadState::PermissionDenied(message) => {
            render_object_browser_message("Permission denied", message)
        }
        ApiLoadState::TransportError(message) => render_object_browser_message("Error", message),
        ApiLoadState::Success(response) => render_object_browser_body(
            response,
            browser_prefix,
            api_base_path,
            browser_download_state,
        ),
        ApiLoadState::StaleData { value, message } => html! {
            <>
                { render_object_browser_message("Stale", message) }
                { render_object_browser_body(
                    value,
                    browser_prefix,
                    api_base_path,
                    browser_download_state,
                ) }
            </>
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_download_state(state: &ObjectBrowserDownloadState) -> Html {
    match state {
        ObjectBrowserDownloadState::Idle => html! {},
        ObjectBrowserDownloadState::Starting { label } => html! {
            <div class="dos-object-browser-message" data-download-state="starting">
                <span class="dos-card-label">{ "Preparing download" }</span>
                <p>{ format!("{label} is being requested from the daemon-authorized Web API.") }</p>
            </div>
        },
        ObjectBrowserDownloadState::Started { filename, detail } => html! {
            <div class="dos-object-browser-message" data-download-state="started">
                <span class="dos-card-label">{ "Download started" }</span>
                <p>{ format!("{filename} has been sent to the browser download manager. {detail}") }</p>
            </div>
        },
        ObjectBrowserDownloadState::PermissionDenied { message } => html! {
            <div class="dos-object-browser-message" data-download-state="permission-denied">
                <span class="dos-card-label">{ "Permission denied" }</span>
                <p>{ message }</p>
            </div>
        },
        ObjectBrowserDownloadState::Error { message } => html! {
            <div class="dos-object-browser-message" data-download-state="error">
                <span class="dos-card-label">{ "Download failed" }</span>
                <p>{ message }</p>
            </div>
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_message(label: &str, message: &str) -> Html {
    html! {
        <div class="dos-object-browser-message">
            <span class="dos-card-label">{ label }</span>
            <p>{ message }</p>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_body(
    response: &ObjectBrowserResponse,
    browser_prefix: UseStateHandle<String>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    let folders = object_browser_folder_summaries(&response.folders);
    let files = object_browser_file_summaries(&response.files);
    html! {
        <div class="dos-object-browser-body" data-endpoint={response.endpoint.clone()} data-prefix={response.prefix.clone()}>
            { render_object_browser_breadcrumbs(response, browser_prefix.clone()) }
            <div class="dos-object-browser-summary">
                <span>{ format!("{} folder(s)", folders.len()) }</span>
                <span>{ format!("{} file(s)", files.len()) }</span>
                <span>{ response.total_entries.map(|entries| format!("{entries} total entries")).unwrap_or_else(|| "total pending".to_string()) }</span>
            </div>
            { render_object_browser_folders(
                folders,
                response.endpoint.clone(),
                api_base_path.clone(),
                browser_prefix.clone(),
                browser_download_state.clone(),
            ) }
            { render_object_browser_files(
                files,
                response.endpoint.clone(),
                api_base_path,
                browser_download_state,
            ) }
            {
                if response.next_cursor.is_some() {
                    html! { <p class="dos-object-browser-note">{ "More entries are available; pagination controls will be enabled in the download/action slice." }</p> }
                } else {
                    html! {}
                }
            }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_breadcrumbs(
    response: &ObjectBrowserResponse,
    browser_prefix: UseStateHandle<String>,
) -> Html {
    let root_click = {
        let browser_prefix = browser_prefix.clone();
        Callback::from(move |_| browser_prefix.set(String::new()))
    };
    html! {
        <nav class="dos-object-browser-breadcrumbs" aria-label="ObjectStore folder path">
            <button type="button" onclick={root_click}>{ response.endpoint.clone() }</button>
            { for response.breadcrumbs.iter().map(|breadcrumb| {
                let prefix = breadcrumb.prefix.clone();
                let label = breadcrumb.name.clone();
                let browser_prefix = browser_prefix.clone();
                html! {
                    <button type="button" onclick={Callback::from(move |_| browser_prefix.set(prefix.clone()))}>{ label }</button>
                }
            }) }
        </nav>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_folders(
    folders: Vec<ObjectBrowserFolderSummary>,
    endpoint: String,
    api_base_path: String,
    browser_prefix: UseStateHandle<String>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    if folders.is_empty() {
        return html! {};
    }
    html! {
        <div class="dos-object-browser-folders">
            { for folders.into_iter().map(|folder| {
                let name = folder.name.clone();
                let objects = folder.objects.clone();
                let size = folder.size.clone();
                let readiness = folder.readiness.clone();
                let prefix = folder.prefix.clone();
                let download_prefix = folder.prefix.clone();
                let download_enabled = object_browser_folder_download_available(&readiness);
                let download_title = object_browser_download_disabled_reason(&readiness, &[]);
                let browser_prefix = browser_prefix.clone();
                let endpoint = endpoint.clone();
                let api_base_path = api_base_path.clone();
                let browser_download_state = browser_download_state.clone();
                html! {
                    <div class="dos-object-browser-folder">
                        <button type="button" class="dos-object-browser-folder-open" onclick={Callback::from(move |_| browser_prefix.set(prefix.clone()))}>
                            <strong>{ name.clone() }</strong>
                        </button>
                        <span>{ objects.clone() }</span>
                        <span>{ size.clone() }</span>
                        <span class="dos-status-pill">{ readiness }</span>
                        <button
                            type="button"
                            class="dos-object-browser-download"
                            disabled={!download_enabled}
                            title={download_title}
                            onclick={Callback::from(move |_| {
                                let confirmed = confirm_large_folder_download(&download_prefix, &objects, &size);
                                if confirmed {
                                    start_object_browser_download(
                                        api_base_path.clone(),
                                        endpoint.clone(),
                                        download_prefix.clone(),
                                        true,
                                        format!("folder {}", download_prefix),
                                        format!("{name}.tar.gz"),
                                        browser_download_state.clone(),
                                    );
                                }
                            })}
                        >
                            { "Download folder" }
                        </button>
                    </div>
                }
            }) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_files(
    files: Vec<ObjectBrowserFileSummary>,
    endpoint: String,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    if files.is_empty() {
        return render_object_browser_message("Files", "No files in this folder.");
    }
    html! {
        <div class="dos-object-browser-table-wrap">
            <table class="dos-object-browser-table">
                <thead>
                    <tr>
                        <th>{ "Name" }</th>
                        <th>{ "Type" }</th>
                        <th>{ "Size" }</th>
                        <th>{ "Readiness" }</th>
                        <th>{ "Lifecycle" }</th>
                        <th>{ "Copies" }</th>
                        <th>{ "Placement" }</th>
                        <th>{ "Modified" }</th>
                        <th>{ "Actions" }</th>
                    </tr>
                </thead>
                <tbody>
                    { for files.into_iter().map(|file| {
                        let download_enabled = object_browser_file_download_available(&file.readiness, &file.placements);
                        let download_title = object_browser_download_disabled_reason(&file.readiness, &file.placements);
                        let object_id = file.object_id.clone();
                        let label = file.name.clone();
                        let fallback_filename = file.name.clone();
                        let endpoint = endpoint.clone();
                        let api_base_path = api_base_path.clone();
                        let browser_download_state = browser_download_state.clone();
                        html! {
                            <tr title={file.path.clone()}>
                                <td><strong>{ file.name }</strong><span>{ file.object_id }</span></td>
                                <td>{ file.object_type }</td>
                                <td>{ file.size }</td>
                                <td><span class="dos-status-pill" data-state={object_browser_state_key(&file.readiness)}>{ file.readiness }</span></td>
                                <td>{ file.lifecycle }</td>
                                <td>{ file.copies }</td>
                                <td>{ render_object_browser_placements(&file.placement_summary, &file.placements) }</td>
                                <td>{ file.modified }</td>
                                <td>
                                    <button
                                        type="button"
                                        class="dos-object-browser-download"
                                        disabled={!download_enabled}
                                        title={download_title}
                                        onclick={Callback::from(move |_| {
                                            start_object_browser_download(
                                                api_base_path.clone(),
                                                endpoint.clone(),
                                                object_id.clone(),
                                                false,
                                                label.clone(),
                                                fallback_filename.clone(),
                                                browser_download_state.clone(),
                                            );
                                        })}
                                    >
                                        { "Download" }
                                    </button>
                                </td>
                            </tr>
                        }
                    }) }
                </tbody>
            </table>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_browser_placements(
    summary: &str,
    placements: &[ObjectBrowserPlacementResponse],
) -> Html {
    if placements.is_empty() {
        return html! {
            <div class="dos-object-browser-placement-stack">
                <span class="dos-object-browser-placement-summary" data-state="pending">{ summary }</span>
                <span class="dos-object-browser-placement" data-state="pending">{ "placement pending" }</span>
            </div>
        };
    }
    html! {
        <div class="dos-object-browser-placement-stack">
            <span class="dos-object-browser-placement-summary" data-state={object_browser_placement_summary_state(placements)}>{ summary }</span>
            <div class="dos-object-browser-placements">
                { for placements.iter().map(|placement| {
                    let location = labelize_state(&placement.location);
                    let state = labelize_state(&placement.state);
                    let disk = placement
                        .disk_label
                        .as_deref()
                        .or(placement.disk_id.as_deref())
                        .unwrap_or("external endpoint");
                    let size = format_browser_bytes(placement.size_bytes);
                    html! {
                        <span
                            class="dos-object-browser-placement"
                            data-location={placement.location.clone()}
                            data-state={placement.state.clone()}
                            title={format!("{} · {} · {} · {}", disk, location, state, size)}
                        >
                            { format!("{} · {} · {} · {}", disk, location, state, size) }
                        </span>
                    }
                }) }
            </div>
        </div>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_placement_summary_state(placements: &[ObjectBrowserPlacementResponse]) -> String {
    if placements
        .iter()
        .any(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
    {
        "degraded".to_string()
    } else if placements
        .iter()
        .any(|placement| placement.location == "ssd_landing")
        && !placements
            .iter()
            .any(|placement| placement.location == "hdd_settled" && placement.state == "verified")
    {
        "ssd_only".to_string()
    } else if placements
        .iter()
        .any(|placement| placement.state == "pending")
    {
        "pending".to_string()
    } else {
        "verified".to_string()
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_file_download_available(
    readiness: &str,
    placements: &[ObjectBrowserPlacementResponse],
) -> bool {
    readiness.eq_ignore_ascii_case("Available")
        && placements
            .iter()
            .any(|placement| placement.location == "hdd_settled" && placement.state == "verified")
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_folder_download_available(readiness: &str) -> bool {
    readiness.eq_ignore_ascii_case("Available")
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_browser_download_disabled_reason(
    readiness: &str,
    placements: &[ObjectBrowserPlacementResponse],
) -> String {
    let readiness_key = object_browser_state_key(readiness);
    if readiness_key == "redownload_required" {
        return "Download disabled: daemon metadata marks this object redownload-required."
            .to_string();
    }
    if readiness_key == "unavailable" {
        return "Download disabled: no available local or external object copy is reported."
            .to_string();
    }
    if readiness_key == "ssd_only" {
        return "Download disabled until the object has a verified settled HDD copy.".to_string();
    }
    if readiness_key == "degraded" {
        return "Download disabled until degraded or missing placements are repaired.".to_string();
    }
    if readiness_key != "available" {
        return format!(
            "Download disabled until daemon readiness is Available; current state is {readiness}."
        );
    }
    if placements
        .iter()
        .any(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
    {
        return "Download disabled because at least one placement is degraded or missing."
            .to_string();
    }
    if !placements.is_empty()
        && !placements
            .iter()
            .any(|placement| placement.location == "hdd_settled" && placement.state == "verified")
    {
        if placements
            .iter()
            .any(|placement| placement.location == "ssd_landing")
        {
            return "Download disabled: only SSD landing placement is currently reported."
                .to_string();
        }
        return "Download disabled until a verified settled HDD copy is available.".to_string();
    }
    "Download through the daemon-authorized Web API.".to_string()
}

#[cfg(target_arch = "wasm32")]
fn confirm_large_folder_download(prefix: &str, objects: &str, size: &str) -> bool {
    let large = objects
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|count| count >= 100)
        || size.ends_with("GiB")
        || size.ends_with("TiB");
    if !large {
        return true;
    }
    web_sys::window()
        .and_then(|window| {
            window
                .confirm_with_message(&format!(
                    "Prepare archive download for folder {prefix} ({objects}, {size})?"
                ))
                .ok()
        })
        .unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
fn start_object_browser_download(
    api_base_path: String,
    endpoint: String,
    object_or_prefix: String,
    folder: bool,
    label: String,
    fallback_filename: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) {
    browser_download_state.set(ObjectBrowserDownloadState::Starting {
        label: label.clone(),
    });
    wasm_bindgen_futures::spawn_local(async move {
        let path = if folder {
            crate::api::object_folder_download_api_path(
                &api_base_path,
                &endpoint,
                &object_or_prefix,
            )
        } else {
            crate::api::object_download_api_path(&api_base_path, &endpoint, &object_or_prefix)
        };
        match crate::api::download_object_browser_asset(&path, &fallback_filename).await {
            Ok(download) => {
                let detail = object_browser_download_detail(&download);
                match download_bytes_to_host(
                    &download.filename,
                    &download.bytes,
                    &download.content_type,
                ) {
                    Ok(()) => browser_download_state.set(ObjectBrowserDownloadState::Started {
                        filename: download.filename,
                        detail,
                    }),
                    Err(message) => {
                        browser_download_state.set(ObjectBrowserDownloadState::Error { message })
                    }
                }
            }
            Err(error) if error.is_permission_denied() => {
                browser_download_state.set(ObjectBrowserDownloadState::PermissionDenied {
                    message: error.message,
                });
            }
            Err(error) => {
                browser_download_state.set(ObjectBrowserDownloadState::Error {
                    message: error.message,
                });
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn object_browser_download_detail(download: &crate::api::ObjectBrowserDownload) -> String {
    if let Some(files) = download.archive_files {
        let bytes = download
            .archive_source_bytes
            .or(download.content_length)
            .map(format_browser_bytes)
            .unwrap_or_else(|| "size pending".to_string());
        format!("Archive preflight reported {files} file(s), {bytes}.")
    } else {
        download
            .content_length
            .map(|bytes| format!("Reported size: {}.", format_browser_bytes(bytes)))
            .unwrap_or_else(|| "Reported size pending.".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_create_card(
    view: Option<&ObjectStoresPageResponse>,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    api_base_path: String,
) -> Html {
    let (status, detail) = match view {
        Some(view) if view.create_object_store.enabled => (
            "Available".to_string(),
            "Admin workflow: create an enclosure-anchored Generated Data ObjectStore after daemon plan review.".to_string(),
        ),
        Some(view) => (
            "Admin only".to_string(),
            view.create_object_store
                .blocked_reason
                .clone()
                .unwrap_or_else(|| {
                    "Admin workflow: assign a writer group, choose enclosure, object type, and redundancy, then submit the daemon creation plan.".to_string()
                }),
        ),
        None => (
            "Admin only".to_string(),
            "Admin workflow: assign a writer group, choose enclosure, object type, and redundancy, then submit the daemon creation plan.".to_string(),
        ),
    };
    let state = (*create_state).clone();
    let enabled = view.is_some_and(|view| view.create_object_store.enabled);
    let store_class_options = view
        .map(|view| view.create_object_store.store_class_options.clone())
        .unwrap_or_default();
    let copy_count_options = view
        .map(|view| view.create_object_store.copy_count_options.clone())
        .filter(|options| !options.is_empty())
        .unwrap_or_else(|| vec![1, 2, 3]);
    let group_options = view.map(|view| view.groups.clone()).unwrap_or_default();
    let enclosure_options = view
        .map(|view| view.mounted_enclosures.clone())
        .unwrap_or_default();
    let can_plan = enabled
        && !state.planning
        && object_store_creation_fields_ready(
            &state.store_id,
            &state.writer_group,
            &state.enclosure_id,
        );
    let can_submit = enabled
        && state.plan.is_some()
        && !state.submitting
        && object_store_create_confirmation_matches(&state.confirmation_phrase);

    let open_form = {
        let create_state = create_state.clone();
        let initial = ObjectStoreCreateFormState::from_view(view);
        Callback::from(move |_| {
            let mut next = (*create_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            create_state.set(next);
        })
    };
    let plan = {
        let create_state = create_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_state).clone();
            let bucket = if pending.bucket.trim().is_empty() {
                object_store_bucket_default(&pending.store_id)
            } else {
                pending.bucket.trim().to_string()
            };
            pending.bucket = bucket.clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            create_state.set(pending.clone());

            let create_state = create_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "store_create".to_string(),
                    store_id: Some(pending.store_id.trim().to_string()),
                    store_class: Some(pending.store_class.clone()),
                    store_copies: Some(pending.required_copies),
                    bucket: (!bucket.is_empty()).then_some(bucket),
                    writer_group: Some(pending.writer_group.trim().to_string()),
                    ssd_root: Some(pending.ssd_root.trim().to_string()),
                    public: Some(pending.public),
                    writeable: Some(pending.writeable),
                    capacity_behavior: Some(pending.capacity_behavior.clone()),
                    retention: Some(pending.retention.clone()),
                    endpoint_export_mode: Some(pending.endpoint_export_mode.clone()),
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
                };
                let result = crate::api::plan_gui_action(&api_base_path, &request).await;
                let mut next = (*create_state).clone();
                next.planning = false;
                match result {
                    Ok(plan) => {
                        next.plan = Some(plan);
                        next.error = None;
                    }
                    Err(error) => {
                        next.plan = None;
                        next.error = Some(error.message);
                    }
                }
                create_state.set(next);
            });
        })
    };
    let submit = {
        let create_state = create_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_state).clone();
            pending.submitting = true;
            pending.error = None;
            pending.submitted = None;
            create_state.set(pending.clone());

            let create_state = create_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = object_store_create_request_from_state(&pending);
                let result = crate::api::submit_object_store_create(&api_base_path, &request).await;
                let mut next = (*create_state).clone();
                next.submitting = false;
                match result {
                    Ok(response) => {
                        next.submitted = Some(response);
                        next.error = None;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                create_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-objectstore-create" data-action="store_create">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create ObjectStore" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close form" } else { "Configure store" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        { object_store_text_field("Store name", state.store_id.clone(), {
                            let create_state = create_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*create_state).clone();
                                next.store_id = input.value();
                                next.bucket = object_store_bucket_default(&next.store_id);
                                next.reset_plan();
                                create_state.set(next);
                            })
                        }) }
                        <label class="dos-form-field">
                            <span>{ "S3 bucket" }</span>
                            <input readonly=true value={object_store_bucket_default(&state.store_id)} />
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Writer group" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.writer_group = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.writer_group.clone()}>
                                <option value="">{ "Select writer group" }</option>
                                { for group_options.iter().map(|group| html! {
                                    <option value={group.group_name.clone()}>
                                        { format!("{} ({})", group.display_name, group.group_name) }
                                    </option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Enclosure" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                let enclosure_options = enclosure_options.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let selected = input.value();
                                    let mut next = (*create_state).clone();
                                    next.enclosure_id = selected.clone();
                                    if let Some(enclosure) = enclosure_options
                                        .iter()
                                        .find(|enclosure| enclosure.enclosure_id == selected)
                                    {
                                        next.ssd_root = enclosure_ssd_root(enclosure);
                                    }
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.enclosure_id.clone()}>
                                <option value="">{ "Select mounted enclosure" }</option>
                                { for enclosure_options.iter().map(|enclosure| html! {
                                    <option value={enclosure.enclosure_id.clone()}>
                                        { format!("{} ({})", enclosure.display_name, enclosure.enclosure_id) }
                                    </option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Object type" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.object_type = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.object_type.clone()}>
                                { for ["naive", "bam", "cram", "pod5", "fastq", "fastq_gz", "fasta", "vcf", "bcf", "gff", "gtf", "ena_sra"].iter().map(|value| html! {
                                    <option value={(*value).to_string()}>{ *value }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Store class" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.store_class = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.store_class.clone()}>
                                { for store_class_options.iter().map(|option| html! {
                                    <option value={option.value.clone()}>{ option.label.clone() }</option>
                                }) }
                                if store_class_options.is_empty() {
                                    <option value={state.store_class.clone()}>{ state.store_class.clone() }</option>
                                }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Redundancy" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.required_copies = input.value().parse::<u8>().unwrap_or(1);
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.required_copies.to_string()}>
                                { for copy_count_options.iter().map(|copies| html! {
                                    <option value={copies.to_string()}>{ format!("{copies} copy/copies") }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Export mode" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.endpoint_export_mode = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.endpoint_export_mode.clone()}>
                                <option value="s3_bucket">{ "S3 bucket" }</option>
                                <option value="read_only_export">{ "Read-only export" }</option>
                                <option value="internal_only">{ "Internal only" }</option>
                            </select>
                        </label>
                    </div>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input type="checkbox" checked={state.public} onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.public = input.checked();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} />
                            <span>{ "Publicly visible within the appliance policy boundary" }</span>
                        </label>
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Derived policy" }</span>
                        <p>{ format!("SSD root: {}", if state.ssd_root.is_empty() { "select an enclosure".to_string() } else { state.ssd_root.clone() }) }</p>
                        <p>{ "Capacity is distributed by available space, retention is until explicitly deleted, and the store remains writeable by its writer group until locked after population." }</p>
                    </section>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Creation review" }</span>
                        <p>{ object_store_create_review(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review daemon plan" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                        <label class="dos-form-field">
                            <span>{ "Confirmation phrase" }</span>
                            <input
                                placeholder="confirm create objectstore"
                                value={state.confirmation_phrase.clone()}
                                oninput={{
                                    let create_state = create_state.clone();
                                    Callback::from(move |event: InputEvent| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*create_state).clone();
                                        next.confirmation_phrase = input.value();
                                        next.submitted = None;
                                        create_state.set(next);
                                    })
                                }}
                            />
                        </label>
                        <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                            { if state.submitting { "Submitting..." } else { "Submit daemon job" } }
                        </button>
                        if let Some(submitted) = &state.submitted {
                            <section class="dos-plan-result" data-job-state="accepted">
                                <span class="dos-card-label">{ "Daemon job accepted" }</span>
                                <h3>{ "ObjectStore creation submitted to dasobjectstored." }</h3>
                                <p>{ format!("Job {} · {} · dry run {}", submitted.accepted.job_id, submitted.accepted.kind, submitted.accepted.dry_run) }</p>
                                <code>{ format!("{} · class {} · {} copy/copies · writer group {} · actor {}", submitted.store_id, submitted.store_class, submitted.required_copies, submitted.writer_group, submitted.administrator_actor.as_deref().unwrap_or("unknown")) }</code>
                            </section>
                        }
                    </section>
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_configure_card(
    view: &ObjectStoresPageResponse,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    api_base_path: String,
) -> Html {
    let state = (*configure_state).clone();
    let enabled = view.create_object_store.enabled && !view.stores.is_empty();
    let status = if enabled {
        "Available"
    } else if view.stores.is_empty() {
        "No stores"
    } else {
        "Admin only"
    };
    let detail = if enabled {
        "Select an existing ObjectStore and review policy changes before daemon-owned execution."
    } else if view.stores.is_empty() {
        "Configuration is available after at least one ObjectStore exists."
    } else {
        view.create_object_store
            .blocked_reason
            .as_deref()
            .unwrap_or("Current user must be an administrator to configure ObjectStores.")
    };
    let group_options = view.groups.clone();
    let store_class_options = view.create_object_store.store_class_options.clone();
    let copy_count_options = if view.create_object_store.copy_count_options.is_empty() {
        vec![1, 2, 3]
    } else {
        view.create_object_store.copy_count_options.clone()
    };
    let can_plan = enabled
        && !state.planning
        && !state.selected_store_id.trim().is_empty()
        && !state.store_class.trim().is_empty()
        && !state.writer_group.trim().is_empty();

    let open_form = {
        let configure_state = configure_state.clone();
        let initial = ObjectStoreConfigureFormState::from_view(Some(view));
        Callback::from(move |_| {
            let mut next = (*configure_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            configure_state.set(next);
        })
    };

    let plan = {
        let configure_state = configure_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*configure_state).clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            configure_state.set(pending.clone());

            let configure_state = configure_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "store_configure".to_string(),
                    store_id: Some(pending.selected_store_id.trim().to_string()),
                    store_class: Some(pending.store_class.clone()),
                    store_copies: Some(pending.required_copies),
                    bucket: None,
                    writer_group: Some(pending.writer_group.trim().to_string()),
                    ssd_root: Some(pending.ssd_root.trim().to_string()),
                    public: Some(pending.public),
                    writeable: Some(pending.writeable),
                    capacity_behavior: Some(pending.capacity_behavior.clone()),
                    retention: Some(pending.retention.clone()),
                    endpoint_export_mode: Some(pending.endpoint_export_mode.clone()),
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
                };
                let result = crate::api::plan_gui_action(&api_base_path, &request).await;
                let mut next = (*configure_state).clone();
                next.planning = false;
                match result {
                    Ok(plan) => {
                        next.plan = Some(plan);
                        next.error = None;
                    }
                    Err(error) => {
                        next.plan = None;
                        next.error = Some(error.message);
                    }
                }
                configure_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-objectstore-configure" data-action="store_configure">
            <span class="dos-create-mark">{ "!" }</span>
            <h2>{ "Configure ObjectStore" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close policy editor" } else { "Edit policy" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        <label class="dos-form-field">
                            <span>{ "ObjectStore" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                let stores = view.stores.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    if let Some(store) = stores.iter().find(|store| store.store_id == input.value()) {
                                        next.apply_store(store);
                                    } else {
                                        next.selected_store_id = input.value();
                                        next.reset_plan();
                                    }
                                    configure_state.set(next);
                                })
                            }} value={state.selected_store_id.clone()}>
                                { for view.stores.iter().map(|store| html! {
                                    <option value={store.store_id.clone()}>{ format!("{} ({})", store.display_name, store.store_id) }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Store class" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.store_class = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.store_class.clone()}>
                                { for store_class_options.iter().map(|option| html! {
                                    <option value={option.value.clone()}>{ option.label.clone() }</option>
                                }) }
                                if store_class_options.is_empty() {
                                    <option value={state.store_class.clone()}>{ state.store_class.clone() }</option>
                                }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Redundancy" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.required_copies = input.value().parse::<u8>().unwrap_or(1);
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.required_copies.to_string()}>
                                { for copy_count_options.iter().map(|copies| html! {
                                    <option value={copies.to_string()}>{ format!("{copies} copy/copies") }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Writer group" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.writer_group = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.writer_group.clone()}>
                                <option value="">{ "Select writer group" }</option>
                                { for group_options.iter().map(|group| html! {
                                    <option value={group.group_name.clone()}>
                                        { format!("{} ({})", group.display_name, group.group_name) }
                                    </option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Capacity behavior" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.capacity_behavior = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.capacity_behavior.clone()}>
                                <option value="reject_writes">{ "Reject writes" }</option>
                                <option value="backpressure_by_priority">{ "Backpressure by priority" }</option>
                                <option value="mark_redownload_required">{ "Mark redownload required" }</option>
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Retention" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.retention = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.retention.clone()}>
                                <option value="immediate_delete">{ "Immediate delete" }</option>
                                <option value="tombstone_then_gc">{ "Tombstone then GC" }</option>
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Export mode" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.endpoint_export_mode = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.endpoint_export_mode.clone()}>
                                <option value="s3">{ "S3" }</option>
                                <option value="read_only_file_export">{ "Read-only file export" }</option>
                                <option value="disabled">{ "Disabled" }</option>
                            </select>
                        </label>
                        { object_store_text_field("SSD root", state.ssd_root.clone(), {
                            let configure_state = configure_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*configure_state).clone();
                                next.ssd_root = input.value();
                                next.reset_plan();
                                configure_state.set(next);
                            })
                        }) }
                    </div>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input type="checkbox" checked={state.public} onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.public = input.checked();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} />
                            <span>{ "Publicly visible within the appliance policy boundary" }</span>
                        </label>
                        <label>
                            <input type="checkbox" checked={state.writeable} onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.writeable = input.checked();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} />
                            <span>{ "Writeable by the selected writer group" }</span>
                        </label>
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Configuration review" }</span>
                        <p>{ object_store_configure_review(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review configuration plan" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                    </section>
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_subobject_create_card(
    view: &ObjectStoresPageResponse,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
) -> Html {
    let state = (*subobject_state).clone();
    let enabled = view.create_object_store.enabled && !view.stores.is_empty();
    let status = if enabled {
        "Available"
    } else if view.stores.is_empty() {
        "No stores"
    } else {
        "Admin only"
    };
    let detail = if enabled {
        "Create a named SubObject endpoint with parent selection, nested prefix preview, object-type policy, and S3 routing review."
    } else if view.stores.is_empty() {
        "SubObjects require an existing parent ObjectStore."
    } else {
        view.create_object_store
            .blocked_reason
            .as_deref()
            .unwrap_or("Current user must be an administrator to create SubObjects.")
    };
    let can_plan = enabled
        && !state.planning
        && !state.subobject_name.trim().is_empty()
        && ((state.parent_kind == "store" && !state.parent_store_id.trim().is_empty())
            || (state.parent_kind == "subobject"
                && !state.parent_subobject_name.trim().is_empty()))
        && (state.object_type_mode == "inherit" || !state.object_type.trim().is_empty());

    let open_form = {
        let subobject_state = subobject_state.clone();
        let initial = SubObjectFormState::from_view(Some(view));
        Callback::from(move |_| {
            let mut next = (*subobject_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            subobject_state.set(next);
        })
    };

    let plan = {
        let subobject_state = subobject_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*subobject_state).clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            subobject_state.set(pending.clone());

            let subobject_state = subobject_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "subobject_create".to_string(),
                    store_id: None,
                    store_class: None,
                    store_copies: None,
                    bucket: None,
                    writer_group: None,
                    ssd_root: (!pending.ssd_root.trim().is_empty())
                        .then(|| pending.ssd_root.trim().to_string()),
                    public: None,
                    writeable: None,
                    capacity_behavior: None,
                    retention: None,
                    endpoint_export_mode: None,
                    subobject_name: Some(pending.subobject_name.trim().to_string()),
                    parent_store_id: (pending.parent_kind == "store")
                        .then(|| pending.parent_store_id.trim().to_string()),
                    parent_subobject_name: (pending.parent_kind == "subobject")
                        .then(|| pending.parent_subobject_name.trim().to_string()),
                    subobject_object_type: (pending.object_type_mode == "override")
                        .then(|| pending.object_type.clone()),
                    subobject_inherits_object_type: Some(pending.object_type_mode == "inherit"),
                    subobject_s3_routing: Some(pending.s3_routing.clone()),
                    ssd_device: None,
                    hdd_devices: Vec::new(),
                    mount_root: None,
                    filesystem: None,
                    owner: None,
                    allow_format: false,
                    existing_data_acknowledged: false,
                    confirmation_phrase: None,
                };
                let result = crate::api::plan_gui_action(&api_base_path, &request).await;
                let mut next = (*subobject_state).clone();
                next.planning = false;
                match result {
                    Ok(plan) => {
                        next.plan = Some(plan);
                        next.error = None;
                    }
                    Err(error) => {
                        next.plan = None;
                        next.error = Some(error.message);
                    }
                }
                subobject_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-subobject-create" data-action="subobject_create">
            <span class="dos-create-mark">{ "/" }</span>
            <h2>{ "Create SubObject" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close SubObject form" } else { "Define SubObject" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        { object_store_text_field("SubObject name", state.subobject_name.clone(), {
                            let subobject_state = subobject_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*subobject_state).clone();
                                next.subobject_name = input.value();
                                next.reset_plan();
                                subobject_state.set(next);
                            })
                        }) }
                        <label class="dos-form-field">
                            <span>{ "Parent type" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.parent_kind = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.parent_kind.clone()}>
                                <option value="store">{ "ObjectStore" }</option>
                                <option value="subobject">{ "SubObject" }</option>
                            </select>
                        </label>
                        if state.parent_kind == "store" {
                            <label class="dos-form-field">
                                <span>{ "Parent ObjectStore" }</span>
                                <select onchange={{
                                    let subobject_state = subobject_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlSelectElement = event.target_unchecked_into();
                                        let mut next = (*subobject_state).clone();
                                        next.parent_store_id = input.value();
                                        next.reset_plan();
                                        subobject_state.set(next);
                                    })
                                }} value={state.parent_store_id.clone()}>
                                    { for view.stores.iter().map(|store| html! {
                                        <option value={store.store_id.clone()}>{ format!("{} ({})", store.display_name, store.store_id) }</option>
                                    }) }
                                </select>
                            </label>
                        } else {
                            { object_store_text_field("Parent SubObject", state.parent_subobject_name.clone(), {
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.parent_subobject_name = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }) }
                        }
                        <label class="dos-form-field">
                            <span>{ "Object type policy" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.object_type_mode = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.object_type_mode.clone()}>
                                <option value="inherit">{ "Inherit from parent/import" }</option>
                                <option value="override">{ "Override for this SubObject" }</option>
                            </select>
                        </label>
                        if state.object_type_mode == "override" {
                            <label class="dos-form-field">
                                <span>{ "Object type" }</span>
                                <select onchange={{
                                    let subobject_state = subobject_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlSelectElement = event.target_unchecked_into();
                                        let mut next = (*subobject_state).clone();
                                        next.object_type = input.value();
                                        next.reset_plan();
                                        subobject_state.set(next);
                                    })
                                }} value={state.object_type.clone()}>
                                    { for ["naive", "bam", "cram", "pod5", "fastq", "fastq_gz", "fasta", "vcf", "bcf", "gff", "gtf", "ena_sra"].iter().map(|value| html! {
                                        <option value={(*value).to_string()}>{ *value }</option>
                                    }) }
                                </select>
                            </label>
                        }
                        <label class="dos-form-field">
                            <span>{ "S3 routing" }</span>
                            <select onchange={{
                                let subobject_state = subobject_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*subobject_state).clone();
                                    next.s3_routing = input.value();
                                    next.reset_plan();
                                    subobject_state.set(next);
                                })
                            }} value={state.s3_routing.clone()}>
                                <option value="inherit_parent">{ "Inherit parent route" }</option>
                                <option value="dedicated_prefix">{ "Dedicated S3 prefix" }</option>
                                <option value="dedicated_bucket">{ "Dedicated bucket" }</option>
                                <option value="disabled">{ "No S3 route" }</option>
                            </select>
                        </label>
                        { object_store_text_field("SSD root", state.ssd_root.clone(), {
                            let subobject_state = subobject_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*subobject_state).clone();
                                next.ssd_root = input.value();
                                next.reset_plan();
                                subobject_state.set(next);
                            })
                        }) }
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Registry preview" }</span>
                        <p>{ subobject_registry_preview(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review SubObject plan" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                    </section>
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn object_store_text_field(
    label: &'static str,
    value: String,
    oninput: Callback<InputEvent>,
) -> Html {
    html! {
        <label class="dos-form-field">
            <span>{ label }</span>
            <input value={value} oninput={oninput} />
        </label>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn object_store_create_review_from_values(
    store_id: &str,
    object_type: &str,
    required_copies: u8,
    writer_group: &str,
    endpoint_export_mode: &str,
    public: bool,
    enclosure_id: &str,
) -> String {
    format!(
        "{} · type {} · {} copy/copies · writer group {} · enclosure {} · export {} · {} · writeable until locked",
        if store_id.trim().is_empty() {
            "unnamed store"
        } else {
            store_id.trim()
        },
        object_type,
        required_copies,
        if writer_group.trim().is_empty() {
            "pending"
        } else {
            writer_group.trim()
        },
        if enclosure_id.trim().is_empty() {
            "pending"
        } else {
            enclosure_id.trim()
        },
        endpoint_export_mode,
        if public { "public" } else { "private" }
    )
}

#[cfg(target_arch = "wasm32")]
fn object_store_create_review(state: &ObjectStoreCreateFormState) -> String {
    object_store_create_review_from_values(
        &state.store_id,
        &state.object_type,
        state.required_copies,
        &state.writer_group,
        &state.endpoint_export_mode,
        state.public,
        &state.enclosure_id,
    )
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_card(store: ObjectStoreCardSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-store-id={store.id.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ store.label }</span>
                <span class="dos-status-pill">{ store.health }</span>
            </div>
            <strong>{ store.name }</strong>
            <p>{ format!("type: {} · access: {}", store.object_type, store.access) }</p>
            <p>{ store.policy }</p>
            <p>{ store.capacity }</p>
            <p>{ format!("{} · writer group: {}", store.objects, store.writer_group) }</p>
            <p>{ store.writer_policy }</p>
            <p>{ format!("endpoint: {} · last ingest: {}", store.endpoint, store.last_ingested) }</p>
            <p>{ format!("{} warning(s)", store.warning_count) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_stores_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

pub fn users_groups_summary_cards(view: &UsersGroupsWorkspaceResponse) -> Vec<DashboardMetric> {
    let authority = view
        .current_user
        .as_ref()
        .map(|user| {
            if user.sudo_administrator {
                "sudo administrator".to_string()
            } else {
                "standard local user".to_string()
            }
        })
        .unwrap_or_else(|| "not reported".to_string());
    let username = view
        .current_user
        .as_ref()
        .map(|user| user.username.as_str())
        .unwrap_or("none");

    vec![
        DashboardMetric::new(
            "Authority adapter",
            &view.host_mode,
            "Prosopikon-aware host boundary for this appliance",
            "identity",
        ),
        DashboardMetric::new("Local actor", username, authority, "authority"),
        DashboardMetric::new(
            "Session principals",
            view.users.len().to_string(),
            "Local principals surfaced to the appliance capability map",
            "local",
        ),
        DashboardMetric::new(
            "OS groups",
            view.groups.len().to_string(),
            "Local groups visible for capability evaluation",
            "membership",
        ),
        DashboardMetric::new(
            "Capability groups",
            view.writer_groups.len().to_string(),
            format!("Local mapping registry: {}", view.groups_file_path),
            "policy",
        ),
        DashboardMetric::new(
            "Mapping actions",
            view.operations
                .iter()
                .filter(|operation| operation.enabled)
                .count()
                .to_string(),
            if view.capabilities.administrator_actions_enabled {
                "Local capability mapping is available."
            } else {
                "Capability mapping requires sudo-derived authority."
            },
            "readiness",
        ),
    ]
}

pub const LOCAL_GROUP_ADMIN_CONFIRMATION: &str = "confirm local group administration";

pub fn local_group_create_fields_ready(group_name: &str) -> bool {
    !group_name.trim().is_empty()
}

pub fn local_group_assignment_fields_ready(username: &str, group_name: &str) -> bool {
    !username.trim().is_empty() && !group_name.trim().is_empty()
}

pub fn local_group_admin_confirmation_matches(value: &str) -> bool {
    value.trim() == LOCAL_GROUP_ADMIN_CONFIRMATION
}

#[cfg(target_arch = "wasm32")]
fn users_groups_empty_workspace_message(view: &UsersGroupsWorkspaceResponse) -> Option<String> {
    (view.current_user.is_none() && view.users.is_empty() && view.writer_groups.is_empty()).then(
        || "No local identity or writer-policy state was returned by the appliance.".to_string(),
    )
}

#[cfg(any(target_arch = "wasm32", test))]
fn local_group_display_name(group_name: &str) -> String {
    let display_name = group_name
        .trim()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ");

    if display_name.is_empty() {
        group_name.trim().to_string()
    } else {
        display_name
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn users_groups_view_with_writer_group(
    mut view: UsersGroupsWorkspaceResponse,
    group_name: &str,
) -> UsersGroupsWorkspaceResponse {
    let group_name = group_name.trim();
    if group_name.is_empty() {
        return view;
    }

    let current_user_member = view
        .current_user
        .as_ref()
        .map(|user| user.groups.iter().any(|group| group == group_name))
        .unwrap_or(false);

    if let Some(group) = view
        .writer_groups
        .iter_mut()
        .find(|group| group.group_name == group_name)
    {
        group.current_user_member |= current_user_member;
    } else {
        view.writer_groups.push(crate::api::StorageGroupResponse {
            group_name: group_name.to_string(),
            display_name: local_group_display_name(group_name),
            source: "object_storage_group_registry".to_string(),
            current_user_member,
        });
    }

    view.writer_groups
        .sort_by(|left, right| left.display_name.cmp(&right.display_name));
    view.selected_group_name = Some(group_name.to_string());
    view
}

#[cfg(any(target_arch = "wasm32", test))]
fn users_groups_view_with_group_assignment(
    mut view: UsersGroupsWorkspaceResponse,
    username: &str,
    group_name: &str,
) -> UsersGroupsWorkspaceResponse {
    let username = username.trim();
    let group_name = group_name.trim();
    if username.is_empty() || group_name.is_empty() {
        return view;
    }

    if view
        .current_user
        .as_ref()
        .map(|user| user.username == username)
        .unwrap_or(false)
    {
        if let Some(user) = view.current_user.as_mut() {
            if !user.groups.iter().any(|group| group == group_name) {
                user.groups.push(group_name.to_string());
                user.groups.sort();
            }
        }
        for writer_group in &mut view.writer_groups {
            if writer_group.group_name == group_name {
                writer_group.current_user_member = true;
            }
        }
        for local_group in &mut view.groups {
            if local_group.group_name == group_name {
                local_group.current_user_member = true;
            }
        }
    }

    view.selected_username = Some(username.to_string());
    view.selected_group_name = Some(group_name.to_string());
    view
}

#[cfg(target_arch = "wasm32")]
fn users_groups_state_with_writer_group(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    group_name: &str,
) -> ApiLoadState<UsersGroupsWorkspaceResponse> {
    match state {
        ApiLoadState::Success(view) => ApiLoadState::Success(users_groups_view_with_writer_group(
            view.clone(),
            group_name,
        )),
        ApiLoadState::StaleData { value, message } => ApiLoadState::StaleData {
            value: users_groups_view_with_writer_group(value.clone(), group_name),
            message: message.clone(),
        },
        state => state.clone(),
    }
}

#[cfg(target_arch = "wasm32")]
fn users_groups_state_with_group_assignment(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    username: &str,
    group_name: &str,
) -> ApiLoadState<UsersGroupsWorkspaceResponse> {
    match state {
        ApiLoadState::Success(view) => ApiLoadState::Success(
            users_groups_view_with_group_assignment(view.clone(), username, group_name),
        ),
        ApiLoadState::StaleData { value, message } => ApiLoadState::StaleData {
            value: users_groups_view_with_group_assignment(value.clone(), username, group_name),
            message: message.clone(),
        },
        state => state.clone(),
    }
}

#[cfg(target_arch = "wasm32")]
fn refresh_users_groups_workspace(
    api_base_path: String,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
) {
    let path = users_groups_workspace_api_path(&api_base_path);
    wasm_bindgen_futures::spawn_local(async move {
        users_groups_state.set(page_load_state_from_result(
            crate::api::get_users_groups_workspace(&path).await,
            users_groups_empty_workspace_message,
        ));
    });
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct CreateLocalGroupFormState {
    group_name: String,
    previewing: bool,
    applying: bool,
    preview: Option<LocalGroupAdminResponse>,
    submitted: Option<LocalGroupAdminResponse>,
    confirmation_phrase: String,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl CreateLocalGroupFormState {
    fn new() -> Self {
        Self {
            group_name: String::new(),
            previewing: false,
            applying: false,
            preview: None,
            submitted: None,
            confirmation_phrase: String::new(),
            error: None,
        }
    }

    fn reset_result(&mut self) {
        self.preview = None;
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct AssignLocalUserFormState {
    username: String,
    group_name: String,
    previewing: bool,
    applying: bool,
    preview: Option<LocalGroupAdminResponse>,
    submitted: Option<LocalGroupAdminResponse>,
    confirmation_phrase: String,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl AssignLocalUserFormState {
    fn from_view(view: Option<&UsersGroupsWorkspaceResponse>) -> Self {
        Self {
            username: view
                .and_then(|view| view.current_user.as_ref())
                .map(|user| user.username.clone())
                .unwrap_or_default(),
            group_name: view
                .and_then(|view| view.writer_groups.first())
                .map(|group| group.group_name.clone())
                .unwrap_or_default(),
            previewing: false,
            applying: false,
            preview: None,
            submitted: None,
            confirmation_phrase: String::new(),
            error: None,
        }
    }

    fn reset_result(&mut self) {
        self.preview = None;
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct UsersGroupsPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(UsersGroupsPage)]
pub fn users_groups_page(props: &UsersGroupsPageProps) -> Html {
    let api_path = WorkspacePage::UsersGroups.api_path(&props.api_base_path);
    let users_groups_state = use_state(|| ApiLoadState::<UsersGroupsWorkspaceResponse>::Loading);
    let create_group_state = use_state(CreateLocalGroupFormState::new);
    let assign_user_state = use_state(|| AssignLocalUserFormState::from_view(None));

    {
        let api_path = api_path.clone();
        let users_groups_state = users_groups_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                users_groups_state.set(page_load_state_from_result(
                    crate::api::get_users_groups_workspace(&path).await,
                    users_groups_empty_workspace_message,
                ));
            });
            || ()
        });
    }

    {
        let assign_user_state = assign_user_state.clone();
        use_effect_with((*users_groups_state).clone(), move |state| {
            if let ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } = state
            {
                let mut next = (*assign_user_state).clone();
                let mut changed = false;
                if next.username.trim().is_empty() {
                    if let Some(user) = &view.current_user {
                        next.username = user.username.clone();
                        changed = true;
                    }
                }
                if next.group_name.trim().is_empty() {
                    if let Some(group) = view.writer_groups.first() {
                        next.group_name = group.group_name.clone();
                        changed = true;
                    }
                }
                if changed {
                    assign_user_state.set(next);
                }
            }
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="users-groups" data-api-route={api_path}>
            <PageHeader
                eyebrow="Prosopikon-aware appliance mapping"
                title="Local Capability Mapping"
                summary="Map Prosopikon-recognized local principals onto appliance OS groups and DASObjectStore writer/admin capabilities."
            />
            { render_users_groups_state(
                &*users_groups_state,
                users_groups_state.clone(),
                create_group_state,
                assign_user_state,
                props.api_base_path.clone(),
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_users_groups_state(
    state: &ApiLoadState<UsersGroupsWorkspaceResponse>,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    match state {
        ApiLoadState::Loading => render_users_groups_state_message(
            "Loading",
            "Loading capability mapping",
            "The Web console is requesting local principal, OS group, and writer-policy readiness.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_users_groups_workspace(
                view,
                users_groups_state,
                create_group_state,
                assign_user_state,
                api_base_path,
            )
        }
        ApiLoadState::Empty(message) => {
            render_users_groups_state_message("Inventory", "No capability mapping data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_users_groups_state_message(
            "Permission denied",
            "Capability mapping requires a standalone authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_users_groups_state_message("Error", "Unable to load capability mapping", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_users_groups_workspace(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    html! {
        <>
            <section class="dos-metric-grid">
                { for users_groups_summary_cards(view).into_iter().map(render_metric_card) }
            </section>
            <section class="dos-attention-grid">
                { render_create_local_group_card(
                    view,
                    users_groups_state.clone(),
                    create_group_state,
                    assign_user_state.clone(),
                    api_base_path.clone(),
                ) }
                { render_assign_local_user_card(
                    view,
                    users_groups_state,
                    assign_user_state,
                    api_base_path,
                ) }
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Local appliance actor" }</span>
                    if let Some(user) = &view.current_user {
                        <h2>{ &user.username }</h2>
                        <p>{ if user.sudo_administrator { "Sudo-derived capability administrator." } else { "Inspection-only local actor; Prosopikon remains the identity authority." } }</p>
                        <div class="dos-chip-row">
                            { for user.groups.iter().map(|group| html! {
                                <span class="dos-status-pill">{ group }</span>
                            }) }
                        </div>
                    } else {
                        <h2>{ "No current local user" }</h2>
                        <p>{ "The standalone session did not include OS-local authority metadata." }</p>
                    }
                </section>
                <section class="dos-card">
                    <span class="dos-card-label">{ "Capability groups" }</span>
                    <h2>{ format!("{} mapped group(s)", view.writer_groups.len()) }</h2>
                    <p>{ format!("Local registry: {}", view.groups_file_path) }</p>
                    <div class="dos-chip-row">
                        { for view.writer_groups.iter().map(|group| html! {
                            <span class="dos-status-pill">{ format!("{} · {}", group.display_name, if group.current_user_member { "member" } else { "not member" }) }</span>
                        }) }
                    </div>
                </section>
                <section class="dos-card">
                    <span class="dos-card-label">{ "Mapping readiness" }</span>
                    <h2>{ if view.capabilities.administrator_actions_enabled { "Ready" } else { "Not ready" } }</h2>
                    <p>{ if view.capabilities.os_local_group_management { "Local capability mapping is available for this session." } else { "Capability mapping is gated until sudo-derived authority is present." } }</p>
                    { for view.operations.iter().map(|operation| html! {
                        <p>{ format!("{}: {}", operation.label, if operation.enabled { "available" } else { operation.blocked_reason.as_deref().unwrap_or("blocked") }) }</p>
                    }) }
                </section>
            </section>
            if !view.warnings.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Warnings" }</span>
                    { for view.warnings.iter().map(|warning| html! {
                        <p>{ format!("{}: {}", warning.code, warning.message) }</p>
                    }) }
                </section>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_create_local_group_card(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    create_group_state: UseStateHandle<CreateLocalGroupFormState>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    let state = (*create_group_state).clone();
    let enabled = view.capabilities.os_local_group_management;
    let can_preview =
        enabled && !state.previewing && local_group_create_fields_ready(&state.group_name);
    let can_apply = enabled
        && state.preview.is_some()
        && !state.applying
        && local_group_admin_confirmation_matches(&state.confirmation_phrase);

    let on_group_name = {
        let create_group_state = create_group_state.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let mut next = (*create_group_state).clone();
            next.group_name = input.value();
            next.reset_result();
            create_group_state.set(next);
        })
    };
    let preview = {
        let create_group_state = create_group_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_group_state).clone();
            pending.previewing = true;
            pending.error = None;
            pending.preview = None;
            create_group_state.set(pending.clone());

            let create_group_state = create_group_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = CreateLocalGroupRequest {
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: true,
                    confirmation_marker: None,
                    client_request_id: None,
                };
                let result = crate::api::submit_create_local_group(&api_base_path, &request).await;
                let mut next = (*create_group_state).clone();
                next.previewing = false;
                match result {
                    Ok(response) => {
                        next.preview = Some(response);
                        next.submitted = None;
                        next.error = None;
                    }
                    Err(error) => {
                        next.preview = None;
                        next.error = Some(error.message);
                    }
                }
                create_group_state.set(next);
            });
        })
    };
    let apply = {
        let create_group_state = create_group_state.clone();
        let users_groups_state = users_groups_state.clone();
        let assign_user_state = assign_user_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_group_state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted = None;
            create_group_state.set(pending.clone());

            let create_group_state = create_group_state.clone();
            let users_groups_state = users_groups_state.clone();
            let assign_user_state = assign_user_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = CreateLocalGroupRequest {
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: false,
                    confirmation_marker: Some(LOCAL_GROUP_ADMIN_CONFIRMATION.to_string()),
                    client_request_id: None,
                };
                let result = crate::api::submit_create_local_group(&api_base_path, &request).await;
                let mut next = (*create_group_state).clone();
                next.applying = false;
                match result {
                    Ok(response) => {
                        let group_name = response.group_name.clone();
                        create_group_state.set(CreateLocalGroupFormState::new());
                        users_groups_state.set(users_groups_state_with_writer_group(
                            &*users_groups_state,
                            &group_name,
                        ));
                        let mut assign_next = (*assign_user_state).clone();
                        assign_next.group_name = group_name.clone();
                        assign_next.confirmation_phrase.clear();
                        assign_next.previewing = false;
                        assign_next.applying = false;
                        assign_next.reset_result();
                        assign_user_state.set(assign_next);
                        refresh_users_groups_workspace(api_base_path, users_groups_state);
                        return;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                create_group_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card" data-action="create_local_group">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create capability group" }</h2>
            <p>{ if enabled { "Preview a local OS group that maps Prosopikon-recognized principals to DASObjectStore writer/admin capabilities." } else { "Requires sudo-derived administrator authority." } }</p>
            <span class="dos-status-pill">{ if enabled { "Available" } else { "Admin only" } }</span>
            <label class="dos-form-field">
                <span>{ "Capability group" }</span>
                <input
                    type="text"
                    value={state.group_name.clone()}
                    placeholder="mnemosyne-writers"
                    oninput={on_group_name}
                    disabled={!enabled}
                />
            </label>
            <button class="dos-secondary-action" type="button" disabled={!can_preview} onclick={preview}>
                { if state.previewing { "Previewing..." } else { "Dry-run preview" } }
            </button>
            { render_local_group_admin_result("Preview", state.preview.as_ref()) }
            <label class="dos-form-field">
                <span>{ "Confirmation phrase" }</span>
                <input
                    type="text"
                    value={state.confirmation_phrase.clone()}
                    placeholder={LOCAL_GROUP_ADMIN_CONFIRMATION}
                    oninput={{
                        let create_group_state = create_group_state.clone();
                        Callback::from(move |event: InputEvent| {
                            let input: HtmlInputElement = event.target_unchecked_into();
                            let mut next = (*create_group_state).clone();
                            next.confirmation_phrase = input.value();
                            next.submitted = None;
                            create_group_state.set(next);
                        })
                    }}
                    disabled={!enabled}
                />
            </label>
            <button class="dos-auth-submit" type="button" disabled={!can_apply} onclick={apply}>
                { if state.applying { "Submitting..." } else { "Submit capability group" } }
            </button>
            { render_local_group_admin_result("Submitted", state.submitted.as_ref()) }
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_assign_local_user_card(
    view: &UsersGroupsWorkspaceResponse,
    users_groups_state: UseStateHandle<ApiLoadState<UsersGroupsWorkspaceResponse>>,
    assign_user_state: UseStateHandle<AssignLocalUserFormState>,
    api_base_path: String,
) -> Html {
    let state = (*assign_user_state).clone();
    let enabled = view.capabilities.os_local_group_management;
    let can_preview = enabled
        && !state.previewing
        && local_group_assignment_fields_ready(&state.username, &state.group_name);
    let can_apply = enabled
        && state.preview.is_some()
        && !state.applying
        && local_group_admin_confirmation_matches(&state.confirmation_phrase);
    let user_options = view.users.clone();
    let group_options = view.writer_groups.clone();

    let preview = {
        let assign_user_state = assign_user_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*assign_user_state).clone();
            pending.previewing = true;
            pending.error = None;
            pending.preview = None;
            assign_user_state.set(pending.clone());

            let assign_user_state = assign_user_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = AssignLocalUserToGroupRequest {
                    username: pending.username.trim().to_string(),
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: true,
                    confirmation_marker: None,
                    client_request_id: None,
                };
                let result =
                    crate::api::submit_assign_local_user_to_group(&api_base_path, &request).await;
                let mut next = (*assign_user_state).clone();
                next.previewing = false;
                match result {
                    Ok(response) => {
                        next.preview = Some(response);
                        next.submitted = None;
                        next.error = None;
                    }
                    Err(error) => {
                        next.preview = None;
                        next.error = Some(error.message);
                    }
                }
                assign_user_state.set(next);
            });
        })
    };
    let apply = {
        let assign_user_state = assign_user_state.clone();
        let users_groups_state = users_groups_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*assign_user_state).clone();
            pending.applying = true;
            pending.error = None;
            pending.submitted = None;
            assign_user_state.set(pending.clone());

            let assign_user_state = assign_user_state.clone();
            let users_groups_state = users_groups_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = AssignLocalUserToGroupRequest {
                    username: pending.username.trim().to_string(),
                    group_name: pending.group_name.trim().to_string(),
                    dry_run: false,
                    confirmation_marker: Some(LOCAL_GROUP_ADMIN_CONFIRMATION.to_string()),
                    client_request_id: None,
                };
                let result =
                    crate::api::submit_assign_local_user_to_group(&api_base_path, &request).await;
                let mut next = (*assign_user_state).clone();
                next.applying = false;
                match result {
                    Ok(response) => {
                        let username = response
                            .username
                            .clone()
                            .unwrap_or_else(|| pending.username.trim().to_string());
                        let group_name = response.group_name.clone();
                        next.submitted = Some(response);
                        next.confirmation_phrase.clear();
                        next.error = None;
                        users_groups_state.set(users_groups_state_with_group_assignment(
                            &*users_groups_state,
                            &username,
                            &group_name,
                        ));
                        refresh_users_groups_workspace(api_base_path, users_groups_state);
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                assign_user_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card" data-action="assign_local_user_to_group">
            <span class="dos-create-mark">{ "@" }</span>
            <h2>{ "Map principal to group" }</h2>
            <p>{ if enabled { "Preview a local principal-to-group mapping for appliance capability enforcement." } else { "Requires sudo-derived administrator authority." } }</p>
            <span class="dos-status-pill">{ if enabled { "Available" } else { "Admin only" } }</span>
            <label class="dos-form-field">
                <span>{ "Local principal" }</span>
                <input
                    type="text"
                    list="dos-local-users"
                    value={state.username.clone()}
                    placeholder="stephen"
                    oninput={{
                        let assign_user_state = assign_user_state.clone();
                        Callback::from(move |event: InputEvent| {
                            let input: HtmlInputElement = event.target_unchecked_into();
                            let mut next = (*assign_user_state).clone();
                            next.username = input.value();
                            next.reset_result();
                            assign_user_state.set(next);
                        })
                    }}
                    disabled={!enabled}
                />
                <datalist id="dos-local-users">
                    { for user_options.iter().map(|user| html! {
                        <option value={user.username.clone()} />
                    }) }
                </datalist>
            </label>
            <label class="dos-form-field">
                <span>{ "Capability group" }</span>
                <select onchange={{
                    let assign_user_state = assign_user_state.clone();
                    Callback::from(move |event: Event| {
                        let input: HtmlSelectElement = event.target_unchecked_into();
                        let mut next = (*assign_user_state).clone();
                        next.group_name = input.value();
                        next.reset_result();
                        assign_user_state.set(next);
                    })
                }} value={state.group_name.clone()} disabled={!enabled}>
                    <option value="">{ "Select group" }</option>
                    { for group_options.iter().map(|group| html! {
                        <option value={group.group_name.clone()}>{ format!("{} ({})", group.display_name, group.group_name) }</option>
                    }) }
                    if group_options.is_empty() && !state.group_name.is_empty() {
                        <option value={state.group_name.clone()}>{ state.group_name.clone() }</option>
                    }
                </select>
            </label>
            <button class="dos-secondary-action" type="button" disabled={!can_preview} onclick={preview}>
                { if state.previewing { "Previewing..." } else { "Dry-run preview" } }
            </button>
            { render_local_group_admin_result("Preview", state.preview.as_ref()) }
            <label class="dos-form-field">
                <span>{ "Confirmation phrase" }</span>
                <input
                    type="text"
                    value={state.confirmation_phrase.clone()}
                    placeholder={LOCAL_GROUP_ADMIN_CONFIRMATION}
                    oninput={{
                        let assign_user_state = assign_user_state.clone();
                        Callback::from(move |event: InputEvent| {
                            let input: HtmlInputElement = event.target_unchecked_into();
                            let mut next = (*assign_user_state).clone();
                            next.confirmation_phrase = input.value();
                            next.submitted = None;
                            assign_user_state.set(next);
                        })
                    }}
                    disabled={!enabled}
                />
            </label>
            <button class="dos-auth-submit" type="button" disabled={!can_apply} onclick={apply}>
                { if state.applying { "Submitting..." } else { "Submit capability mapping" } }
            </button>
            { render_local_group_admin_result("Submitted", state.submitted.as_ref()) }
            if let Some(error) = &state.error {
                <div class="dos-auth-error" role="alert">{ error.clone() }</div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_local_group_admin_result(
    label: &str,
    response: Option<&LocalGroupAdminResponse>,
) -> Html {
    match response {
        Some(response) => html! {
            <section class="dos-plan-result" data-job-state="accepted">
                <span class="dos-card-label">{ label }</span>
                <p>{ format!("Job {} · {} · dry run {}", response.accepted.job_id, response.accepted.kind, response.accepted.dry_run) }</p>
                <code>{ format!("{} · group {}{}", response.operation, response.group_name, response.username.as_ref().map(|username| format!(" · user {username}")).unwrap_or_default()) }</code>
            </section>
        },
        None => Html::default(),
    }
}

#[cfg(target_arch = "wasm32")]
fn render_users_groups_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct ActivityPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
enum ReportUploadState {
    Idle,
    Rendering {
        filename: String,
        size_label: String,
    },
    Downloaded {
        filename: String,
    },
    Failed {
        message: String,
    },
}

#[cfg(target_arch = "wasm32")]
#[function_component(ActivityPage)]
pub fn activity_page(props: &ActivityPageProps) -> Html {
    let api_path = WorkspacePage::Activity.api_path(&props.api_base_path);
    let report_upload_path =
        crate::api::activity_performance_report_upload_path(&props.api_base_path);
    let activity_state = use_state(|| ApiLoadState::<ActivityWorkspaceResponse>::Loading);
    let report_upload_state = use_state(|| ReportUploadState::Idle);

    {
        let api_path = api_path.clone();
        let activity_state = activity_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                activity_state.set(page_load_state_from_result(
                    crate::api::get_activity_workspace(&path).await,
                    |view| {
                        view.categories.is_empty().then(|| {
                            "No daemon activity categories were reported by the workspace API."
                                .to_string()
                        })
                    },
                ));
            });
            || ()
        });
    }

    let submit_report_file = {
        let report_upload_path = report_upload_path.clone();
        let report_upload_state = report_upload_state.clone();
        Callback::from(move |file: File| {
            let filename = file.name();
            let size_label = format_file_size(file.size());
            if !filename.to_ascii_lowercase().ends_with(".json") {
                report_upload_state.set(ReportUploadState::Failed {
                    message: "Select a DASObjectStore benchmarking JSON artifact.".to_string(),
                });
                return;
            }
            report_upload_state.set(ReportUploadState::Rendering {
                filename: filename.clone(),
                size_label,
            });
            let report_upload_path = report_upload_path.clone();
            let report_upload_state = report_upload_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::upload_performance_report_json(&report_upload_path, file).await {
                    Ok(download) => match download_pdf_to_host(&download.filename, &download.bytes)
                    {
                        Ok(()) => report_upload_state.set(ReportUploadState::Downloaded {
                            filename: download.filename,
                        }),
                        Err(message) => {
                            report_upload_state.set(ReportUploadState::Failed { message })
                        }
                    },
                    Err(err) => report_upload_state.set(ReportUploadState::Failed {
                        message: err.message,
                    }),
                }
            });
        })
    };

    let on_report_file_change = {
        let submit_report_file = submit_report_file.clone();
        Callback::from(move |event: Event| {
            let Some(input) = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlInputElement>().ok())
            else {
                return;
            };
            if let Some(file) = input.files().and_then(|files| files.item(0)) {
                submit_report_file.emit(file);
            }
            input.set_value("");
        })
    };

    let on_report_drag_over = Callback::from(|event: DragEvent| {
        event.prevent_default();
    });
    let on_report_drop = {
        let submit_report_file = submit_report_file.clone();
        Callback::from(move |event: DragEvent| {
            event.prevent_default();
            if let Some(file) = event
                .data_transfer()
                .and_then(|transfer| transfer.files())
                .and_then(|files| files.item(0))
            {
                submit_report_file.emit(file);
            }
        })
    };

    html! {
        <section class="dos-page" data-page="activity" data-api-route={api_path}>
            <PageHeader
                eyebrow="Daemon jobs"
                title="Activity"
                summary="Administrator work, ingest, settlement, repair, and endpoint validation from the shared daemon job model."
            />
            { render_activity_state(
                &*activity_state,
                &*report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_state(
    state: &ApiLoadState<ActivityWorkspaceResponse>,
    report_upload_state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_activity_state_message(
            "Loading",
            "Loading daemon activity",
            "The Web console is requesting the shared daemon activity workspace.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_activity_workspace(
                view,
                report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            )
        }
        ApiLoadState::Empty(message) => {
            render_activity_state_message("Inventory", "No daemon activity data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_activity_state_message(
            "Permission denied",
            "Activity requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_activity_state_message("Error", "Unable to load Activity", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_workspace(
    view: &ActivityWorkspaceResponse,
    report_upload_state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    html! {
        <>
            <div class="dos-metric-grid dos-activity-queues">
                { for activity_queue_summary(view).into_iter().map(render_metric_card) }
            </div>
            <div class="dos-activity-grid">
                { for activity_category_summaries(view).into_iter().map(render_activity_category_card) }
            </div>
            { render_activity_reporting_card(
                report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            ) }
            <section class="dos-card dos-wide-card dos-activity-tasks">
                <div class="dos-card-row">
                    <span class="dos-card-label">{ "Daemon task stream" }</span>
                    <span class="dos-status-pill">{ format!("{} task(s)", view.tasks.len()) }</span>
                </div>
                if view.tasks.is_empty() {
                    <p>{ "No active administrator, ingest, destage, repair, or endpoint validation tasks are currently reported." }</p>
                } else {
                    <div class="dos-task-list">
                        { for view.tasks.iter().map(render_activity_task) }
                    </div>
                }
            </section>
            if !view.warnings.is_empty() {
                <section class="dos-card dos-wide-card" data-state="warning">
                    <span class="dos-card-label">{ "Activity warnings" }</span>
                    <div class="dos-task-list">
                        { for view.warnings.iter().map(|warning| html! {
                            <p>{ format!("{}: {}", warning.code, warning.message) }</p>
                        }) }
                    </div>
                </section>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_reporting_card(
    state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    let disabled = matches!(state, ReportUploadState::Rendering { .. });
    html! {
        <section class="dos-card dos-wide-card dos-reporting-card" data-panel="reporting">
            <div class="dos-card-row">
                <span class="dos-card-label">{ "Reporting" }</span>
                <span class="dos-status-pill">{ report_upload_state_label(state) }</span>
            </div>
            <h2>{ "Rebuild performance report" }</h2>
            <p>{ "Drop a DASObjectStore benchmarking JSON artifact to regenerate the formal Mnemosyne PDF report. The PDF downloads automatically when rendering completes." }</p>
            <label
                class={classes!("dos-report-dropzone", disabled.then_some("disabled"))}
                ondragover={on_report_drag_over}
                ondrop={on_report_drop}
            >
                <strong>{ "Drop benchmarking JSON here" }</strong>
                <span>{ "or choose a .json artifact generated by dasobjectstore performance-test" }</span>
                <input
                    type="file"
                    accept=".json,application/json"
                    disabled={disabled}
                    onchange={on_report_file_change}
                />
            </label>
            { render_report_upload_progress(state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn report_upload_state_label(state: &ReportUploadState) -> &'static str {
    match state {
        ReportUploadState::Idle => "ready",
        ReportUploadState::Rendering { .. } => "rendering",
        ReportUploadState::Downloaded { .. } => "downloaded",
        ReportUploadState::Failed { .. } => "review",
    }
}

#[cfg(target_arch = "wasm32")]
fn render_report_upload_progress(state: &ReportUploadState) -> Html {
    match state {
        ReportUploadState::Idle => html! {
            <div class="dos-report-progress" data-state="idle">
                <span>{ "Accepted input: DASObjectStore performance-test JSON." }</span>
            </div>
        },
        ReportUploadState::Rendering {
            filename,
            size_label,
        } => html! {
            <div class="dos-report-progress" data-state="rendering">
                <div class="dos-report-progress-meta">
                    <span>{ filename.clone() }</span>
                    <span>{ size_label.clone() }</span>
                </div>
                <div class="dos-report-progress-bar">
                    <span class="dos-report-progress-fill"></span>
                </div>
                <span>{ "Uploading JSON and rendering the formal PDF report." }</span>
            </div>
        },
        ReportUploadState::Downloaded { filename } => html! {
            <div class="dos-report-progress" data-state="downloaded">
                <strong>{ "PDF report prepared" }</strong>
                <span>{ format!("{filename} has been sent to the browser download manager.") }</span>
            </div>
        },
        ReportUploadState::Failed { message } => html! {
            <div class="dos-report-progress" data-state="error">
                <strong>{ "Report rebuild failed" }</strong>
                <span>{ message.clone() }</span>
            </div>
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn download_pdf_to_host(filename: &str, bytes: &[u8]) -> Result<(), String> {
    download_bytes_to_host(filename, bytes, "application/pdf")
}

#[cfg(target_arch = "wasm32")]
fn download_bytes_to_host(filename: &str, bytes: &[u8], content_type: &str) -> Result<(), String> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let options = BlobPropertyBag::new();
    options.set_type(content_type);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| "could not prepare browser download blob".to_string())?;
    let url = Url::create_object_url_with_blob(&blob)
        .map_err(|_| "could not create browser download URL".to_string())?;
    let result = (|| {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "browser document is unavailable".to_string())?;
        let anchor = document
            .create_element("a")
            .map_err(|_| "could not create browser download link".to_string())?
            .dyn_into::<HtmlAnchorElement>()
            .map_err(|_| "browser download link is not an anchor".to_string())?;
        anchor.set_href(&url);
        anchor.set_download(filename);
        let body = document
            .body()
            .ok_or_else(|| "browser document body is unavailable".to_string())?;
        body.append_child(&anchor)
            .map_err(|_| "could not attach browser download link".to_string())?;
        anchor.click();
        anchor.remove();
        Ok(())
    })();
    let _ = Url::revoke_object_url(&url);
    result
}

#[cfg(target_arch = "wasm32")]
fn format_file_size(bytes: f64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else {
        format!("{:.0} B", bytes)
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_category_card(summary: ActivityCategorySummary) -> Html {
    html! {
        <section class="dos-card dos-activity-card" data-kind={summary.kind.clone()} data-state={summary.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ summary.label.clone() }</span>
                <span class="dos-status-pill">{ summary.state.clone() }</span>
            </div>
            <strong>{ format!("{} active", summary.active_count) }</strong>
            <p>{ summary.description }</p>
            <div class="dos-drive-meta">
                <span>{ format!("{} waiting", summary.waiting_count) }</span>
                <span>{ format!("{} failed", summary.failed_count) }</span>
                <span>{ format!("{} complete", summary.complete_count) }</span>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_task(task: &crate::api::ActivityTaskResponse) -> Html {
    html! {
        <article class="dos-task-card" data-state={task.state.clone()} data-kind={task.kind.clone()}>
            <div>
                <span class="dos-card-label">{ activity_task_kind_label(&task.kind) }</span>
                <strong>{ task.label.clone() }</strong>
                <p>{ format!("{} · updated {}", task.task_id, task.updated_at_utc) }</p>
            </div>
            <span class="dos-status-pill">{ activity_task_state_label(&task.state) }</span>
        </article>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_activity_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsReadinessSummary {
    pub object_type: String,
    pub label: String,
    pub category: String,
    pub state: String,
    pub state_label: String,
    pub primary_workflow: String,
    pub handoff: String,
    pub metadata: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsContextSummary {
    pub section: String,
    pub label: String,
    pub state: String,
    pub state_label: String,
    pub summary: String,
    pub detail: String,
    pub evidence: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsDerivationSourceSummary {
    pub source_kind: String,
    pub source_id: String,
    pub display_name: String,
    pub object_type: String,
    pub parent: String,
    pub endpoint_export: String,
    pub binding: String,
    pub workflow_roles: String,
    pub evidence: String,
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_readiness_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsReadinessSummary> {
    if !view.readiness_cards.is_empty() {
        return view
            .readiness_cards
            .iter()
            .map(|card| BioinformaticsReadinessSummary {
                object_type: card.object_type.clone(),
                label: card.label.clone(),
                category: card.category.clone(),
                state: card.state.clone(),
                state_label: bioinformatics_readiness_state_label(&card.state).to_string(),
                primary_workflow: card.primary_workflow.clone(),
                handoff: card.handoff.clone(),
                metadata: bioinformatics_metadata_summary(&card.required_metadata),
            })
            .collect();
    }

    view.supported_object_types
        .iter()
        .map(|object_type| BioinformaticsReadinessSummary {
            object_type: object_type.to_ascii_lowercase().replace(['/', '.'], "_"),
            label: object_type.clone(),
            category: "Supported type".to_string(),
            state: "reserved".to_string(),
            state_label: "Reserved".to_string(),
            primary_workflow: "Workflow handoff metadata has not yet been published by the API."
                .to_string(),
            handoff: "Pending workflow contract".to_string(),
            metadata: "metadata contract pending".to_string(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_context_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsContextSummary> {
    let mut summaries = Vec::new();
    summaries.extend(bioinformatics_context_group(
        "Sequencing Runs",
        &view.sequencing_runs,
    ));
    summaries.extend(bioinformatics_context_group(
        "Object Lineage",
        &view.object_lineage,
    ));
    summaries.extend(bioinformatics_context_group(
        "Workflow Handoff",
        &view.workflow_handoffs,
    ));
    summaries.extend(bioinformatics_context_group(
        "Governance",
        &view.governance_bindings,
    ));
    summaries
}

#[cfg(any(target_arch = "wasm32", test))]
fn bioinformatics_context_group(
    section: &str,
    cards: &[crate::api::BioinformaticsContextCardResponse],
) -> Vec<BioinformaticsContextSummary> {
    cards
        .iter()
        .map(|card| BioinformaticsContextSummary {
            section: section.to_string(),
            label: card.label.clone(),
            state: card.state.clone(),
            state_label: bioinformatics_readiness_state_label(&card.state).to_string(),
            summary: card.summary.clone(),
            detail: card.detail.clone(),
            evidence: bioinformatics_metadata_summary(&card.evidence),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_derivation_source_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsDerivationSourceSummary> {
    view.derivation_sources
        .iter()
        .map(|source| BioinformaticsDerivationSourceSummary {
            source_kind: source.source_kind.clone(),
            source_id: source.source_id.clone(),
            display_name: source.display_name.clone(),
            object_type: source.object_type.clone(),
            parent: source
                .parent_id
                .clone()
                .unwrap_or_else(|| "top-level source".to_string()),
            endpoint_export: source
                .endpoint_export_mode
                .clone()
                .unwrap_or_else(|| "not exported".to_string()),
            binding: match &source.governance_domain {
                Some(domain) => format!("{} · {}", source.mneion_binding_state, domain),
                None => source.mneion_binding_state.clone(),
            },
            workflow_roles: bioinformatics_metadata_summary(&source.workflow_roles),
            evidence: bioinformatics_metadata_summary(&source.evidence),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_summary_cards(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<(String, String, String)> {
    let cards = bioinformatics_readiness_summaries(view);
    let context_cards = bioinformatics_context_summaries(view);
    let derivation_sources = bioinformatics_derivation_source_summaries(view);
    let workflow_ready = cards
        .iter()
        .filter(|card| card.state == "workflow_ready")
        .count();
    let metadata_needed = cards
        .iter()
        .filter(|card| card.state.contains("metadata"))
        .count();

    vec![
        (
            "Object families".to_string(),
            cards.len().to_string(),
            "Supported bioinformatics object classifications".to_string(),
        ),
        (
            "Workflow ready".to_string(),
            workflow_ready.to_string(),
            "Cards with sufficient default handoff semantics".to_string(),
        ),
        (
            "Metadata needed".to_string(),
            metadata_needed.to_string(),
            "Cards that require explicit reference or provenance binding".to_string(),
        ),
        (
            "Context views".to_string(),
            context_cards.len().to_string(),
            "Provenance, lineage, handoff, and governance cards".to_string(),
        ),
        (
            "Derivation sources".to_string(),
            derivation_sources.len().to_string(),
            "ObjectStore, SubObject, object-type, and Mneion source records".to_string(),
        ),
    ]
}

#[cfg(any(target_arch = "wasm32", test))]
fn bioinformatics_readiness_state_label(state: &str) -> &'static str {
    match state {
        "workflow_ready" => "Workflow ready",
        "metadata_required" => "Metadata needed",
        "catalogue_ready" => "Catalogue ready",
        "planned" => "Planned",
        "binding_required" => "Binding needed",
        "reserved" => "Reserved",
        _ => "Review",
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn bioinformatics_metadata_summary(required_metadata: &[String]) -> String {
    if required_metadata.is_empty() {
        "metadata contract pending".to_string()
    } else {
        required_metadata.join("; ")
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct BioinformaticsPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(BioinformaticsPage)]
pub fn bioinformatics_page(props: &BioinformaticsPageProps) -> Html {
    let api_path = WorkspacePage::Bioinformatics.api_path(&props.api_base_path);
    let bioinformatics_state =
        use_state(|| ApiLoadState::<BioinformaticsWorkspaceResponse>::Loading);

    {
        let api_path = api_path.clone();
        let bioinformatics_state = bioinformatics_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                bioinformatics_state.set(page_load_state_from_result(
                    crate::api::get_bioinformatics_workspace(&path).await,
                    |view| {
                        (view.supported_object_types.is_empty()
                            && view.readiness_cards.is_empty())
                        .then(|| {
                            "No bioinformatics object types or readiness cards were reported by the daemon workspace API."
                                .to_string()
                        })
                    },
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="bioinformatics" data-api-route={api_path}>
            <PageHeader
                eyebrow="Workflow integration"
                title="Bioinformatics"
                summary="Sequencing data readiness, workflow handoff, and Mnemosyne integration state."
            />
            { render_bioinformatics_state(&*bioinformatics_state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_state(state: &ApiLoadState<BioinformaticsWorkspaceResponse>) -> Html {
    match state {
        ApiLoadState::Loading => render_bioinformatics_state_message(
            "Loading",
            "Loading bioinformatics readiness",
            "The Web console is requesting daemon-backed object type and workflow readiness state.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_bioinformatics_workspace(view)
        }
        ApiLoadState::Empty(message) => render_bioinformatics_state_message(
            "Inventory",
            "No bioinformatics readiness data",
            message,
        ),
        ApiLoadState::PermissionDenied(message) => render_bioinformatics_state_message(
            "Permission denied",
            "Bioinformatics readiness requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => render_bioinformatics_state_message(
            "Error",
            "Unable to load bioinformatics readiness",
            message,
        ),
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_workspace(view: &BioinformaticsWorkspaceResponse) -> Html {
    let summaries = bioinformatics_readiness_summaries(view);
    let derivation_sources = bioinformatics_derivation_source_summaries(view);
    let context_summaries = bioinformatics_context_summaries(view);
    html! {
        <>
            <div class="dos-metric-grid">
                { for bioinformatics_summary_cards(view).into_iter().map(render_bioinformatics_metric_card) }
            </div>
            <section class="dos-card dos-wide-card" data-state={if view.available { "available" } else { "reserved" }}>
                <span class="dos-card-label">{ if view.available { "Workflow readiness" } else { "Reserved workflow" } }</span>
                <h2>{ if view.available { "Bioinformatics object-type readiness is available." } else { "Bioinformatics workspace is reserved." } }</h2>
                <p>{ &view.message }</p>
                <div class="dos-chip-row">
                    { for view.supported_object_types.iter().map(|object_type| html! {
                        <span class="dos-status-pill">{ object_type }</span>
                    }) }
                </div>
            </section>
            <div class="dos-store-grid">
                { for summaries.into_iter().map(render_bioinformatics_readiness_card) }
            </div>
            if !derivation_sources.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Metadata derivation" }</span>
                    <h2>{ "API-owned readiness source records" }</h2>
                    <p>{ "The Bioinformatics page renders source records supplied by the API instead of hard-coding ObjectStore, SubObject, or Mneion metadata paths in browser code." }</p>
                </section>
                <div class="dos-store-grid">
                    { for derivation_sources.into_iter().map(render_bioinformatics_derivation_source_card) }
                </div>
            }
            if !context_summaries.is_empty() {
                <section class="dos-card dos-wide-card">
                    <span class="dos-card-label">{ "Workflow context" }</span>
                    <h2>{ "Provenance, lineage, handoff, and governance state" }</h2>
                    <p>{ "These read-only cards describe the orchestration context that must be resolved before daemon-owned workflow dispatch." }</p>
                </section>
                <div class="dos-store-grid">
                    { for context_summaries.into_iter().map(render_bioinformatics_context_card) }
                </div>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_metric_card(metric: (String, String, String)) -> Html {
    html! {
        <section class="dos-card dos-metric-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.0 }</span>
                <span class="dos-status-pill">{ "Readiness" }</span>
            </div>
            <strong>{ metric.1 }</strong>
            <p>{ metric.2 }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_readiness_card(card: BioinformaticsReadinessSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-object-type={card.object_type.clone()} data-state={card.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ card.category }</span>
                <span class="dos-status-pill">{ card.state_label }</span>
            </div>
            <strong>{ card.label }</strong>
            <p>{ card.primary_workflow }</p>
            <p>{ format!("Handoff: {}", card.handoff) }</p>
            <p>{ format!("Metadata: {}", card.metadata) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_derivation_source_card(
    source: BioinformaticsDerivationSourceSummary,
) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-source-kind={source.source_kind.clone()} data-object-type={source.object_type.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ source.source_kind }</span>
                <span class="dos-status-pill">{ source.object_type }</span>
            </div>
            <strong>{ source.display_name }</strong>
            <p>{ format!("Source: {} · parent {}", source.source_id, source.parent) }</p>
            <p>{ format!("Export: {} · binding {}", source.endpoint_export, source.binding) }</p>
            <p>{ format!("Roles: {}", source.workflow_roles) }</p>
            <p>{ format!("Evidence: {}", source.evidence) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_context_card(card: BioinformaticsContextSummary) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-state={card.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ card.section }</span>
                <span class="dos-status-pill">{ card.state_label }</span>
            </div>
            <strong>{ card.label }</strong>
            <p>{ card.summary }</p>
            <p>{ card.detail }</p>
            <p>{ format!("Evidence: {}", card.evidence) }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_bioinformatics_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
struct PageHeaderProps {
    eyebrow: &'static str,
    title: &'static str,
    summary: &'static str,
}

#[cfg(target_arch = "wasm32")]
#[function_component(PageHeader)]
fn page_header(props: &PageHeaderProps) -> Html {
    html! {
        <header class="dos-page-header">
            <p>{ props.eyebrow }</p>
            <h1>{ props.title }</h1>
            <span>{ props.summary }</span>
        </header>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        activity_category_summaries, activity_queue_summary, activity_workspace_api_path,
        admin_job_percent, admin_job_progress_text, admin_job_state_is_terminal,
        bioinformatics_derivation_source_summaries, bioinformatics_readiness_summaries,
        bioinformatics_summary_cards, bioinformatics_workspace_api_path, enclosure_card_summaries,
        enclosure_prepare_candidate, enclosure_prepare_confirmed, enclosure_retry_clears_job_state,
        enclosure_ssd_root, enclosures_workspace_api_path, endpoints_workspace_api_path,
        home_dashboard_attention, home_dashboard_metrics, home_workspace_api_path,
        object_browser_download_disabled_reason, object_browser_file_download_available,
        object_browser_file_summaries, object_browser_folder_download_available,
        object_browser_folder_summaries, object_browser_initial_endpoint,
        object_browser_placement_summary, object_browser_placement_summary_state,
        object_store_bucket_default, object_store_card_summaries,
        object_store_configure_review_from_values, object_store_create_confirmation_matches,
        object_store_create_review_from_values, object_store_creation_fields_ready,
        objectstores_workspace_api_path, primary_navigation_for_host,
        subobject_registry_preview_from_values, users_groups_summary_cards,
        users_groups_workspace_api_path, ApiLoadState, EnclosureWizardState, WorkspacePage,
        ACTIVITY_WORKSPACE_ROUTE, BIOINFORMATICS_WORKSPACE_ROUTE, ENCLOSURES_WORKSPACE_ROUTE,
        ENDPOINTS_WORKSPACE_ROUTE, HOME_WORKSPACE_ROUTE, LOCAL_GROUP_ADMIN_CONFIRMATION,
        OBJECTSTORES_WORKSPACE_ROUTE, PRIMARY_NAVIGATION,
    };
    use super::{
        local_group_admin_confirmation_matches, local_group_assignment_fields_ready,
        local_group_create_fields_ready, local_group_display_name,
        users_groups_view_with_group_assignment, users_groups_view_with_writer_group,
    };
    use crate::api::{
        ActivityCategoryResponse, ActivityTaskResponse, ActivityWorkspaceResponse,
        AdminJobCancelResponse, AdminJobProgress, AdminJobStatusResponse, AdminJobSummary,
        BioinformaticsContextCardResponse, BioinformaticsDerivationSourceResponse,
        BioinformaticsReadinessCardResponse, BioinformaticsWorkspaceResponse,
        CapacitySummaryResponse, DasEnclosureCardResponse, DestageQueueSummaryResponse,
        DriveCountSummaryResponse, EnclosureConnectionResponse, EnclosurePrepareAcceptedResponse,
        EnclosurePrepareHddDevice, EnclosurePrepareResponse, EnclosuresPageResponse,
        HomeDashboardResponse, IngestQueueSummaryResponse, LocalGroupMembershipResponse,
        LocalGroupOperationResponse, LocalUserAuthorityResponse, ObjectBrowserFileNodeResponse,
        ObjectBrowserFolderNodeResponse, ObjectBrowserPlacementResponse, ObjectStoresPageResponse,
        StandaloneUserAccountResponse, StorageGroupResponse, UsersGroupsCapabilitiesResponse,
        UsersGroupsWorkspaceResponse,
    };
    use crate::mount::FrontendHost;
    use crate::stores::STORES_WORKSPACE_ROUTE;
    use crate::users_groups::USERS_GROUPS_WORKSPACE_ROUTE;

    #[test]
    fn primary_navigation_uses_redesign_labels() {
        let labels: Vec<_> = PRIMARY_NAVIGATION.iter().map(|page| page.label()).collect();

        assert_eq!(
            labels,
            vec![
                "Home",
                "Enclosures",
                "ObjectStores",
                "Endpoints",
                "Activity",
                "Capabilities",
                "Bioinformatics"
            ]
        );
    }

    #[test]
    fn primary_navigation_is_host_mode_aware_for_users_groups() {
        let standalone_labels: Vec<_> = primary_navigation_for_host(FrontendHost::Standalone)
            .iter()
            .map(|page| page.label())
            .collect();
        let synoptikon_labels: Vec<_> = primary_navigation_for_host(FrontendHost::Synoptikon)
            .iter()
            .map(|page| page.label())
            .collect();

        assert!(standalone_labels.contains(&"Activity"));
        assert!(synoptikon_labels.contains(&"Activity"));
        assert!(standalone_labels.contains(&"Endpoints"));
        assert!(!synoptikon_labels.contains(&"Endpoints"));
        assert!(standalone_labels.contains(&"Capabilities"));
        assert!(!synoptikon_labels.contains(&"Capabilities"));
    }

    #[test]
    fn workspace_pages_build_expected_api_paths() {
        let base = "/products/dasobjectstore/api/v1/";

        assert_eq!(
            WorkspacePage::Home.api_path(base),
            "/products/dasobjectstore/api/v1/dashboard/home"
        );
        assert_eq!(
            WorkspacePage::Enclosures.api_path(base),
            "/products/dasobjectstore/api/v1/dashboard/enclosures"
        );
        assert_eq!(
            WorkspacePage::ObjectStores.api_path(base),
            "/products/dasobjectstore/api/v1/dashboard/object-stores"
        );
        assert_eq!(
            WorkspacePage::Activity.api_path(base),
            "/products/dasobjectstore/api/v1/workspaces/activity"
        );
        assert_eq!(
            WorkspacePage::Endpoints.api_path(base),
            "/products/dasobjectstore/api/v1/workspaces/endpoints"
        );
        assert_eq!(
            WorkspacePage::UsersGroups.api_path(base),
            "/products/dasobjectstore/api/v1/workspaces/users-groups"
        );
        assert_eq!(
            WorkspacePage::Bioinformatics.api_path(base),
            "/products/dasobjectstore/api/v1/workspaces/bioinformatics"
        );
    }

    #[test]
    fn frontend_page_routes_use_dashboard_contracts() {
        assert_eq!(HOME_WORKSPACE_ROUTE, "dashboard/home");
        assert_eq!(ENCLOSURES_WORKSPACE_ROUTE, "dashboard/enclosures");
        assert_eq!(OBJECTSTORES_WORKSPACE_ROUTE, "dashboard/object-stores");
        assert_eq!(ACTIVITY_WORKSPACE_ROUTE, "workspaces/activity");
        assert_eq!(ENDPOINTS_WORKSPACE_ROUTE, "workspaces/endpoints");
        assert_eq!(home_workspace_api_path("/api/"), "/api/dashboard/home");
        assert_eq!(
            enclosures_workspace_api_path("/api/"),
            "/api/dashboard/enclosures"
        );
        assert_eq!(
            objectstores_workspace_api_path("/api/"),
            "/api/dashboard/object-stores"
        );
        assert_eq!(
            activity_workspace_api_path("/api/"),
            "/api/workspaces/activity"
        );
        assert_eq!(
            endpoints_workspace_api_path("/api/"),
            "/api/workspaces/endpoints"
        );
        assert_eq!(
            users_groups_workspace_api_path("/api/"),
            "/api/workspaces/users-groups"
        );
    }

    #[test]
    fn primary_navigation_promotes_users_groups_without_legacy_stores_holder() {
        let base = "/products/dasobjectstore/api/v1/";
        let primary_paths: Vec<_> = PRIMARY_NAVIGATION
            .iter()
            .map(|page| page.api_path(base))
            .collect();

        assert!(!primary_paths
            .iter()
            .any(|path| path.ends_with(STORES_WORKSPACE_ROUTE)));
        assert!(primary_paths
            .iter()
            .any(|path| path.ends_with(USERS_GROUPS_WORKSPACE_ROUTE)));
        assert!(primary_paths
            .iter()
            .any(|path| path.ends_with(ACTIVITY_WORKSPACE_ROUTE)));
        assert!(primary_paths
            .iter()
            .any(|path| path.ends_with(ENDPOINTS_WORKSPACE_ROUTE)));
        assert!(primary_paths
            .iter()
            .any(|path| path.ends_with(OBJECTSTORES_WORKSPACE_ROUTE)));
    }

    #[test]
    fn activity_category_summaries_cover_daemon_job_states() {
        let view = ActivityWorkspaceResponse {
            ingest: Some(IngestQueueSummaryResponse {
                pressure: "normal".to_string(),
                queued_jobs: 2,
                active_jobs: 1,
                failed_jobs: 0,
                warnings: Vec::new(),
            }),
            destage: Some(DestageQueueSummaryResponse {
                pending_objects: 3,
                copying_objects: 1,
                verified_objects: 8,
                warnings: Vec::new(),
            }),
            categories: vec![
                ActivityCategoryResponse {
                    kind: "system_administration".to_string(),
                    label: "Administrator jobs".to_string(),
                    description: "Privileged work".to_string(),
                },
                ActivityCategoryResponse {
                    kind: "ingest".to_string(),
                    label: "Ingest".to_string(),
                    description: "Uploads".to_string(),
                },
                ActivityCategoryResponse {
                    kind: "repair".to_string(),
                    label: "Repair".to_string(),
                    description: "Repair work".to_string(),
                },
            ],
            tasks: vec![
                ActivityTaskResponse {
                    task_id: "job-admin".to_string(),
                    kind: "system_administration".to_string(),
                    state: "running".to_string(),
                    label: "Create local writer group".to_string(),
                    updated_at_utc: "2026-07-09T00:00:00Z".to_string(),
                    warnings: Vec::new(),
                },
                ActivityTaskResponse {
                    task_id: "job-ingest".to_string(),
                    kind: "ingest".to_string(),
                    state: "queued".to_string(),
                    label: "Ingest zymo".to_string(),
                    updated_at_utc: "2026-07-09T00:01:00Z".to_string(),
                    warnings: Vec::new(),
                },
                ActivityTaskResponse {
                    task_id: "job-repair".to_string(),
                    kind: "repair".to_string(),
                    state: "failed".to_string(),
                    label: "Restore copy".to_string(),
                    updated_at_utc: "2026-07-09T00:02:00Z".to_string(),
                    warnings: Vec::new(),
                },
                ActivityTaskResponse {
                    task_id: "job-repair-cancelled".to_string(),
                    kind: "repair".to_string(),
                    state: "cancelled".to_string(),
                    label: "Cancelled replacement".to_string(),
                    updated_at_utc: "2026-07-09T00:03:00Z".to_string(),
                    warnings: Vec::new(),
                },
            ],
            warnings: Vec::new(),
        };

        let summaries = activity_category_summaries(&view);
        let queues = activity_queue_summary(&view);

        assert_eq!(summaries[0].active_count, 1);
        assert_eq!(summaries[0].state, "running");
        assert_eq!(summaries[1].waiting_count, 1);
        assert_eq!(summaries[1].state, "waiting");
        assert_eq!(summaries[2].failed_count, 1);
        assert_eq!(summaries[2].complete_count, 1);
        assert_eq!(summaries[2].state, "critical");
        assert_eq!(queues[0].value, "1 active");
        assert_eq!(queues[1].value, "1 copying");
    }

    #[test]
    fn users_groups_summary_surfaces_authority_and_writer_policy() {
        let cards = users_groups_summary_cards(&users_groups_workspace_fixture());
        let values: Vec<_> = cards
            .iter()
            .map(|card| (card.label.as_str(), card.value.as_str()))
            .collect();

        assert!(values.contains(&("Authority adapter", "standalone")));
        assert!(values.contains(&("Local actor", "operator")));
        assert!(values.contains(&("Session principals", "1")));
        assert!(values.contains(&("Capability groups", "1")));
        assert!(values.contains(&("Mapping actions", "2")));
    }

    #[test]
    fn users_groups_forms_gate_required_fields_and_confirmation() {
        assert!(local_group_create_fields_ready("mnemosyne-writers"));
        assert!(!local_group_create_fields_ready(" "));
        assert!(local_group_assignment_fields_ready(
            "stephen",
            "mnemosyne-writers"
        ));
        assert!(!local_group_assignment_fields_ready("stephen", " "));
        assert!(local_group_admin_confirmation_matches(
            LOCAL_GROUP_ADMIN_CONFIRMATION
        ));
        assert!(!local_group_admin_confirmation_matches(
            "confirm create objectstore"
        ));
    }

    #[test]
    fn users_groups_live_create_updates_writer_policy_view() {
        let view = users_groups_view_with_writer_group(
            users_groups_workspace_fixture(),
            "mnemosyne_writers",
        );

        assert_eq!(
            local_group_display_name("mnemosyne_writers"),
            "Mnemosyne Writers"
        );
        assert!(view
            .writer_groups
            .iter()
            .any(|group| group.group_name == "mnemosyne_writers"
                && group.display_name == "Mnemosyne Writers"
                && !group.current_user_member));
        assert_eq!(
            view.selected_group_name.as_deref(),
            Some("mnemosyne_writers")
        );
    }

    #[test]
    fn users_groups_live_assignment_updates_current_user_membership_view() {
        let view = users_groups_view_with_writer_group(
            users_groups_workspace_fixture(),
            "mnemosyne_writers",
        );
        let view = users_groups_view_with_group_assignment(view, "operator", "mnemosyne_writers");

        assert!(view
            .current_user
            .as_ref()
            .expect("fixture user")
            .groups
            .iter()
            .any(|group| group == "mnemosyne_writers"));
        assert!(view
            .writer_groups
            .iter()
            .any(|group| group.group_name == "mnemosyne_writers" && group.current_user_member));
        assert_eq!(view.selected_username.as_deref(), Some("operator"));
        assert_eq!(
            view.selected_group_name.as_deref(),
            Some("mnemosyne_writers")
        );
    }

    #[test]
    fn bioinformatics_route_is_stable() {
        assert_eq!(BIOINFORMATICS_WORKSPACE_ROUTE, "workspaces/bioinformatics");
        assert_eq!(
            bioinformatics_workspace_api_path("/api/"),
            "/api/workspaces/bioinformatics"
        );
    }

    #[test]
    fn bioinformatics_readiness_cards_surface_workflow_handoff() {
        let view = BioinformaticsWorkspaceResponse {
            schema_version: "dasobjectstore.product_workspaces.v1".to_string(),
            available: true,
            supported_object_types: vec![
                "BAM".to_string(),
                "CRAM".to_string(),
                "POD5".to_string(),
                "FASTQ/FASTQ.GZ".to_string(),
                "FASTA".to_string(),
                "VCF/BCF".to_string(),
                "GFF/GTF".to_string(),
                "ENA/SRA".to_string(),
            ],
            readiness_cards: vec![
                BioinformaticsReadinessCardResponse {
                    object_type: "pod5".to_string(),
                    label: "POD5".to_string(),
                    category: "Nanopore signal".to_string(),
                    state: "workflow_ready".to_string(),
                    primary_workflow: "Basecalling and signal provenance.".to_string(),
                    handoff: "Basecalling readiness".to_string(),
                    required_metadata: vec![
                        "flowcell/run identity".to_string(),
                        "sequencing kit".to_string(),
                    ],
                },
                BioinformaticsReadinessCardResponse {
                    object_type: "cram".to_string(),
                    label: "CRAM".to_string(),
                    category: "Compressed alignment".to_string(),
                    state: "metadata_required".to_string(),
                    primary_workflow: "Reference-backed analysis.".to_string(),
                    handoff: "Genome analysis with reference binding".to_string(),
                    required_metadata: vec!["reference genome".to_string()],
                },
            ],
            derivation_sources: vec![
                BioinformaticsDerivationSourceResponse {
                    source_kind: "object_store_metadata".to_string(),
                    source_id: "contract-object-store-object-type".to_string(),
                    display_name: "ObjectStore object-type assignment".to_string(),
                    object_type: "pod5".to_string(),
                    parent_id: None,
                    endpoint_export_mode: Some("s3_bucket".to_string()),
                    mneion_binding_state: "binding_required".to_string(),
                    governance_domain: None,
                    workflow_roles: vec![
                        "sequencing_run_provenance".to_string(),
                        "basecalling_handoff".to_string(),
                    ],
                    evidence: vec!["ObjectStore object_type assignment".to_string()],
                },
                BioinformaticsDerivationSourceResponse {
                    source_kind: "subobject_metadata".to_string(),
                    source_id: "contract-subobject-lineage".to_string(),
                    display_name: "SubObject lineage and object-type policy".to_string(),
                    object_type: "fastq".to_string(),
                    parent_id: Some("contract-object-store-object-type".to_string()),
                    endpoint_export_mode: Some("dedicated_prefix".to_string()),
                    mneion_binding_state: "binding_required".to_string(),
                    governance_domain: None,
                    workflow_roles: vec!["object_lineage".to_string()],
                    evidence: vec!["SubObject parent relationship".to_string()],
                },
                BioinformaticsDerivationSourceResponse {
                    source_kind: "mneion_binding".to_string(),
                    source_id: "contract-mneion-governance-binding".to_string(),
                    display_name: "Mneion governance-domain binding".to_string(),
                    object_type: "mixed".to_string(),
                    parent_id: None,
                    endpoint_export_mode: None,
                    mneion_binding_state: "binding_required".to_string(),
                    governance_domain: Some("unassigned".to_string()),
                    workflow_roles: vec!["governance_binding".to_string()],
                    evidence: vec!["Mneion storage definition".to_string()],
                },
            ],
            sequencing_runs: vec![BioinformaticsContextCardResponse {
                label: "Sequencing run provenance".to_string(),
                state: "metadata_required".to_string(),
                summary: "Run metadata required.".to_string(),
                detail: "Bind flowcell, kit, and sample state.".to_string(),
                evidence: vec!["POD5 basecalling readiness".to_string()],
            }],
            object_lineage: vec![BioinformaticsContextCardResponse {
                label: "Object lineage".to_string(),
                state: "planned".to_string(),
                summary: "Lineage planned.".to_string(),
                detail: "Connect signal, reads, alignment, and variants.".to_string(),
                evidence: vec!["raw signal to reads".to_string()],
            }],
            workflow_handoffs: vec![BioinformaticsContextCardResponse {
                label: "Basecalling handoff".to_string(),
                state: "workflow_ready".to_string(),
                summary: "Basecalling ready.".to_string(),
                detail: "POD5 handoff state is available.".to_string(),
                evidence: vec!["POD5 readiness cards".to_string()],
            }],
            governance_bindings: vec![BioinformaticsContextCardResponse {
                label: "Mnemosyne governance binding".to_string(),
                state: "binding_required".to_string(),
                summary: "Binding required.".to_string(),
                detail: "Project and governance-domain binding is required.".to_string(),
                evidence: vec!["endpoint inventory bindings".to_string()],
            }],
            message: "Readiness cards available.".to_string(),
        };

        let cards = bioinformatics_readiness_summaries(&view);
        let derivation_sources = bioinformatics_derivation_source_summaries(&view);
        let context_cards = super::bioinformatics_context_summaries(&view);
        let metrics = bioinformatics_summary_cards(&view);

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].label, "POD5");
        assert_eq!(cards[0].state_label, "Workflow ready");
        assert_eq!(cards[0].handoff, "Basecalling readiness");
        assert_eq!(cards[0].metadata, "flowcell/run identity; sequencing kit");
        assert_eq!(cards[1].state_label, "Metadata needed");
        assert_eq!(metrics[0].1, "2");
        assert_eq!(metrics[1].1, "1");
        assert_eq!(metrics[2].1, "1");
        assert_eq!(metrics[3].1, "4");
        assert_eq!(metrics[4].1, "3");
        assert_eq!(derivation_sources[0].source_kind, "object_store_metadata");
        assert_eq!(derivation_sources[0].parent, "top-level source");
        assert_eq!(
            derivation_sources[1].parent,
            "contract-object-store-object-type"
        );
        assert_eq!(
            derivation_sources[2].binding,
            "binding_required · unassigned"
        );
        assert_eq!(context_cards[0].section, "Sequencing Runs");
        assert_eq!(context_cards[1].state_label, "Planned");
        assert_eq!(context_cards[3].state_label, "Binding needed");
    }

    #[test]
    fn bioinformatics_readiness_falls_back_to_supported_types() {
        let view = BioinformaticsWorkspaceResponse {
            schema_version: "dasobjectstore.product_workspaces.v1".to_string(),
            available: false,
            supported_object_types: vec!["FASTQ/FASTQ.GZ".to_string(), "ENA/SRA".to_string()],
            readiness_cards: Vec::new(),
            derivation_sources: Vec::new(),
            sequencing_runs: Vec::new(),
            object_lineage: Vec::new(),
            workflow_handoffs: Vec::new(),
            governance_bindings: Vec::new(),
            message: "Older payload.".to_string(),
        };

        let cards = bioinformatics_readiness_summaries(&view);

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].label, "FASTQ/FASTQ.GZ");
        assert_eq!(cards[0].state_label, "Reserved");
        assert_eq!(cards[0].handoff, "Pending workflow contract");
    }

    #[test]
    fn admin_job_terminal_states_are_stable_for_wizard_actions() {
        assert!(admin_job_state_is_terminal("complete"));
        assert!(admin_job_state_is_terminal("failed"));
        assert!(admin_job_state_is_terminal("cancelled"));
        assert!(!admin_job_state_is_terminal("queued"));
        assert!(!admin_job_state_is_terminal("running"));
        assert!(!admin_job_state_is_terminal("waiting"));
    }

    #[test]
    fn admin_job_percent_prefers_daemon_percent_then_unit_progress() {
        let with_percent = AdminJobSummary {
            percent_complete: Some(42),
            ..admin_job_summary_fixture()
        };
        assert_eq!(admin_job_percent(&with_percent), Some(42));

        let by_units = AdminJobSummary {
            percent_complete: None,
            progress: AdminJobProgress {
                stage: "formatting".to_string(),
                work_units_done: 3,
                work_units_total: 4,
                ..AdminJobProgress::default()
            },
            ..admin_job_summary_fixture()
        };
        assert_eq!(admin_job_percent(&by_units), Some(75));
        assert_eq!(admin_job_progress_text(&by_units), "3 / 4 step(s)");
    }

    #[test]
    fn admin_job_progress_text_prefers_byte_progress_when_available() {
        let job = AdminJobSummary {
            progress: AdminJobProgress {
                stage: "copying".to_string(),
                work_bytes_done: 512,
                work_bytes_total: 1024,
                work_units_done: 1,
                work_units_total: 4,
                message: None,
            },
            ..admin_job_summary_fixture()
        };

        assert_eq!(admin_job_progress_text(&job), "512 / 1024 byte(s)");
    }

    #[test]
    fn enclosure_prepare_confirmation_requires_existing_data_acknowledgement() {
        assert!(enclosure_prepare_confirmed(
            true,
            true,
            " confirm prepare das "
        ));
        assert!(!enclosure_prepare_confirmed(
            true,
            false,
            "confirm prepare das"
        ));
        assert!(!enclosure_prepare_confirmed(
            false,
            true,
            "confirm prepare das"
        ));
    }

    #[test]
    fn enclosure_retry_preserves_selection_but_clears_job_and_cancel_state() {
        let mut state = EnclosureWizardState {
            open: true,
            selected_ssd: "/dev/disk/by-id/nvme-ssd".to_string(),
            selected_hdds: vec!["/dev/disk/by-id/usb-qnap-1057".to_string()],
            mount_root: "/srv/dasobjectstore".to_string(),
            filesystem: "ext4".to_string(),
            owner: "stephen".to_string(),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_phrase: "confirm prepare das".to_string(),
            submitting: false,
            job: Some(enclosure_prepare_response_fixture()),
            job_status: Some(AdminJobStatusResponse {
                job: AdminJobSummary {
                    state: "failed".to_string(),
                    failure_message: Some("existing data preflight failed".to_string()),
                    ..admin_job_summary_fixture()
                },
            }),
            job_polling: true,
            job_status_error: Some("stale failure".to_string()),
            cancelling: true,
            cancellation: Some(AdminJobCancelResponse {
                job_id: "enclosure-prepare-1".to_string(),
                accepted: true,
                state: "cancelled".to_string(),
            }),
            cancel_error: Some("cancel failed".to_string()),
            error: Some("daemon failed".to_string()),
        };

        enclosure_retry_clears_job_state(&mut state);

        assert!(state.open);
        assert_eq!(state.selected_ssd, "/dev/disk/by-id/nvme-ssd");
        assert_eq!(state.selected_hdds.len(), 1);
        assert!(state.allow_format);
        assert!(state.existing_data_acknowledged);
        assert_eq!(state.confirmation_phrase, "confirm prepare das");
        assert!(state.job.is_none());
        assert!(state.job_status.is_none());
        assert!(!state.job_polling);
        assert!(state.cancellation.is_none());
        assert!(state.cancel_error.is_none());
        assert!(state.error.is_none());
    }

    #[test]
    fn object_store_bucket_default_normalizes_store_name_for_s3() {
        assert_eq!(
            object_store_bucket_default("Zymo Fecal 2025.05/raw"),
            "zymo-fecal-2025-05-raw"
        );
        assert_eq!(
            object_store_bucket_default("...Generated_Data..."),
            "generated-data"
        );
    }

    #[test]
    fn enclosure_ssd_root_derives_from_hdd_mount() {
        let enclosure = DasEnclosureCardResponse {
            enclosure_id: "qnap-tl-d800c-managed".to_string(),
            display_name: "QNAP TL-D800C".to_string(),
            mount_path: "/srv/dasobjectstore/hdd".to_string(),
            connection: EnclosureConnectionResponse {
                bus: "usb".to_string(),
                protocol: "uas/filesystem".to_string(),
                link_speed: "host reported".to_string(),
            },
            health: "healthy".to_string(),
            drive_count: DriveCountSummaryResponse {
                total: 8,
                mounted: 8,
                healthy: 8,
                watch: 0,
                suspect: 0,
                failed: 0,
            },
            capacity: CapacitySummaryResponse {
                total_tib: "100.0".to_string(),
                used_tib: "12.5".to_string(),
                free_tib: "87.5".to_string(),
                used_percent_basis_points: 1250,
            },
            last_seen_at_utc: "2026-07-08T08:30:00Z".to_string(),
            warnings: Vec::new(),
        };

        assert_eq!(enclosure_ssd_root(&enclosure), "/srv/dasobjectstore/ssd");
    }

    #[test]
    fn object_store_creation_requires_identity_group_and_enclosure() {
        assert!(object_store_creation_fields_ready(
            "generated-data",
            "mnemosyne",
            "qnap-tl-d800c-managed"
        ));
        assert!(!object_store_creation_fields_ready(
            "",
            "mnemosyne",
            "qnap-tl-d800c-managed"
        ));
        assert!(!object_store_creation_fields_ready(
            "generated-data",
            "",
            "qnap-tl-d800c-managed"
        ));
        assert!(!object_store_creation_fields_ready(
            "generated-data",
            "mnemosyne",
            ""
        ));
    }

    #[test]
    fn object_store_create_review_captures_policy_controls() {
        let review = object_store_create_review_from_values(
            "generated-data",
            "pod5",
            2,
            "bioinformatics",
            "s3_bucket",
            false,
            "qnap-tl-d800c-managed",
        );

        assert_eq!(
            review,
            "generated-data · type pod5 · 2 copy/copies · writer group bioinformatics · enclosure qnap-tl-d800c-managed · export s3_bucket · private · writeable until locked"
        );
    }

    #[test]
    fn object_store_configure_review_captures_policy_controls() {
        let review = object_store_configure_review_from_values(
            "generated-data",
            3,
            "bioinformatics",
            "backpressure_by_priority",
            "tombstone_then_gc",
            "s3",
            true,
            false,
        );

        assert_eq!(
            review,
            "generated-data · 3 copy/copies · writer group bioinformatics · capacity backpressure_by_priority · retention tombstone_then_gc · export s3 · public · read-only"
        );
    }

    #[test]
    fn subobject_registry_preview_captures_parent_type_and_routing() {
        let review = subobject_registry_preview_from_values(
            "pod5-raw",
            "store",
            "generated-data",
            "",
            "override",
            "pod5",
            "dedicated_prefix",
        );

        assert_eq!(
            review,
            "pod5-raw under generated-data · prefix generated-data/pod5-raw · object type pod5 · S3 routing dedicated_prefix"
        );
    }

    #[test]
    fn object_store_create_confirmation_requires_exact_phrase() {
        assert!(object_store_create_confirmation_matches(
            "confirm create objectstore"
        ));
        assert!(object_store_create_confirmation_matches(
            " confirm create objectstore "
        ));
        assert!(!object_store_create_confirmation_matches(
            "confirm create object store"
        ));
        assert!(!object_store_create_confirmation_matches(
            "CONFIRM CREATE OBJECTSTORE"
        ));
    }

    #[test]
    fn shared_api_load_state_names_cover_page_contract() {
        let success = ApiLoadState::success("payload");
        let empty = ApiLoadState::<&str>::empty("empty");
        let permission_denied = ApiLoadState::<&str>::permission_denied("denied");
        let transport_error = ApiLoadState::<&str>::transport_error("offline");
        let stale = ApiLoadState::stale_data("payload", "stale");
        let states = [
            ApiLoadState::<&str>::Loading.state_name(),
            success.state_name(),
            empty.state_name(),
            permission_denied.state_name(),
            transport_error.state_name(),
            stale.state_name(),
        ];

        assert_eq!(
            states,
            [
                "loading",
                "success",
                "empty",
                "permission-denied",
                "transport-error",
                "stale-data",
            ]
        );
    }

    #[test]
    fn authenticated_pages_do_not_expose_fixture_fallback_helpers() {
        let source = include_str!("workspace.rs");

        assert!(!source.contains(&format!("{}{}", "fallback_", "dashboard_metrics")));
        assert!(!source.contains(&format!("{}{}", "fallback_", "enclosures")));
        assert!(!source.contains(&format!("{}{}", "fallback_", "object_stores")));
    }

    fn admin_job_summary_fixture() -> AdminJobSummary {
        AdminJobSummary {
            job_id: "enclosure-prepare-1".to_string(),
            kind: "enclosure_preparation".to_string(),
            state: "running".to_string(),
            progress: AdminJobProgress::default(),
            percent_complete: None,
            submitted_at_utc: "2026-07-08T20:00:00Z".to_string(),
            updated_at_utc: "2026-07-08T20:00:01Z".to_string(),
            actor: Some("stephen".to_string()),
            failure_message: None,
        }
    }

    fn enclosure_prepare_response_fixture() -> EnclosurePrepareResponse {
        EnclosurePrepareResponse {
            accepted: EnclosurePrepareAcceptedResponse {
                job_id: "enclosure-prepare-1".to_string(),
                kind: "enclosure_preparation".to_string(),
                accepted_at_utc: "2026-07-08T20:00:00Z".to_string(),
                dry_run: false,
            },
            ssd_device: "/dev/disk/by-id/nvme-ssd".to_string(),
            hdd_devices: vec![EnclosurePrepareHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".to_string(),
            }],
            mount_root: "/srv/dasobjectstore".to_string(),
            filesystem: "ext4".to_string(),
            owner: Some("stephen".to_string()),
            administrator_actor: Some("stephen".to_string()),
            client_request_id: Some("prepare-1".to_string()),
        }
    }

    #[test]
    fn object_stores_live_payload_maps_to_card_summaries() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "store_class": "generated_data",
                "object_type": "pod5",
                "health": "healthy",
                "required_copies": 2,
                "object_count": 42,
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "12.5",
                    "free_tib": "87.5",
                    "used_percent_basis_points": 1250
                },
                "placement_policy": "fractional_free_space",
                "endpoint_export_mode": "s3_bucket",
                "writer_group": "bioinformatics",
                "public": false,
                "writeable": true,
                "created_at_utc": "2026-07-08T08:00:00Z",
                "last_ingested_at_utc": "2026-07-08T08:30:00Z",
                "warnings": [{
                    "code": "store_watch",
                    "message": "Store warning."
                }]
            }],
            "selected_store_id": "zymo_fecal_2025.05",
            "create_object_store": {
                "enabled": false,
                "action_kind": "store_create",
                "label": "Create ObjectStore",
                "required_fields": [],
                "optional_fields": [],
                "defaults": {
                    "store_class": "generated_data",
                    "required_copies": 2,
                    "endpoint_export_mode": "s3_bucket"
                },
                "store_class_options": [],
                "copy_count_options": [1, 2, 3],
                "confirmation_required": true,
                "blocked_reason": "admin required"
            },
            "warnings": []
        });
        let view = serde_json::from_value::<ObjectStoresPageResponse>(payload)
            .expect("object stores payload decodes");

        let summaries = object_store_card_summaries(&view);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "zymo_fecal_2025.05");
        assert_eq!(summaries[0].label, "generated_data");
        assert_eq!(summaries[0].object_type, "pod5");
        assert_eq!(summaries[0].access, "private / writeable");
        assert!(summaries[0].policy.contains("2 required copy/copies"));
        assert!(summaries[0].capacity.contains("12.5 TiB used"));
        assert_eq!(summaries[0].writer_group, "bioinformatics");
        assert_eq!(summaries[0].endpoint, "s3_bucket");
        assert_eq!(summaries[0].warning_count, 1);
    }

    #[test]
    fn object_browser_payload_maps_to_dense_view_summaries() {
        let view = serde_json::from_value::<ObjectStoresPageResponse>(serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-09T10:00:00Z",
            "stores": [{
                "store_id": "ENA",
                "display_name": "ENA",
                "store_class": "reproducible_cache",
                "object_type": "ena_sra",
                "health": "healthy",
                "required_copies": 1,
                "object_count": 2,
                "capacity": null,
                "placement_policy": "fractional_free_space",
                "endpoint_export_mode": "s3_bucket",
                "writer_group": "mnemosyne",
                "public": true,
                "writeable": true,
                "created_at_utc": null,
                "last_ingested_at_utc": null,
                "warnings": []
            }],
            "selected_store_id": "ENA",
            "create_object_store": {
                "enabled": false,
                "action_kind": "store_create",
                "label": "Create ObjectStore",
                "required_fields": [],
                "optional_fields": [],
                "defaults": {
                    "store_class": "generated_data",
                    "required_copies": 1,
                    "endpoint_export_mode": "s3_bucket"
                },
                "store_class_options": [],
                "copy_count_options": [1],
                "confirmation_required": true,
                "blocked_reason": "admin required"
            },
            "warnings": []
        }))
        .expect("object store view decodes");
        let folders = vec![ObjectBrowserFolderNodeResponse {
            name: "Xenognostikon".to_string(),
            prefix: "Xenognostikon".to_string(),
            object_count: Some(2),
            total_size_bytes: Some(2 * 1024 * 1024 * 1024),
            readiness: "available".to_string(),
        }];
        let files = vec![ObjectBrowserFileNodeResponse {
            object_id: "Xenognostikon/Vervet/sample.fastq.gz".to_string(),
            name: "sample.fastq.gz".to_string(),
            path: "Xenognostikon/Vervet/sample.fastq.gz".to_string(),
            object_type: "fastq".to_string(),
            size_bytes: 1536,
            modified_at_utc: Some("2026-07-09T10:00:00Z".to_string()),
            checksum: None,
            readiness: "ssd_only".to_string(),
            lifecycle_state: "ReceivedOnSsd".to_string(),
            copy_count: 1,
            placements: vec![ObjectBrowserPlacementResponse {
                disk_id: Some("qnap-1057".to_string()),
                disk_label: Some("QNAP bay 1".to_string()),
                location: "hdd_settled".to_string(),
                state: "verified".to_string(),
                size_bytes: 1536,
                checksum: None,
                verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
            }],
        }];

        let folder_summaries = object_browser_folder_summaries(&folders);
        let file_summaries = object_browser_file_summaries(&files);

        assert_eq!(
            object_browser_initial_endpoint(&view).as_deref(),
            Some("ENA")
        );
        assert_eq!(folder_summaries[0].objects, "2 object(s)");
        assert_eq!(folder_summaries[0].size, "2.0 GiB");
        assert_eq!(folder_summaries[0].readiness, "Available");
        assert_eq!(file_summaries[0].object_type, "Fastq");
        assert_eq!(file_summaries[0].size, "1.5 KiB");
        assert_eq!(file_summaries[0].readiness, "Ssd Only");
        assert_eq!(file_summaries[0].lifecycle, "Received On Ssd");
        assert_eq!(file_summaries[0].copies, "1 copy/copies");
        assert_eq!(file_summaries[0].placement_summary, "1 HDD settled");
        assert_eq!(
            file_summaries[0].placements[0].disk_label.as_deref(),
            Some("QNAP bay 1")
        );
        assert!(object_browser_folder_download_available(
            &folder_summaries[0].readiness
        ));
        assert!(!object_browser_file_download_available(
            &file_summaries[0].readiness,
            &file_summaries[0].placements,
        ));
        assert!(object_browser_download_disabled_reason(
            &file_summaries[0].readiness,
            &file_summaries[0].placements,
        )
        .contains("verified settled HDD"));

        let mut available_file = file_summaries[0].clone();
        available_file.readiness = "Available".to_string();
        assert!(object_browser_file_download_available(
            &available_file.readiness,
            &available_file.placements,
        ));

        let multi_copy = vec![
            ObjectBrowserPlacementResponse {
                disk_id: Some("qnap-1057".to_string()),
                disk_label: Some("QNAP bay 1".to_string()),
                location: "hdd_settled".to_string(),
                state: "verified".to_string(),
                size_bytes: 1536,
                checksum: None,
                verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
            },
            ObjectBrowserPlacementResponse {
                disk_id: Some("qnap-1058".to_string()),
                disk_label: Some("QNAP bay 2".to_string()),
                location: "hdd_settled".to_string(),
                state: "verified".to_string(),
                size_bytes: 1536,
                checksum: None,
                verified_at_utc: Some("2026-07-09T10:01:00Z".to_string()),
            },
            ObjectBrowserPlacementResponse {
                disk_id: Some("ssd-landing".to_string()),
                disk_label: Some("Landing SSD".to_string()),
                location: "ssd_landing".to_string(),
                state: "pending".to_string(),
                size_bytes: 1536,
                checksum: None,
                verified_at_utc: None,
            },
        ];
        assert_eq!(
            object_browser_placement_summary(&multi_copy),
            "1 SSD landing · 2 HDD settled · 2 verified HDD copies · 1 pending"
        );
        assert_eq!(
            object_browser_placement_summary_state(&multi_copy),
            "pending"
        );

        let degraded = vec![ObjectBrowserPlacementResponse {
            disk_id: Some("qnap-1059".to_string()),
            disk_label: None,
            location: "hdd_settled".to_string(),
            state: "missing".to_string(),
            size_bytes: 1536,
            checksum: None,
            verified_at_utc: None,
        }];
        assert_eq!(
            object_browser_placement_summary(&degraded),
            "1 HDD settled · 1 degraded/missing"
        );
        assert_eq!(
            object_browser_placement_summary_state(&degraded),
            "degraded"
        );
    }

    #[test]
    fn object_browser_component_contract_covers_rows_downloads_and_empty_states() {
        let source = include_str!("workspace.rs");

        assert!(source.contains("dos-object-browser-table"));
        assert!(source.contains("<th>{ \"Name\" }</th>"));
        assert!(source.contains("<th>{ \"Placement\" }</th>"));
        assert!(source.contains("<th>{ \"Actions\" }</th>"));
        assert!(source.contains("dos-object-browser-folder"));
        assert!(source.contains("dos-object-browser-download"));
        assert!(source.contains("Download folder"));
        assert!(source.contains("Download\""));
        assert!(source.contains("disabled={!download_enabled}"));
        assert!(source.contains("render_object_browser_download_state"));
        assert!(source.contains("data-download-state=\"starting\""));
        assert!(source.contains("data-download-state=\"permission-denied\""));
        assert!(source.contains("render_object_browser_message(\"Empty\", message)"));
        assert!(source
            .contains("render_object_browser_message(\"Files\", \"No files in this folder.\")"));
    }

    #[test]
    fn object_browser_component_contract_covers_placement_badges_and_no_overlap_css() {
        let source = include_str!("workspace.rs");
        let css = include_str!("../styles.css");

        assert!(source.contains("dos-object-browser-placement-stack"));
        assert!(source.contains("dos-object-browser-placement-summary"));
        assert!(source.contains("data-location={placement.location.clone()}"));
        assert!(source.contains("data-state={placement.state.clone()}"));
        assert!(source.contains("data-state={object_browser_state_key(&file.readiness)}"));
        assert!(source.contains("object_browser_placement_summary_state(placements)"));
        assert!(source.contains("object_browser_download_disabled_reason"));
        assert!(source.contains("object_browser_file_download_available"));

        assert!(css.contains(".dos-object-browser-table-wrap {\n  overflow-x: auto;"));
        assert!(css.contains(".dos-object-browser-table {\n  width: 100%;\n  min-width: 1040px;"));
        assert!(css.contains(".dos-object-browser-table td:first-child span"));
        assert!(css.contains("text-overflow: ellipsis;"));
        assert!(
            css.contains(".dos-object-browser-placements {\n  display: flex;\n  flex-wrap: wrap;")
        );
        assert!(css.contains(
            ".dos-object-browser-placement {\n  display: inline-flex;\n  max-width: 220px;"
        ));
        assert!(css.contains("@media (max-width: 980px)"));
        assert!(css.contains(".dos-object-browser-controls,\n  .dos-object-browser-folders {\n    grid-template-columns: repeat(2, minmax(0, 1fr));"));
        assert!(css.contains("@media (max-width: 640px)"));
        assert!(css.contains(".dos-object-browser-controls,\n  .dos-object-browser-folders {\n    grid-template-columns: 1fr;"));
    }

    #[test]
    fn enclosures_live_payload_maps_to_card_summaries() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "enclosures": [{
                "enclosure_id": "qnap-tl-d800c-01",
                "display_name": "QNAP TL-D800C",
                "mount_path": "/srv/dasobjectstore/hdd",
                "connection": {
                    "bus": "usb",
                    "protocol": "uas",
                    "link_speed": "10 Gb/s"
                },
                "health": "watch",
                "drive_count": {
                    "total": 8,
                    "mounted": 7,
                    "healthy": 6,
                    "watch": 1,
                    "suspect": 0,
                    "failed": 0
                },
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "12.5",
                    "free_tib": "87.5",
                    "used_percent_basis_points": 1250
                },
                "last_seen_at_utc": "2026-07-08T08:00:00Z",
                "warnings": [{
                    "code": "smart_watch",
                    "message": "One member drive has a SMART warning."
                }]
            }],
            "selected_enclosure_id": "qnap-tl-d800c-01",
            "details": null,
            "warnings": []
        });
        let view = serde_json::from_value::<EnclosuresPageResponse>(payload)
            .expect("enclosures payload decodes");

        let summaries = enclosure_card_summaries(&view);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "qnap-tl-d800c-01");
        assert_eq!(summaries[0].name, "QNAP TL-D800C");
        assert!(summaries[0].label.contains("usb / uas / 10 Gb/s"));
        assert!(summaries[0].drives.contains("7 mounted of 8"));
        assert_eq!(summaries[0].capacity, "87.5 TiB free of 100.0 TiB");
        assert_eq!(summaries[0].warning_count, 1);
    }

    #[test]
    fn enclosure_prepare_candidate_separates_ssd_and_hdd_devices() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "add_enclosure": {
                "enabled": true,
                "action_kind": "enclosure_add",
                "label": "Add enclosure",
                "state": "ready",
                "administrator": true,
                "supported_enclosure_detected": true,
                "daemon_ready": true,
                "confirmation_required": true,
                "blocked_reason": null,
                "next_step": "Start supported DAS detection and preparation planning."
            },
            "enclosures": [{
                "enclosure_id": "qnap-tl-d800c-01",
                "display_name": "QNAP TL-D800C",
                "mount_path": "/srv/dasobjectstore",
                "connection": {"bus": "usb", "protocol": "uas", "link_speed": "10 Gb/s"},
                "health": "healthy",
                "drive_count": {"total": 3, "mounted": 3, "healthy": 3, "watch": 0, "suspect": 0, "failed": 0},
                "capacity": {"total_tib": "32.0", "used_tib": "0.0", "free_tib": "32.0", "used_percent_basis_points": 0},
                "last_seen_at_utc": "2026-07-08T08:00:00Z",
                "warnings": []
            }],
            "selected_enclosure_id": "qnap-tl-d800c-01",
            "details": {
                "enclosure_id": "qnap-tl-d800c-01",
                "vendor": "QNAP",
                "model": "TL-D800C",
                "serial": "TL-D800C-TEST",
                "firmware": null,
                "slots": [
                    {
                        "slot_number": 0,
                        "drive_id": "nvme-landing",
                        "role": "ssd",
                        "device_path": "/dev/disk/by-id/nvme-landing",
                        "size_tib": "3.6",
                        "health": "healthy",
                        "mounted": true
                    },
                    {
                        "slot_number": 1,
                        "drive_id": "qnap-1057",
                        "role": "hdd",
                        "device_path": "/dev/disk/by-id/usb-qnap-1057",
                        "size_tib": "14.6",
                        "health": "healthy",
                        "mounted": true
                    },
                    {
                        "slot_number": 2,
                        "drive_id": "qnap-1058",
                        "role": "hdd",
                        "device_path": "/dev/disk/by-id/usb-qnap-1058",
                        "size_tib": "14.6",
                        "health": "healthy",
                        "mounted": true
                    }
                ]
            },
            "warnings": []
        });
        let view = serde_json::from_value::<EnclosuresPageResponse>(payload)
            .expect("enclosures payload decodes");

        let candidate =
            enclosure_prepare_candidate(&view, "qnap-tl-d800c-01").expect("prepare candidate");

        assert!(candidate.ready());
        assert_eq!(candidate.ssd_devices.len(), 1);
        assert_eq!(candidate.hdd_devices.len(), 2);
        assert_eq!(
            candidate.ssd_devices[0].device_path,
            "/dev/disk/by-id/nvme-landing"
        );
        assert_eq!(candidate.hdd_devices[0].disk_id, "qnap-1057");
    }

    #[test]
    fn home_dashboard_live_payload_maps_to_metrics_and_attention() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "health": {
                "state": "watch",
                "label": "Watch",
                "warning_count": 1,
                "critical_count": 0,
                "action_count": 1,
                "last_checked_at_utc": null
            },
            "drives": {
                "total": 7,
                "mounted": 7,
                "healthy": 6,
                "watch": 1,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "12.5",
                "free_tib": "87.5",
                "used_percent_basis_points": 1250
            },
            "mounted_enclosures": [],
            "throughput_7d": {
                "window_days": 7,
                "read_tib": "1.0",
                "written_tib": "2.0",
                "ingest_tib": "2.5",
                "avg_read_mib_s": 120,
                "avg_write_mib_s": 240
            },
            "memory_stress": {
                "state": "elevated",
                "pressure_percent": 71,
                "swap_used_percent": 9,
                "page_cache_tib": "0.4",
                "warning": {
                    "code": "memory_pressure_high",
                    "message": "Memory pressure is elevated."
                }
            },
            "smart_warnings": {
                "warning_count": 1,
                "affected_drive_count": 1,
                "warnings": [{
                    "drive_id": "qnap-1057",
                    "severity": "warning",
                    "attribute": "reallocated_sector_count",
                    "message": "SMART attribute is above warning threshold."
                }]
            },
            "object_stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "health": "healthy",
                "object_count": 42,
                "warnings": []
            }]
        });
        let view =
            serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

        let metrics = home_dashboard_metrics(&view);
        assert!(metrics
            .iter()
            .any(|metric| metric.label == "Drives" && metric.value == "7"));
        assert!(metrics
            .iter()
            .any(|metric| metric.label == "Capacity" && metric.value == "87.5 TiB free"));
        assert!(metrics
            .iter()
            .any(|metric| metric.label == "ObjectStores" && metric.value == "1"));

        let attention = home_dashboard_attention(&view);
        assert!(attention
            .iter()
            .any(|item| item.title == "Appliance attention"));
        assert!(attention.iter().any(|item| item.title == "Memory stress"));
        assert!(attention.iter().any(|item| item.title == "SMART qnap-1057"));
    }

    #[test]
    fn home_dashboard_attention_surfaces_capacity_enclosure_and_store_signals() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "health": {
                "state": "watch",
                "label": "Watch",
                "warning_count": 0,
                "critical_count": 0,
                "action_count": 0,
                "last_checked_at_utc": null
            },
            "drives": {
                "total": 7,
                "mounted": 7,
                "healthy": 6,
                "watch": 0,
                "suspect": 1,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "91.0",
                "free_tib": "9.0",
                "used_percent_basis_points": 9100
            },
            "mounted_enclosures": [{
                "enclosure_id": "tl-d800c-1",
                "display_name": "QNAP TL-D800C",
                "mount_path": "/srv/dasobjectstore",
                "connection": {
                    "bus": "usb",
                    "protocol": "uas",
                    "link_speed": "10Gbps"
                },
                "health": "healthy",
                "drive_count": {
                    "total": 7,
                    "mounted": 7,
                    "healthy": 6,
                    "watch": 1,
                    "suspect": 0,
                    "failed": 0
                },
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "91.0",
                    "free_tib": "9.0",
                    "used_percent_basis_points": 9100
                },
                "last_seen_at_utc": "2026-07-08T08:00:00Z",
                "warnings": [{
                    "code": "enclosure_usb_reset",
                    "message": "USB reset observed on this enclosure."
                }]
            }],
            "throughput_7d": {
                "window_days": 7,
                "read_tib": "1.0",
                "written_tib": "2.0",
                "ingest_tib": "2.5",
                "avg_read_mib_s": 120,
                "avg_write_mib_s": 240
            },
            "ingest": {
                "pressure": "critical",
                "queued_jobs": 2,
                "active_jobs": 1,
                "failed_jobs": 1,
                "jobs": [],
                "warnings": [{
                    "code": "ingest_critical_pressure",
                    "message": "SSD ingest pressure is critical; new writes may be blocked."
                }]
            },
            "destage": {
                "pending_objects": 3,
                "copying_objects": 1,
                "verified_objects": 4,
                "objects": [],
                "warnings": [{
                    "code": "destage_objects_need_review",
                    "message": "One or more destage objects need review before SSD eviction."
                }]
            },
            "memory_stress": {
                "state": "nominal",
                "pressure_percent": 31,
                "swap_used_percent": 0,
                "page_cache_tib": "0.4",
                "warning": null
            },
            "smart_warnings": {
                "warning_count": 0,
                "affected_drive_count": 0,
                "warnings": []
            },
            "object_stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "health": "healthy",
                "object_count": 42,
                "endpoint_export_mode": null,
                "warnings": []
            }]
        });
        let view =
            serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

        let attention = home_dashboard_attention(&view);

        assert!(attention.iter().any(|item| item.title == "Drive health"
            && item.state == "warning"
            && item.detail.contains("1 suspect")));
        assert!(attention
            .iter()
            .any(|item| item.title == "Capacity pressure"
                && item.state == "critical"
                && item.detail.contains("91.0 TiB used")));
        assert!(attention.iter().any(|item| item.title == "Ingest queue"
            && item.state == "critical"
            && item.detail.contains("SSD ingest pressure is critical")));
        assert!(attention.iter().any(|item| item.title == "Destage queue"
            && item.state == "warning"
            && item.detail.contains("destage objects need review")));
        assert!(attention
            .iter()
            .any(|item| item.title == "Enclosure QNAP TL-D800C"
                && item.state == "warning"
                && item.detail.contains("USB reset")));
        assert!(attention
            .iter()
            .any(|item| item.title == "ObjectStore zymo_fecal_2025.05"
                && item.state == "warning"
                && item.detail.contains("object-service export mode")));
    }

    #[test]
    fn home_dashboard_attention_clear_state_has_operator_copy() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "health": {
                "state": "healthy",
                "label": "Healthy",
                "warning_count": 0,
                "critical_count": 0,
                "action_count": 0,
                "last_checked_at_utc": "2026-07-08T08:00:00Z"
            },
            "drives": {
                "total": 7,
                "mounted": 7,
                "healthy": 7,
                "watch": 0,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "45.0",
                "free_tib": "55.0",
                "used_percent_basis_points": 4500
            },
            "mounted_enclosures": [{
                "enclosure_id": "tl-d800c-1",
                "display_name": "QNAP TL-D800C",
                "mount_path": "/srv/dasobjectstore",
                "connection": {
                    "bus": "usb",
                    "protocol": "uas",
                    "link_speed": "10Gbps"
                },
                "health": "healthy",
                "drive_count": {
                    "total": 7,
                    "mounted": 7,
                    "healthy": 7,
                    "watch": 0,
                    "suspect": 0,
                    "failed": 0
                },
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "45.0",
                    "free_tib": "55.0",
                    "used_percent_basis_points": 4500
                },
                "last_seen_at_utc": "2026-07-08T08:00:00Z",
                "warnings": []
            }],
            "throughput_7d": {
                "window_days": 7,
                "read_tib": "1.0",
                "written_tib": "2.0",
                "ingest_tib": "2.5",
                "avg_read_mib_s": 120,
                "avg_write_mib_s": 240
            },
            "ingest": {
                "pressure": "normal",
                "queued_jobs": 0,
                "active_jobs": 0,
                "failed_jobs": 0,
                "jobs": [],
                "warnings": []
            },
            "destage": {
                "pending_objects": 0,
                "copying_objects": 0,
                "verified_objects": 2,
                "objects": [],
                "warnings": []
            },
            "memory_stress": {
                "state": "nominal",
                "pressure_percent": 31,
                "swap_used_percent": 0,
                "page_cache_tib": "0.4",
                "warning": null
            },
            "smart_warnings": {
                "warning_count": 0,
                "affected_drive_count": 0,
                "warnings": []
            },
            "object_stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "health": "healthy",
                "object_count": 42,
                "endpoint_export_mode": "s3_bucket",
                "warnings": []
            }]
        });
        let view =
            serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

        let attention = home_dashboard_attention(&view);

        assert_eq!(attention.len(), 1);
        assert_eq!(attention[0].title, "No operator attention required");
        assert!(!attention[0].detail.contains("bootstrapped"));
        assert!(!attention[0].detail.contains("fixture"));
    }

    fn users_groups_workspace_fixture() -> UsersGroupsWorkspaceResponse {
        UsersGroupsWorkspaceResponse {
            host_mode: "standalone".to_string(),
            current_user: Some(LocalUserAuthorityResponse {
                username: "operator".to_string(),
                groups: vec!["sudo".to_string(), "mnemosyne".to_string()],
                sudo_administrator: true,
            }),
            users: vec![StandaloneUserAccountResponse {
                username: "operator".to_string(),
                registered: true,
                created_at_unix_seconds: 1,
                registered_at_unix_seconds: Some(2),
                active_session_count: 1,
            }],
            groups: vec![
                LocalGroupMembershipResponse {
                    group_name: "sudo".to_string(),
                    current_user_member: true,
                    sudo_administrator_group: true,
                },
                LocalGroupMembershipResponse {
                    group_name: "mnemosyne".to_string(),
                    current_user_member: true,
                    sudo_administrator_group: false,
                },
            ],
            groups_file_path: "/opt/dasobjectstore/groups.json".to_string(),
            writer_groups: vec![StorageGroupResponse {
                group_name: "mnemosyne".to_string(),
                display_name: "Mnemosyne".to_string(),
                source: "object_storage_group_registry".to_string(),
                current_user_member: true,
            }],
            operations: vec![
                LocalGroupOperationResponse {
                    kind: "create_local_group".to_string(),
                    label: "Create local writer/admin group".to_string(),
                    requires_sudo_administrator: true,
                    enabled: true,
                    blocked_reason: None,
                },
                LocalGroupOperationResponse {
                    kind: "assign_local_user_to_group".to_string(),
                    label: "Assign local user to group".to_string(),
                    requires_sudo_administrator: true,
                    enabled: true,
                    blocked_reason: None,
                },
            ],
            capabilities: UsersGroupsCapabilitiesResponse {
                product_local_user_registration: true,
                os_local_user_management: true,
                os_local_group_management: true,
                administrator_actions_enabled: true,
            },
            selected_username: Some("operator".to_string()),
            selected_group_name: Some("mnemosyne".to_string()),
            warnings: Vec::new(),
        }
    }
}
