#[cfg(target_arch = "wasm32")]
use super::*;
#[cfg(target_arch = "wasm32")]
use crate::components::{TaskPane, TaskPaneMode};

#[cfg(target_arch = "wasm32")]
#[function_component(ObjectStoresPage)]
pub fn object_stores_page(props: &ObjectStoresPageProps) -> Html {
    let api_path = WorkspacePage::ObjectStores.api_path(&props.api_base_path);
    let object_stores_state = use_state(|| ApiLoadState::<ObjectStoresPageResponse>::Loading);
    let create_state = use_state(|| ObjectStoreCreateFormState::from_view(None));
    let configure_state = use_state(|| ObjectStoreConfigureFormState::from_view(None));
    let subobject_state = use_state(|| SubObjectFormState::from_view(None));
    let pane_mode = use_state(|| TaskPaneMode::Closed);
    let registry_sort = use_state(ObjectStoreSort::default);
    let create_trigger_ref = use_node_ref();
    let refresh_nonce = use_state(|| 0_u64);
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
        let refresh = *refresh_nonce;
        use_effect_with((api_path.clone(), refresh), move |(path, _)| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::get_object_stores_dashboard(&path).await;
                // An empty inventory is still an actionable workspace: operators
                // must retain the page-level Create ObjectStore affordance.
                object_stores_state.set(page_load_state_from_result(result, |_| None));
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
                summary="Choose a store to inspect its evidence or begin a scoped action."
            />
            { render_object_stores_state(
                &*object_stores_state,
                create_state,
                configure_state,
                subobject_state,
                pane_mode,
                registry_sort,
                create_trigger_ref,
                refresh_nonce,
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
    pane_mode: UseStateHandle<TaskPaneMode>,
    registry_sort: UseStateHandle<ObjectStoreSort>,
    create_trigger_ref: NodeRef,
    refresh_nonce: UseStateHandle<u64>,
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
            <div class="dos-objectstores-registry">
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
                pane_mode,
                registry_sort,
                create_trigger_ref,
                refresh_nonce,
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
            <div class="dos-objectstores-registry">
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
    pane_mode: UseStateHandle<TaskPaneMode>,
    registry_sort: UseStateHandle<ObjectStoreSort>,
    create_trigger_ref: NodeRef,
    refresh_nonce: UseStateHandle<u64>,
    api_base_path: String,
    browser_state: UseStateHandle<ApiLoadState<ObjectBrowserResponse>>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
    on_upload_target: Callback<String>,
) -> Html {
    let create_enabled = view.create_object_store.enabled;
    let open_create = {
        let view = view.clone();
        let create_state = create_state.clone();
        let pane_mode = pane_mode.clone();
        Callback::from(move |_| {
            let mut state = ObjectStoreCreateFormState::from_view(Some(&view));
            state.open = true;
            create_state.set(state);
            pane_mode.set(TaskPaneMode::Create);
        })
    };
    let mut summaries = object_store_card_summaries(view);
    sort_object_store_summaries(&mut summaries, *registry_sort);
    html! {
        <div class="dos-objectstores-registry">
            <div class="dos-objectstores-toolbar" aria-label="ObjectStore registry actions">
                <div>
                    <strong>{ format!("{} {}", summaries.len(), if summaries.len() == 1 { "ObjectStore" } else { "ObjectStores" }) }</strong>
                    <span>{ "Select a store to inspect evidence or begin a scoped action." }</span>
                </div>
                <button
                    ref={create_trigger_ref.clone()}
                    type="button"
                    class="dos-objectstores-primary-action"
                    disabled={!create_enabled}
                    onclick={open_create}
                >
                    { "Create ObjectStore" }
                </button>
            </div>
            { render_object_store_registry_table(
                &summaries,
                &*pane_mode,
                pane_mode.clone(),
                *registry_sort,
                registry_sort,
            ) }
            { render_object_store_task_surface(
                view,
                &summaries,
                pane_mode,
                create_trigger_ref,
                refresh_nonce,
                create_state,
                configure_state,
                subobject_state,
                api_base_path.clone(),
                browser_endpoint.clone(),
                browser_prefix.clone(),
                on_upload_target,
            ) }
            if !browser_endpoint.trim().is_empty() {
                { render_object_browser_panel(
                    view,
                    &*browser_state,
                    api_base_path,
                    browser_download_state,
                    browser_endpoint,
                    browser_prefix,
                    browser_search,
                    browser_sort,
                ) }
            }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_object_store_registry_table(
    stores: &[ObjectStoreCardSummary],
    pane_mode: &TaskPaneMode,
    set_pane_mode: UseStateHandle<TaskPaneMode>,
    sort: ObjectStoreSort,
    set_sort: UseStateHandle<ObjectStoreSort>,
) -> Html {
    html! {
        <div class="dos-objectstores-table-wrap">
            <table class="dos-objectstores-table">
                <thead>
                    <tr>
                        { object_store_sort_header("ObjectStore", ObjectStoreSortColumn::ObjectStore, sort, set_sort.clone()) }
                        <th scope="col">{ "Purpose" }</th>
                        <th scope="col">{ "State" }</th>
                        { object_store_sort_header("Capacity", ObjectStoreSortColumn::Capacity, sort, set_sort.clone()) }
                        { object_store_sort_header("Objects", ObjectStoreSortColumn::Objects, sort, set_sort.clone()) }
                        { object_store_sort_header("Last activity", ObjectStoreSortColumn::LastActivity, sort, set_sort) }
                        <th scope="col"><span class="dos-visually-hidden">{ "Open" }</span></th>
                    </tr>
                </thead>
                <tbody>
                    if stores.is_empty() {
                        <tr class="dos-objectstores-empty">
                            <td colspan="7">
                                <strong>{ "No ObjectStores yet" }</strong>
                                <span>{ "Create one when you are ready; DASObjectStore will review the plan before making changes." }</span>
                            </td>
                        </tr>
                    }
                    { for stores.iter().map(|store| {
                        let store_id = store.id.clone();
                        let open = matches!(pane_mode, TaskPaneMode::Edit(context) if context == &format!("inspect:{store_id}"));
                        let open_store = {
                            let set_pane_mode = set_pane_mode.clone();
                            let store_id = store_id.clone();
                            Callback::from(move |_| set_pane_mode.set(TaskPaneMode::Edit(format!("inspect:{store_id}"))))
                        };
                        html! {
                            <tr data-selected={open.to_string()} data-store-id={store.id.clone()}>
                                <th scope="row">
                                    <button
                                        type="button"
                                        class="dos-objectstores-name"
                                        aria-expanded={open.to_string()}
                                        aria-controls="dos-task-pane-title"
                                        onclick={open_store.clone()}
                                    >
                                        <strong>{ store.name.clone() }</strong>
                                        <span>{ store.id.clone() }</span>
                                    </button>
                                </th>
                                <td>{ humanize_store_value(&store.label) }</td>
                                <td><span class="dos-objectstores-state" data-state={store.health.clone()}>{ humanize_store_value(&store.health) }</span></td>
                                <td>{ compact_capacity(&store.capacity, &store.capacity_status) }</td>
                                <td>{ store.objects.clone() }</td>
                                <td>{ store.last_ingested.clone() }</td>
                                <td><button type="button" class="dos-objectstores-open" onclick={open_store}>{ "Open" }</button></td>
                            </tr>
                        }
                    }) }
                </tbody>
            </table>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn object_store_sort_header(
    label: &'static str,
    column: ObjectStoreSortColumn,
    sort: ObjectStoreSort,
    set_sort: UseStateHandle<ObjectStoreSort>,
) -> Html {
    let active = sort.column == column;
    let aria_sort = active.then_some(if sort.descending {
        "descending"
    } else {
        "ascending"
    });
    let next = sort.select(column);
    let direction = if next.descending {
        "descending"
    } else {
        "ascending"
    };
    html! {
        <th scope="col" aria-sort={aria_sort}>
            <button
                type="button"
                class="dos-objectstores-sort"
                aria-label={format!("Sort by {label}, {direction}")}
                title={format!("Sort by {label}, {direction}")}
                onclick={Callback::from(move |_| set_sort.set(next))}
            >
                <span>{ label }</span>
                if active {
                    <span aria-hidden="true" class="dos-objectstores-sort-indicator">
                        { if sort.descending { "↓" } else { "↑" } }
                    </span>
                }
            </button>
        </th>
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn render_object_store_task_surface(
    view: &ObjectStoresPageResponse,
    stores: &[ObjectStoreCardSummary],
    pane_mode: UseStateHandle<TaskPaneMode>,
    create_trigger_ref: NodeRef,
    refresh_nonce: UseStateHandle<u64>,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    subobject_state: UseStateHandle<SubObjectFormState>,
    api_base_path: String,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    on_upload_target: Callback<String>,
) -> Html {
    let close = {
        let pane_mode = pane_mode.clone();
        let create_state = create_state.clone();
        let configure_state = configure_state.clone();
        let subobject_state = subobject_state.clone();
        Callback::<()>::from(move |_| {
            let mut create = (*create_state).clone();
            create.open = false;
            create_state.set(create);
            let mut configure = (*configure_state).clone();
            configure.open = false;
            configure_state.set(configure);
            let mut subobject = (*subobject_state).clone();
            subobject.open = false;
            subobject_state.set(subobject);
            pane_mode.set(TaskPaneMode::Closed);
        })
    };

    match &*pane_mode {
        TaskPaneMode::Closed | TaskPaneMode::Review => Html::default(),
        TaskPaneMode::Create => html! {
            <TaskPane mode={TaskPaneMode::Create} title="Create ObjectStore" selected_context={Some("Daemon-planned creation".to_string())} return_focus_to={Some(create_trigger_ref)} on_close={close}>
                { render_object_store_create_card(Some(view), create_state, api_base_path, {
                    let refresh_nonce = refresh_nonce.clone();
                    Callback::from(move |_: String| {
                        refresh_nonce.set((*refresh_nonce).wrapping_add(1));
                    })
                }) }
            </TaskPane>
        },
        TaskPaneMode::Edit(context) if context.starts_with("configure:") => {
            let store_id = context.trim_start_matches("configure:").to_string();
            html! {
                <TaskPane mode={TaskPaneMode::Edit(store_id.clone())} title="Edit ObjectStore policy" selected_context={Some(store_id)} return_focus_to={Some(create_trigger_ref)} on_close={close}>
                    { render_object_store_configure_card(view, configure_state, api_base_path) }
                </TaskPane>
            }
        }
        TaskPaneMode::Edit(context) if context.starts_with("subobject:") => {
            let store_id = context.trim_start_matches("subobject:").to_string();
            html! {
                <TaskPane mode={TaskPaneMode::Edit(store_id.clone())} title="Create SubObject" selected_context={Some(format!("Parent ObjectStore: {store_id}"))} return_focus_to={Some(create_trigger_ref)} on_close={close}>
                    { render_subobject_create_card(view, subobject_state, api_base_path) }
                </TaskPane>
            }
        }
        TaskPaneMode::Edit(context) => {
            let store_id = context.trim_start_matches("inspect:");
            let Some(store) = stores.iter().find(|store| store.id == store_id) else {
                return Html::default();
            };
            let upload = {
                let on_upload_target = on_upload_target.clone();
                let store_id = store.id.clone();
                Callback::from(move |_| on_upload_target.emit(store_id.clone()))
            };
            let browse = {
                let browser_endpoint = browser_endpoint.clone();
                let browser_prefix = browser_prefix.clone();
                let pane_mode = pane_mode.clone();
                let store_id = store.id.clone();
                Callback::from(move |_| {
                    browser_endpoint.set(store_id.clone());
                    browser_prefix.set(String::new());
                    pane_mode.set(TaskPaneMode::Closed);
                })
            };
            let configure = {
                let view = view.clone();
                let configure_state = configure_state.clone();
                let pane_mode = pane_mode.clone();
                let store_id = store.id.clone();
                Callback::from(move |_| {
                    let mut state = ObjectStoreConfigureFormState::from_view(Some(&view));
                    if let Some(store) = view.stores.iter().find(|store| store.store_id == store_id)
                    {
                        state.apply_store(store);
                    }
                    state.open = true;
                    configure_state.set(state);
                    pane_mode.set(TaskPaneMode::Edit(format!("configure:{store_id}")));
                })
            };
            let create_subobject = {
                let view = view.clone();
                let subobject_state = subobject_state.clone();
                let pane_mode = pane_mode.clone();
                let store_id = store.id.clone();
                Callback::from(move |_| {
                    let mut state = SubObjectFormState::from_view(Some(&view));
                    state.parent_kind = "store".to_string();
                    state.parent_store_id = store_id.clone();
                    state.open = true;
                    subobject_state.set(state);
                    pane_mode.set(TaskPaneMode::Edit(format!("subobject:{store_id}")));
                })
            };
            html! {
                <TaskPane mode={TaskPaneMode::Edit(store.id.clone())} title={format!("ObjectStore: {}", store.name)} selected_context={Some(store.id.clone())} return_focus_to={Some(create_trigger_ref)} on_close={close}>
                    <div class="dos-objectstores-detail">
                        <section>
                            <h3>{ "Overview" }</h3>
                            <dl>
                                <dt>{ "State" }</dt><dd>{ humanize_store_value(&store.health) }</dd>
                                <dt>{ "Purpose" }</dt><dd>{ humanize_store_value(&store.label) }</dd>
                                <dt>{ "Object type" }</dt><dd>{ humanize_store_value(&store.object_type) }</dd>
                                <dt>{ "Objects" }</dt><dd>{ store.objects.clone() }</dd>
                            </dl>
                        </section>
                        <section>
                            <h3>{ "Storage" }</h3>
                            <dl>
                                <dt>{ "Observed capacity" }</dt><dd>{ compact_capacity(&store.capacity, &store.capacity_status) }</dd>
                                <dt>{ "Policy" }</dt><dd>{ humanize_store_value(&store.policy) }</dd>
                                <dt>{ "Live evidence" }</dt><dd>{ humanize_store_value(&store.capacity_status) }</dd>
                            </dl>
                        </section>
                        <section>
                            <h3>{ "Access & service" }</h3>
                            <dl>
                                <dt>{ "Access" }</dt><dd>{ humanize_store_value(&store.access) }</dd>
                                <dt>{ "Writer group" }</dt><dd>{ store.writer_group.clone() }</dd>
                                <dt>{ "Writer readiness" }</dt><dd>{ store.writer_policy.clone() }</dd>
                                <dt>{ "Endpoint" }</dt><dd>{ humanize_store_value(&store.endpoint) }</dd>
                            </dl>
                        </section>
                        <section>
                            <h3>{ "Activity & warnings" }</h3>
                            <dl>
                                <dt>{ "Last ingest" }</dt><dd>{ store.last_ingested.clone() }</dd>
                                <dt>{ "Warnings" }</dt><dd>{ format!("{} warning(s)", store.warning_count) }</dd>
                            </dl>
                        </section>
                        <div class="dos-objectstores-pane-actions">
                            if store.upload_allowed {
                                <button type="button" class="dos-objectstores-primary-action" onclick={upload}>{ "Upload" }</button>
                            }
                            <button type="button" onclick={browse}>{ "Browse objects" }</button>
                            <button type="button" disabled={!view.create_object_store.enabled} onclick={create_subobject}>{ "Create SubObject" }</button>
                            <button type="button" disabled={!view.create_object_store.enabled} onclick={configure}>{ "Edit policy" }</button>
                        </div>
                    </div>
                </TaskPane>
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn humanize_store_value(value: &str) -> String {
    value.replace('_', " ").replace(';', " ·")
}

#[cfg(target_arch = "wasm32")]
fn compact_capacity(capacity: &str, live_status: &str) -> String {
    if live_status.contains("unavailable") || live_status.contains("not connected") {
        let catalogued = capacity
            .split(';')
            .next()
            .unwrap_or(capacity)
            .replace(" used", " catalogued");
        format!("{catalogued} · live capacity unavailable")
    } else {
        capacity.to_string()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct RemoteUploadPageProps {
    pub api_base_path: String,
    pub target_store_id: String,
}
