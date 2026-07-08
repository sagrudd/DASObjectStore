#[cfg(target_arch = "wasm32")]
use crate::api::{DasEnclosureCardResponse, DasEnclosureDetailResponse};
use crate::api::{EnclosuresPageResponse, HomeDashboardResponse, ObjectStoresPageResponse};

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
    Loaded(T),
    Empty(String),
    PermissionDenied(String),
    Error(String),
    Stale { value: T, message: String },
}

impl<T> ApiLoadState<T> {
    pub fn loaded(value: T) -> Self {
        Self::Loaded(value)
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

pub fn fallback_dashboard_metrics() -> Vec<DashboardMetric> {
    vec![
        DashboardMetric::new("Drives", "0", "Live daemon inventory pending", "Pending"),
        DashboardMetric::new(
            "DAS enclosures",
            "0 mounted",
            "Supported enclosure mapping pending",
            "Pending",
        ),
        DashboardMetric::new(
            "Capacity",
            "0 B",
            "Used and available TiB will appear after inventory",
            "Pending",
        ),
        DashboardMetric::new(
            "7-day throughput",
            "Pending",
            "Ingress and destage rates require daemon metrics",
            "Pending",
        ),
        DashboardMetric::new(
            "Memory stress",
            "Unknown",
            "Host memory telemetry pending",
            "Pending",
        ),
        DashboardMetric::new(
            "SMART warnings",
            "0",
            "No live SMART feed attached to this page yet",
            "Pending",
        ),
    ]
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
    if let Some(warning) = &view.memory_stress.warning {
        items.push(DashboardAttentionItem::new(
            "Memory stress",
            warning.message.clone(),
            &view.memory_stress.state,
        ));
    }
    for warning in view.smart_warnings.warnings.iter().take(3) {
        items.push(DashboardAttentionItem::new(
            format!("SMART {}", warning.drive_id),
            format!("{}: {}", warning.attribute, warning.message),
            &warning.severity,
        ));
    }
    if items.is_empty() {
        items.push(DashboardAttentionItem::new(
            "No dashboard warnings reported",
            "The daemon dashboard API did not report active Home attention items.",
            "clear",
        ));
    }
    items
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnclosureSummary {
    pub id: &'static str,
    pub name: &'static str,
    pub role: &'static str,
    pub health: &'static str,
    pub bays_used: u8,
    pub bays_total: u8,
    pub capacity: &'static str,
    pub note: &'static str,
}

pub fn fallback_enclosures() -> Vec<EnclosureSummary> {
    Vec::new()
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectStoreSummary {
    pub id: &'static str,
    pub name: &'static str,
    pub policy: &'static str,
    pub capacity: &'static str,
    pub objects: &'static str,
    pub state: &'static str,
}

pub fn fallback_object_stores() -> Vec<ObjectStoreSummary> {
    Vec::new()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectStoreCardSummary {
    pub id: String,
    pub label: String,
    pub name: String,
    pub health: String,
    pub policy: String,
    pub capacity: String,
    pub objects: String,
    pub writer_group: String,
    pub endpoint: String,
    pub warning_count: usize,
    pub last_ingested: String,
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
                match crate::api::get_home_dashboard(&path).await {
                    Ok(view) => dashboard_state.set(ApiLoadState::loaded(view)),
                    Err(error) if error.is_permission_denied() => {
                        dashboard_state.set(ApiLoadState::PermissionDenied(error.message))
                    }
                    Err(error) => dashboard_state.set(ApiLoadState::Error(error.message)),
                }
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
                    { for fallback_dashboard_metrics().into_iter().map(render_metric_card) }
                </div>
                <section class="dos-card dos-wide-card dos-loading-card">
                    <span class="dos-card-label">{ "Loading" }</span>
                    <h2>{ "Loading live dashboard telemetry." }</h2>
                    <p>{ "The Web console is requesting daemon-backed drive, capacity, throughput, memory, and SMART state." }</p>
                </section>
            </>
        },
        ApiLoadState::Loaded(view) | ApiLoadState::Stale { value: view, .. } => html! {
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
        ApiLoadState::Error(message) => {
            render_home_state_message("Error", "Unable to load Home dashboard", message)
        }
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

    {
        let api_path = api_path.clone();
        let enclosures_state = enclosures_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::get_enclosures_dashboard(&path).await {
                    Ok(view) if view.enclosures.is_empty() => {
                        let message = view
                            .warnings
                            .first()
                            .map(|warning| warning.message.clone())
                            .unwrap_or_else(|| "No supported DAS enclosures reported.".to_string());
                        enclosures_state.set(ApiLoadState::Empty(message));
                    }
                    Ok(view) => enclosures_state.set(ApiLoadState::loaded(view)),
                    Err(error) if error.is_permission_denied() => {
                        enclosures_state.set(ApiLoadState::PermissionDenied(error.message))
                    }
                    Err(error) => enclosures_state.set(ApiLoadState::Error(error.message)),
                }
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
            { render_enclosures_state(&*enclosures_state, selected_id) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosures_state(
    state: &ApiLoadState<EnclosuresPageResponse>,
    selected_id: UseStateHandle<String>,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-two-column">
                <div class="dos-card-list">
                    { render_add_enclosure_card() }
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
        ApiLoadState::Loaded(view) | ApiLoadState::Stale { value: view, .. } => {
            render_enclosure_inventory(view, selected_id)
        }
        ApiLoadState::Empty(message) => {
            render_enclosures_state_message("Inventory", "No live enclosures reported yet", message)
        }
        ApiLoadState::PermissionDenied(message) => render_enclosures_state_message(
            "Permission denied",
            "Enclosure inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::Error(message) => {
            render_enclosures_state_message("Error", "Unable to load enclosure inventory", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn render_enclosure_inventory(
    view: &EnclosuresPageResponse,
    selected_id: UseStateHandle<String>,
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
                { render_add_enclosure_card() }
                { for enclosure_card_summaries(view).into_iter().map(|summary| {
                    render_enclosure_card(summary, &active_id, selected_id.clone())
                }) }
            </div>
            { render_enclosure_detail_panel(view, &active_id) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_add_enclosure_card() -> Html {
    html! {
        <section class="dos-card dos-create-card" data-action="enclosure_add">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Add enclosure" }</h2>
            <p>{ "Admin workflow: detect supported DAS hardware, identify SSD/HDD media, review format risk, then submit the daemon preparation job." }</p>
            <span class="dos-status-pill">{ "Admin only" }</span>
        </section>
    }
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
                { for detail.slots.iter().map(|slot| html! {
                    <div class="dos-slot-row">
                        <span>{ format!("Bay {}", slot.slot_number) }</span>
                        <strong>{ &slot.drive_id }</strong>
                        <span>{ format!("{} TiB · {} · {}", slot.size_tib, slot.health, if slot.mounted { "mounted" } else { "not mounted" }) }</span>
                    </div>
                }) }
            </div>
        </>
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
                match crate::api::get_object_stores_dashboard(&path).await {
                    Ok(view) if view.stores.is_empty() => {
                        let message = view
                            .warnings
                            .first()
                            .map(|warning| warning.message.clone())
                            .unwrap_or_else(|| "No object stores reported.".to_string());
                        object_stores_state.set(ApiLoadState::Empty(message));
                    }
                    Ok(view) => object_stores_state.set(ApiLoadState::loaded(view)),
                    Err(error) if error.is_permission_denied() => {
                        object_stores_state.set(ApiLoadState::PermissionDenied(error.message))
                    }
                    Err(error) => object_stores_state.set(ApiLoadState::Error(error.message)),
                }
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
        ApiLoadState::Loaded(view) | ApiLoadState::Stale { value: view, .. } => {
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
        ApiLoadState::Error(message) => render_object_stores_state_message(
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
            <p>{ store.policy }</p>
            <p>{ store.capacity }</p>
            <p>{ format!("{} · writer group: {}", store.objects, store.writer_group) }</p>
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

    html! {
        <section class="dos-page" data-page="bioinformatics" data-api-route={api_path}>
            <PageHeader
                eyebrow="Workflow integration"
                title="Bioinformatics"
                summary="Placeholder for run provenance, analysis handoff, and Mnemosyne integration state."
            />
            <section class="dos-card dos-placeholder-card">
                <span class="dos-card-label">{ "Reserved workflow" }</span>
                <h2>{ "Bioinformatics workspace is reserved." }</h2>
                <p>{ "This page will surface sequencing run context, object lineage, and downstream analysis readiness." }</p>
            </section>
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
        bioinformatics_workspace_api_path, enclosure_card_summaries, enclosures_workspace_api_path,
        fallback_enclosures, fallback_object_stores, home_dashboard_attention,
        home_dashboard_metrics, home_workspace_api_path, object_store_card_summaries,
        objectstores_workspace_api_path, WorkspacePage, BIOINFORMATICS_WORKSPACE_ROUTE,
        ENCLOSURES_WORKSPACE_ROUTE, HOME_WORKSPACE_ROUTE, OBJECTSTORES_WORKSPACE_ROUTE,
        PRIMARY_NAVIGATION,
    };
    use crate::api::{EnclosuresPageResponse, HomeDashboardResponse, ObjectStoresPageResponse};

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
    fn bioinformatics_route_is_stable() {
        assert_eq!(BIOINFORMATICS_WORKSPACE_ROUTE, "workspaces/bioinformatics");
        assert_eq!(
            bioinformatics_workspace_api_path("/api/"),
            "/api/workspaces/bioinformatics"
        );
    }

    #[test]
    fn fallback_enclosures_support_card_and_detail_views() {
        let enclosures = fallback_enclosures();

        assert!(enclosures.is_empty());
        assert!(enclosures
            .iter()
            .all(|enclosure| enclosure.bays_used <= enclosure.bays_total));
    }

    #[test]
    fn fallback_object_stores_leave_room_for_create_card() {
        let stores = fallback_object_stores();

        assert!(stores.is_empty());
        assert!(stores.iter().all(|store| !store.id.is_empty()));
        assert!(stores.iter().all(|store| !store.policy.is_empty()));
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
}
