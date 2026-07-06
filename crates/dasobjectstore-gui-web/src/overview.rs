pub const OVERVIEW_WORKSPACE_ROUTE: &str = "workspaces/overview";

pub fn overview_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        OVERVIEW_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct OverviewWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(OverviewWorkspace)]
pub fn overview_workspace(props: &OverviewWorkspaceProps) -> Html {
    let api_path = overview_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-overview" data-api-route={api_path}>
            <header class="dos-overview__header">
                <h1>{ "Overview" }</h1>
            </header>
            <div class="dos-overview__grid">
                <section class="dos-overview__panel" data-panel="capacity">
                    <h2>{ "Capacity" }</h2>
                </section>
                <section class="dos-overview__panel" data-panel="ingest-pressure">
                    <h2>{ "Ingest Pressure" }</h2>
                </section>
                <section class="dos-overview__panel" data-panel="destage-urgency">
                    <h2>{ "Destage Urgency" }</h2>
                </section>
                <section class="dos-overview__panel" data-panel="endpoint-state">
                    <h2>{ "Endpoint State" }</h2>
                </section>
                <section class="dos-overview__panel" data-panel="required-actions">
                    <h2>{ "Required Actions" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::overview_workspace_api_path;

    #[test]
    fn builds_overview_workspace_api_path() {
        assert_eq!(
            overview_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/overview"
        );
    }
}
