use crate::mount::{FrontendHost, FrontendMount};
use crate::overview::OverviewWorkspace;
use yew::prelude::*;

#[function_component(App)]
pub fn app() -> Html {
    let mount = FrontendMount::default_for(FrontendHost::Synoptikon);
    let api_base_path = mount.api_base_path.clone();

    html! {
        <main data-host={mount.host.name()} data-api-base={api_base_path.clone()}>
            <OverviewWorkspace api_base_path={api_base_path} />
        </main>
    }
}
