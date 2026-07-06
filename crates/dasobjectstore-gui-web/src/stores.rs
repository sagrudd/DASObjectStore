pub const STORES_WORKSPACE_ROUTE: &str = "workspaces/stores";

pub fn stores_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        STORES_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct StoresWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(StoresWorkspace)]
pub fn stores_workspace(props: &StoresWorkspaceProps) -> Html {
    let api_path = stores_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-stores" data-api-route={api_path}>
            <header class="dos-stores__header">
                <h1>{ "Stores" }</h1>
            </header>
            <div class="dos-stores__layout">
                <section class="dos-stores__panel" data-panel="policy-create-modify">
                    <h2>{ "Policy Create and Modify" }</h2>
                </section>
                <section class="dos-stores__panel" data-panel="resize">
                    <h2>{ "Resize" }</h2>
                </section>
                <section class="dos-stores__panel" data-panel="redundancy">
                    <h2>{ "Redundancy" }</h2>
                </section>
                <section class="dos-stores__panel" data-panel="retention">
                    <h2>{ "Retention" }</h2>
                </section>
                <section class="dos-stores__panel" data-panel="export-mode">
                    <h2>{ "Export Mode" }</h2>
                </section>
                <section class="dos-stores__panel" data-panel="capacity-behavior">
                    <h2>{ "Capacity Behavior" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::stores_workspace_api_path;

    #[test]
    fn builds_stores_workspace_api_path() {
        assert_eq!(
            stores_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/stores"
        );
    }
}
