pub const DISKS_WORKSPACE_ROUTE: &str = "workspaces/disks";

pub fn disks_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        DISKS_WORKSPACE_ROUTE
    )
}

#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct DisksWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[function_component(DisksWorkspace)]
pub fn disks_workspace(props: &DisksWorkspaceProps) -> Html {
    let api_path = disks_workspace_api_path(&props.api_base_path);

    html! {
        <section class="dos-disks" data-api-route={api_path}>
            <header class="dos-disks__header">
                <h1>{ "Disks" }</h1>
            </header>
            <div class="dos-disks__layout">
                <section class="dos-disks__panel" data-panel="enclosure-grouping">
                    <h2>{ "Enclosures" }</h2>
                </section>
                <section class="dos-disks__panel" data-panel="health">
                    <h2>{ "Health" }</h2>
                </section>
                <section class="dos-disks__panel" data-panel="usb-smart-warnings">
                    <h2>{ "USB and SMART Warnings" }</h2>
                </section>
                <section class="dos-disks__panel" data-panel="benchmark-drift">
                    <h2>{ "Benchmark Drift" }</h2>
                </section>
                <section class="dos-disks__panel" data-panel="disk-actions">
                    <h2>{ "Migrate, Drain, Replace, Retire" }</h2>
                </section>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::disks_workspace_api_path;

    #[test]
    fn builds_disks_workspace_api_path() {
        assert_eq!(
            disks_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/disks"
        );
    }
}
