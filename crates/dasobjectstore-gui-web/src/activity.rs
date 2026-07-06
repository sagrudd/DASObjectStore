pub const ACTIVITY_WORKSPACE_ROUTE: &str = "workspaces/activity";

pub fn activity_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ACTIVITY_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct ActivityWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(ActivityWorkspace)]
pub fn activity_workspace(props: &ActivityWorkspaceProps) -> Html {
    let api_path = activity_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-activity" data-api-route={api_path}>
            <header class="dos-activity__header">
                <h1>{ "Activity" }</h1>
            </header>
            <div class="dos-activity__layout">
                <section class="dos-activity__panel" data-panel="ingest-queue">
                    <h2>{ "Ingest Queue" }</h2>
                </section>
                <section class="dos-activity__panel" data-panel="destage-queue">
                    <h2>{ "Destage Queue" }</h2>
                </section>
                <section class="dos-activity__panel" data-panel="repair-tasks">
                    <h2>{ "Repair Tasks" }</h2>
                </section>
                <section class="dos-activity__panel" data-panel="audit-provenance">
                    <h2>{ "Audit and Provenance" }</h2>
                </section>
                <section class="dos-activity__panel" data-panel="long-running-operations">
                    <h2>{ "Long-Running Operations" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::activity_workspace_api_path;

    #[test]
    fn builds_activity_workspace_api_path() {
        assert_eq!(
            activity_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/activity"
        );
    }
}
