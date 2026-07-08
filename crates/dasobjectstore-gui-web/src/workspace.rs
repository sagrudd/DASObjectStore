#[cfg(test)]
use crate::api::AdminJobSummary;
#[cfg(target_arch = "wasm32")]
use crate::api::{
    AddEnclosureAffordanceResponse, AdminJobCancelRequest, AdminJobCancelResponse,
    AdminJobStatusResponse, AdminJobSummary, BioinformaticsWorkspaceResponse,
    DasEnclosureCardResponse, DasEnclosureDetailResponse, EnclosurePrepareHddDevice,
    EnclosurePrepareRequest, EnclosurePrepareResponse,
};
use crate::api::{
    EnclosureDriveSlotResponse, EnclosuresPageResponse, HomeDashboardResponse,
    ObjectStoresPageResponse,
};
#[cfg(target_arch = "wasm32")]
use gloo_timers::callback::Timeout;

pub const HOME_WORKSPACE_ROUTE: &str = "dashboard/home";
pub const ENCLOSURES_WORKSPACE_ROUTE: &str = "dashboard/enclosures";
pub const OBJECTSTORES_WORKSPACE_ROUTE: &str = "dashboard/object-stores";
pub const BIOINFORMATICS_WORKSPACE_ROUTE: &str = "workspaces/bioinformatics";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspacePage {
    Home,
    Enclosures,
    ObjectStores,
    Bioinformatics,
}

impl WorkspacePage {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Enclosures => "enclosures",
            Self::ObjectStores => "objectstores",
            Self::Bioinformatics => "bioinformatics",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Enclosures => "Enclosures",
            Self::ObjectStores => "ObjectStores",
            Self::Bioinformatics => "Bioinformatics",
        }
    }

    pub fn api_path(self, api_base_path: &str) -> String {
        match self {
            Self::Home => home_workspace_api_path(api_base_path),
            Self::Enclosures => enclosures_workspace_api_path(api_base_path),
            Self::ObjectStores => objectstores_workspace_api_path(api_base_path),
            Self::Bioinformatics => bioinformatics_workspace_api_path(api_base_path),
        }
    }
}

pub const PRIMARY_NAVIGATION: [WorkspacePage; 4] = [
    WorkspacePage::Home,
    WorkspacePage::Enclosures,
    WorkspacePage::ObjectStores,
    WorkspacePage::Bioinformatics,
];

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

pub fn bioinformatics_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        BIOINFORMATICS_WORKSPACE_ROUTE
    )
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

