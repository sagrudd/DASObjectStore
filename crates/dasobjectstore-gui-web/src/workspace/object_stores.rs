#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
#[function_component(ObjectStoresPage)]
pub fn object_stores_page(props: &ObjectStoresPageProps) -> Html {
    let api_path = WorkspacePage::ObjectStores.api_path(&props.api_base_path);
    let object_stores_state = use_state(|| ApiLoadState::<ObjectStoresPageResponse>::Loading);
    let create_state = use_state(|| ObjectStoreCreateFormState::from_view(None));
    let configure_state = use_state(|| ObjectStoreConfigureFormState::from_view(None));
    let subobject_state = use_state(|| SubObjectFormState::from_view(None));
    let browser_endpoint = use_state(String::new);
    let browser_prefix = use_state(String::new);
    let browser_search = use_state(String::new);
    let browser_sort = use_state(|| "name_asc".to_string());
    let browser_state =
        use_state(|| ApiLoadState::<ObjectBrowserResponse>::Empty("Select an ObjectStore.".into()));
    let browser_download_state = use_state(|| ObjectBrowserDownloadState::Idle);

    {
        let api_path = api_path.clone();
        let object_stores_state = object_stores_state.clone();
        let browser_endpoint = browser_endpoint.clone();
        let browser_prefix = browser_prefix.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::get_object_stores_dashboard(&path).await;
                if let Ok(view) = &result {
                    if browser_endpoint.trim().is_empty() {
                        if let Some(endpoint) = object_browser_initial_endpoint(view) {
                            browser_endpoint.set(endpoint);
                            browser_prefix.set(String::new());
                        }
                    }
                }
                object_stores_state.set(page_load_state_from_result(result, |view| {
                    view.stores.is_empty().then(|| {
                        view.warnings
                            .first()
                            .map(|warning| warning.message.clone())
                            .unwrap_or_else(|| "No object stores reported.".to_string())
                    })
                }));
            });
            || ()
        });
    }

    {
        let api_base_path = props.api_base_path.clone();
        let browser_state = browser_state.clone();
        let endpoint = (*browser_endpoint).clone();
        let prefix = (*browser_prefix).clone();
        let search = (*browser_search).clone();
        let sort = (*browser_sort).clone();
        use_effect_with(
            (api_base_path, endpoint, prefix, search, sort),
            move |(api_base_path, endpoint, prefix, search, sort)| {
                let endpoint = endpoint.clone();
                if endpoint.trim().is_empty() {
                    browser_state.set(ApiLoadState::empty("Select an ObjectStore."));
                } else {
                    let path = crate::api::object_browser_api_path(
                        api_base_path,
                        &endpoint,
                        prefix,
                        search,
                        sort,
                        true,
                    );
                    browser_state.set(ApiLoadState::Loading);
                    wasm_bindgen_futures::spawn_local(async move {
                        browser_state.set(page_load_state_from_result(
                            crate::api::get_object_browser(&path).await,
                            |view| {
                                (view.folders.is_empty() && view.files.is_empty()).then(|| {
                                    "No folders or objects match this browser view.".to_string()
                                })
                            },
                        ));
                    });
                }
                || ()
            },
        );
    }

    html! {
        <section class="dos-page" data-page="objectstores" data-api-route={api_path}>
            <PageHeader
                eyebrow="Managed stores"
                title="ObjectStores"
                summary="Operational view of store policies, capacity, and service state."
            />
            { render_object_stores_state(
                &*object_stores_state,
                create_state,
                configure_state,
                subobject_state,
                props.api_base_path.clone(),
                browser_state,
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
                props.on_upload_target.clone(),
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_stores_state(
    state: &ApiLoadState<ObjectStoresPageResponse>,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
    browser_state: UseStateHandle<ApiLoadState<ObjectBrowserResponse>>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
    on_upload_target: Callback<String>,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None, create_state, api_base_path) }
                { render_object_stores_state_message(
                    "Loading",
                    "Loading object-store inventory",
                    "The Web console is requesting daemon-backed store registry, policy, capacity, endpoint, and warning state.",
                ) }
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_object_store_inventory(
                view,
                create_state,
                configure_state,
                subobject_state,
                api_base_path,
                browser_state,
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
                on_upload_target,
            )
        }
        ApiLoadState::Empty(message) => html! {
            <div class="dos-store-grid">
                { render_object_store_create_card(None, create_state, api_base_path) }
                { render_object_stores_state_message("Inventory", "No object stores reported yet", message) }
            </div>
        },
        ApiLoadState::PermissionDenied(message) => render_object_stores_state_message(
            "Permission denied",
            "ObjectStore inventory requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => render_object_stores_state_message(
            "Error",
            "Unable to load ObjectStore inventory",
            message,
        ),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_store_inventory(
    view: &ObjectStoresPageResponse,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
    browser_state: UseStateHandle<ApiLoadState<ObjectBrowserResponse>>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
    on_upload_target: Callback<String>,
) -> Html {
    html! {
        <div class="dos-store-grid">
            { render_object_store_create_card(Some(view), create_state, api_base_path.clone()) }
            { render_subobject_create_card(view, subobject_state, api_base_path.clone()) }
            { render_object_store_configure_card(view, configure_state, api_base_path.clone()) }
            { for object_store_card_summaries(view).into_iter().map(|store| render_object_store_card(store, on_upload_target.clone())) }
            { render_object_browser_panel(
                view,
                &*browser_state,
                api_base_path.clone(),
                browser_download_state,
                browser_endpoint,
                browser_prefix,
                browser_search,
                browser_sort,
            ) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct RemoteUploadPageProps {
    pub api_base_path: String,
    pub target_store_id: String,
}
