use crate::mount::{FrontendHost, FrontendMount};
use yew::prelude::*;

#[function_component(App)]
pub fn app() -> Html {
    let mount = FrontendMount::default_for(FrontendHost::Synoptikon);

    html! {
        <main data-host={mount.host.name()} data-api-base={mount.api_base_path}>
            <h1>{ "DASObjectStore" }</h1>
        </main>
    }
}