#[cfg(target_arch = "wasm32")]
use web_sys::{HtmlInputElement, HtmlSelectElement};
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
                { render_add_enclosure_card(
                    &view.add_enclosure,
                    enclosure_prepare_candidate(view, &active_id),
                    wizard_state,
                    api_base_path,
                ) }
                { for enclosure_card_summaries(view).into_iter().map(|summary| {
                    render_enclosure_card(summary, &active_id, selected_id.clone())
                }) }
            </div>
            { render_enclosure_detail_panel(view, &active_id) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct EnclosureWizardState {
    open: bool,
    selected_ssd: String,
    selected_hdds: Vec<String>,
    mount_root: String,
    filesystem: String,
    owner: String,
    allow_format: bool,
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

#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
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
    let confirmed = state.allow_format && state.confirmation_phrase.trim() == "confirm prepare das";
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
            clear_enclosure_job_monitor(&mut next);
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

    {
        let api_path = api_path.clone();
        let object_stores_state = object_stores_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                object_stores_state.set(page_load_state_from_result(
                    crate::api::get_object_stores_dashboard(&path).await,
                    |view| {
                        view.stores.is_empty().then(|| {
                            view.warnings
                                .first()
                                .map(|warning| warning.message.clone())
                                .unwrap_or_else(|| "No object stores reported.".to_string())
                        })
                    },
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="objectstores" data-api-route={api_path}>
            <PageHeader
                eyebrow="Managed stores"
                title="ObjectStores"
                summary="Operational view of store policies, capacity, and service state."
            />
            { render_object_stores_state(&*object_stores_state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_stores_state(state: &ApiLoadState<ObjectStoresPageResponse>) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None) }
                { render_object_stores_state_message(
                    "Loading",
                    "Loading object-store inventory",
                    "The Web console is requesting daemon-backed store registry, policy, capacity, endpoint, and warning state.",
                ) }
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_object_store_inventory(view)
        }
        ApiLoadState::Empty(message) => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None) }
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
fn render_object_store_inventory(view: &ObjectStoresPageResponse) -> Html {
    html! {
        <div class="dos-store-grid">
            { render_object_store_create_card(Some(view)) }
            { for object_store_card_summaries(view).into_iter().map(render_object_store_card) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_create_card(view: Option<&ObjectStoresPageResponse>) -> Html {
    let (status, detail) = match view {
        Some(view) if view.create_object_store.enabled => (
            "Available".to_string(),
            format!(
                "Admin workflow: create a {} store with {} copy/copies and {} export after daemon plan review.",
                view.create_object_store.defaults.store_class,
                view.create_object_store.defaults.required_copies,
                view.create_object_store.defaults.endpoint_export_mode
            ),
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

    html! {
        <section class="dos-card dos-create-card" data-action="store_create">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create ObjectStore" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
        </section>
    }
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
                        view.supported_object_types.is_empty().then(|| {
                            "No bioinformatics object types were reported by the daemon workspace API."
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
    html! {
        <section class="dos-card dos-placeholder-card" data-state={if view.available { "available" } else { "reserved" }}>
            <span class="dos-card-label">{ if view.available { "Workflow ready" } else { "Reserved workflow" } }</span>
            <h2>{ if view.available { "Bioinformatics readiness is available." } else { "Bioinformatics workspace is reserved." } }</h2>
            <p>{ &view.message }</p>
            <div class="dos-chip-row">
                { for view.supported_object_types.iter().map(|object_type| html! {
                    <span class="dos-status-pill">{ object_type }</span>
                }) }
            </div>
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
        admin_job_percent, admin_job_progress_text, admin_job_state_is_terminal,
        bioinformatics_workspace_api_path, enclosure_card_summaries, enclosure_prepare_candidate,
        enclosures_workspace_api_path, home_dashboard_attention, home_dashboard_metrics,
        home_workspace_api_path, object_store_card_summaries, objectstores_workspace_api_path,
        ApiLoadState, WorkspacePage, BIOINFORMATICS_WORKSPACE_ROUTE, ENCLOSURES_WORKSPACE_ROUTE,
        HOME_WORKSPACE_ROUTE, OBJECTSTORES_WORKSPACE_ROUTE, PRIMARY_NAVIGATION,
    };
    use crate::api::{
        AdminJobProgress, AdminJobSummary, EnclosuresPageResponse, HomeDashboardResponse,
        ObjectStoresPageResponse,
    };
    use crate::stores::STORES_WORKSPACE_ROUTE;
    use crate::users_groups::USERS_GROUPS_WORKSPACE_ROUTE;

    #[test]
    fn primary_navigation_uses_redesign_labels() {
        let labels: Vec<_> = PRIMARY_NAVIGATION.iter().map(|page| page.label()).collect();

        assert_eq!(
            labels,
            vec!["Home", "Enclosures", "ObjectStores", "Bioinformatics"]
        );
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
            WorkspacePage::Bioinformatics.api_path(base),
            "/products/dasobjectstore/api/v1/workspaces/bioinformatics"
        );
    }

    #[test]
    fn frontend_page_routes_use_dashboard_contracts() {
        assert_eq!(HOME_WORKSPACE_ROUTE, "dashboard/home");
        assert_eq!(ENCLOSURES_WORKSPACE_ROUTE, "dashboard/enclosures");
        assert_eq!(OBJECTSTORES_WORKSPACE_ROUTE, "dashboard/object-stores");
        assert_eq!(home_workspace_api_path("/api/"), "/api/dashboard/home");
        assert_eq!(
            enclosures_workspace_api_path("/api/"),
            "/api/dashboard/enclosures"
        );
        assert_eq!(
            objectstores_workspace_api_path("/api/"),
            "/api/dashboard/object-stores"
        );
    }

    #[test]
    fn primary_navigation_excludes_legacy_holder_routes() {
        let base = "/products/dasobjectstore/api/v1/";
        let primary_paths: Vec<_> = PRIMARY_NAVIGATION
            .iter()
            .map(|page| page.api_path(base))
            .collect();

        assert!(!primary_paths
            .iter()
            .any(|path| path.ends_with(STORES_WORKSPACE_ROUTE)));
        assert!(!primary_paths
            .iter()
            .any(|path| path.ends_with(USERS_GROUPS_WORKSPACE_ROUTE)));
        assert!(primary_paths
            .iter()
            .any(|path| path.ends_with(OBJECTSTORES_WORKSPACE_ROUTE)));
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
}
