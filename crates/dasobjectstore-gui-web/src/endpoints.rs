pub const ENDPOINTS_WORKSPACE_ROUTE: &str = "workspaces/endpoints";

pub fn endpoints_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENDPOINTS_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct EndpointsWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(EndpointsWorkspace)]
pub fn endpoints_workspace(props: &EndpointsWorkspaceProps) -> Html {
    let api_path = endpoints_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-endpoints" data-api-route={api_path}>
            <header class="dos-endpoints__header">
                <h1>{ "Endpoints" }</h1>
            </header>
            <div class="dos-endpoints__layout">
                <section class="dos-endpoints__panel" data-panel="das-pools">
                    <h2>{ "DAS Pools" }</h2>
                </section>
                <section class="dos-endpoints__panel" data-panel="external-nas-nfs">
                    <h2>{ "External NAS and NFS" }</h2>
                </section>
                <section class="dos-endpoints__panel" data-panel="s3-service-state">
                    <h2>{ "S3 Service State" }</h2>
                </section>
                <section class="dos-endpoints__panel" data-panel="mneion-export">
                    <h2>{ "Mneion Export" }</h2>
                </section>
                <section class="dos-endpoints__panel" data-panel="binding-readiness">
                    <h2>{ "Binding Readiness" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::endpoints_workspace_api_path;

    #[test]
    fn builds_endpoints_workspace_api_path() {
        assert_eq!(
            endpoints_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints"
        );
    }
}
