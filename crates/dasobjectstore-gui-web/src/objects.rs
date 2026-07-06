pub const OBJECTS_WORKSPACE_ROUTE: &str = "workspaces/objects";

pub fn objects_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        OBJECTS_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct ObjectsWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(ObjectsWorkspace)]
pub fn objects_workspace(props: &ObjectsWorkspaceProps) -> Html {
    let api_path = objects_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-objects" data-api-route={api_path}>
            <header class="dos-objects__header">
                <h1>{ "Objects" }</h1>
            </header>
            <div class="dos-objects__layout">
                <section class="dos-objects__panel" data-panel="inventory">
                    <h2>{ "Inventory" }</h2>
                </section>
                <section class="dos-objects__panel" data-panel="hashes">
                    <h2>{ "Hashes" }</h2>
                </section>
                <section class="dos-objects__panel" data-panel="copy-locations">
                    <h2>{ "Copy Locations" }</h2>
                </section>
                <section class="dos-objects__panel" data-panel="reproducibility-source">
                    <h2>{ "Reproducibility Source" }</h2>
                </section>
                <section class="dos-objects__panel" data-panel="export-download">
                    <h2>{ "Export and Download" }</h2>
                </section>
                <section class="dos-objects__panel" data-panel="repair-redownload">
                    <h2>{ "Repair and Redownload" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::objects_workspace_api_path;

    #[test]
    fn builds_objects_workspace_api_path() {
        assert_eq!(
            objects_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/objects"
        );
    }
}
