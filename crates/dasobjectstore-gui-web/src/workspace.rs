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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DashboardMetric {
    pub label: &'static str,
    pub value: &'static str,
    pub detail: &'static str,
    pub state: &'static str,
}

pub fn fallback_dashboard_metrics() -> Vec<DashboardMetric> {
    vec![
        DashboardMetric {
            label: "Drives",
            value: "0",
            detail: "Live daemon inventory pending",
            state: "Pending",
        },
        DashboardMetric {
            label: "DAS enclosures",
            value: "0 mounted",
            detail: "Supported enclosure mapping pending",
            state: "Pending",
        },
        DashboardMetric {
            label: "Capacity",
            value: "0 B",
            detail: "Used and available TiB will appear after inventory",
            state: "Pending",
        },
        DashboardMetric {
            label: "7-day throughput",
            value: "Pending",
            detail: "Ingress and destage rates require daemon metrics",
            state: "Pending",
        },
        DashboardMetric {
            label: "Memory stress",
            value: "Unknown",
            detail: "Host memory telemetry pending",
            state: "Pending",
        },
        DashboardMetric {
            label: "SMART warnings",
            value: "0",
            detail: "No live SMART feed attached to this page yet",
            state: "Pending",
        },
    ]
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

    html! {
        <section class="dos-page" data-page="home" data-api-route={api_path}>
            <PageHeader
                eyebrow="Appliance"
                title="Home"
                summary="Current operating posture for local storage, ingress, and object service."
            />
            <div class="dos-metric-grid">
                { for fallback_dashboard_metrics().into_iter().map(|metric| html! {
                    <section class="dos-card dos-metric-card" data-state={metric.state}>
                        <div class="dos-card-row">
                            <span class="dos-card-label">{ metric.label }</span>
                            <span class="dos-status-pill">{ metric.state }</span>
                        </div>
                        <strong>{ metric.value }</strong>
                        <p>{ metric.detail }</p>
                    </section>
                }) }
            </div>
            <section class="dos-card dos-wide-card">
                <div>
                    <span class="dos-card-label">{ "Attention" }</span>
                    <h2>{ "Live dashboard telemetry is being bootstrapped." }</h2>
                    <p>{ "The page is wired to the Home API contract and will populate drive, enclosure, capacity, throughput, memory, and SMART data as daemon inventory is connected." }</p>
                </div>
            </section>
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
    let enclosures = fallback_enclosures();
    let selected_id = use_state(|| {
        enclosures
            .first()
            .map(|enclosure| enclosure.id.to_string())
            .unwrap_or_default()
    });
    let selected = enclosures
        .iter()
        .find(|enclosure| enclosure.id == selected_id.as_str())
        .copied();

    html! {
        <section class="dos-page" data-page="enclosures" data-api-route={api_path}>
            <PageHeader
                eyebrow="Storage hardware"
                title="Enclosures"
                summary="Physical shelves and landing media grouped for operator review."
            />
            <div class="dos-two-column">
                <div class="dos-card-list">
                    <section class="dos-card dos-create-card" data-action="enclosure_add">
                        <span class="dos-create-mark">{ "+" }</span>
                        <h2>{ "Add enclosure" }</h2>
                        <p>{ "Admin workflow: detect supported DAS hardware, identify SSD/HDD media, review format risk, then submit the daemon preparation job." }</p>
                        <span class="dos-status-pill">{ "Admin only" }</span>
                    </section>
                    if enclosures.is_empty() {
                        <section class="dos-card dos-empty-card">
                            <span class="dos-card-label">{ "Inventory" }</span>
                            <h2>{ "No live enclosures reported yet." }</h2>
                            <p>{ "Supported DAS enclosures will appear here as cards with branding, topology, capacity, health, and drive membership." }</p>
                        </section>
                    }
                    { for enclosures.iter().map(|enclosure| {
                        let is_selected = enclosure.id == selected_id.as_str();
                        let selected_id = selected_id.clone();
                        let enclosure_id = enclosure.id.to_string();
                        html! {
                            <button
                                type="button"
                                class={classes!("dos-card", "dos-enclosure-card", is_selected.then_some("is-selected"))}
                                aria-pressed={is_selected.to_string()}
                                onclick={Callback::from(move |_| selected_id.set(enclosure_id.clone()))}
                            >
                                <div class="dos-card-row">
                                    <span class="dos-card-label">{ enclosure.role }</span>
                                    <span class="dos-status-pill">{ enclosure.health }</span>
                                </div>
                                <strong>{ enclosure.name }</strong>
                                <p>{ format!("{}/{} bays · {}", enclosure.bays_used, enclosure.bays_total, enclosure.capacity) }</p>
                            </button>
                        }
                    }) }
                </div>
                <section class="dos-card dos-detail-panel">
                    { match selected {
                        Some(enclosure) => html! {
                            <>
                                <span class="dos-card-label">{ "Enclosure detail" }</span>
                                <h2>{ enclosure.name }</h2>
                                <dl class="dos-detail-list">
                                    <div><dt>{ "Role" }</dt><dd>{ enclosure.role }</dd></div>
                                    <div><dt>{ "Health" }</dt><dd>{ enclosure.health }</dd></div>
                                    <div><dt>{ "Bays" }</dt><dd>{ format!("{}/{}", enclosure.bays_used, enclosure.bays_total) }</dd></div>
                                    <div><dt>{ "Capacity" }</dt><dd>{ enclosure.capacity }</dd></div>
                                </dl>
                                <p>{ enclosure.note }</p>
                            </>
                        },
                        None => html! {
                            <>
                                <span class="dos-card-label">{ "Enclosure detail" }</span>
                                <h2>{ "Select an enclosure" }</h2>
                                <p>{ "Drive cards, SMART warnings, bay mapping, mount state, and administrator actions will appear here once a supported enclosure is detected." }</p>
                            </>
                        },
                    } }
                </section>
            </div>
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

    html! {
        <section class="dos-page" data-page="objectstores" data-api-route={api_path}>
            <PageHeader
                eyebrow="Managed stores"
                title="ObjectStores"
                summary="Operational view of store policies, capacity, and service state."
            />
            <div class="dos-store-grid">
                <section class="dos-card dos-create-card" data-action="store_create">
                    <span class="dos-create-mark">{ "+" }</span>
                    <h2>{ "Create ObjectStore" }</h2>
                    <p>{ "Admin workflow: assign a writer group from /opt/dasobjectstore/groups.json, choose enclosure, object type, and redundancy, then submit the daemon creation plan." }</p>
                    <span class="dos-status-pill">{ "Admin only" }</span>
                </section>
                if fallback_object_stores().is_empty() {
                    <section class="dos-card dos-empty-card">
                        <span class="dos-card-label">{ "Inventory" }</span>
                        <h2>{ "No object stores reported yet." }</h2>
                        <p>{ "Available stores will be shown with name, group policy, public/writeable status, used capacity, object count, and warnings." }</p>
                    </section>
                }
                { for fallback_object_stores().into_iter().map(|store| html! {
                    <section class="dos-card dos-store-card" data-store-id={store.id}>
                        <div class="dos-card-row">
                            <span class="dos-card-label">{ store.policy }</span>
                            <span class="dos-status-pill">{ store.state }</span>
                        </div>
                        <strong>{ store.name }</strong>
                        <p>{ store.capacity }</p>
                        <p>{ store.objects }</p>
                    </section>
                }) }
            </div>
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
        bioinformatics_workspace_api_path, enclosures_workspace_api_path, fallback_enclosures,
        fallback_object_stores, home_workspace_api_path, objectstores_workspace_api_path,
        WorkspacePage, BIOINFORMATICS_WORKSPACE_ROUTE, ENCLOSURES_WORKSPACE_ROUTE,
        HOME_WORKSPACE_ROUTE, OBJECTSTORES_WORKSPACE_ROUTE, PRIMARY_NAVIGATION,
    };

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
}
