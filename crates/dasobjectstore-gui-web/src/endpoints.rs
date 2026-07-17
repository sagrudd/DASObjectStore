pub const ENDPOINTS_WORKSPACE_ROUTE: &str = "workspaces/endpoints";
pub const ENDPOINT_INVENTORY_UPSERT_ROUTE: &str = "workspaces/endpoints/upsert";
pub const ENDPOINT_RECORD_CONFIRMATION: &str = "record endpoint inventory";

pub fn endpoints_workspace_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENDPOINTS_WORKSPACE_ROUTE
    )
}

pub fn endpoint_inventory_upsert_action_api_path(api_base_path: &str) -> String {
    format!(
        "{}/{}",
        api_base_path.trim_end_matches('/'),
        ENDPOINT_INVENTORY_UPSERT_ROUTE
    )
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointCardSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub validation: String,
    pub object_service_url: String,
    pub manager_product_id: String,
    pub bindings: String,
    pub warning_count: usize,
    pub checked_at: String,
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_card_summaries(
    view: &crate::api::EndpointsWorkspaceResponse,
) -> Vec<EndpointCardSummary> {
    view.inventory
        .endpoints
        .iter()
        .map(|endpoint| EndpointCardSummary {
            id: endpoint.endpoint_id.clone(),
            name: endpoint.display_name.clone(),
            kind: endpoint.kind.clone(),
            validation: endpoint.validation.state.clone(),
            object_service_url: endpoint.object_service_url.clone(),
            manager_product_id: endpoint.manager_product_id.clone(),
            bindings: format!("{} binding(s)", endpoint.active_bindings.len()),
            warning_count: endpoint.warnings.len(),
            checked_at: endpoint
                .validation
                .checked_at_utc
                .as_deref()
                .unwrap_or("not checked")
                .to_string(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_inventory_summary_cards(
    view: &crate::api::EndpointsWorkspaceResponse,
) -> Vec<(String, String, String)> {
    vec![
        (
            "Endpoints".to_string(),
            view.inventory.endpoint_count.to_string(),
            "Registered storage endpoints".to_string(),
        ),
        (
            "Bindings".to_string(),
            view.inventory.binding_count.to_string(),
            "Active ObjectStore/governance-domain bindings".to_string(),
        ),
        (
            "Degraded".to_string(),
            view.inventory.degraded_endpoint_count.to_string(),
            "Endpoints requiring operator review".to_string(),
        ),
        (
            "Warnings".to_string(),
            view.inventory.warnings.len().to_string(),
            "Endpoint and binding warnings".to_string(),
        ),
    ]
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_fields_ready(
    endpoint_id: &str,
    display_name: &str,
    object_service_url: &str,
) -> bool {
    !endpoint_id.trim().is_empty()
        && !display_name.trim().is_empty()
        && !object_service_url.trim().is_empty()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_kind_label(kind: &str) -> &'static str {
    match kind {
        "dasobjectstore_das" => "DASObjectStore direct storage",
        "dasobjectstore_nfs" => "NAS / NFS",
        "s3_compatible" => "S3-compatible service",
        _ => "Storage service",
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_validation_label(state: &str) -> &'static str {
    match state {
        "validated" => "Connected",
        "pending_validation" => "Needs testing",
        "draft" => "Draft",
        "degraded" => "Attention needed",
        "rejected" => "Unavailable",
        _ => "Status unknown",
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_binding_fields_ready(
    binding_enabled: bool,
    _binding_id: &str,
    governance_domain: &str,
    store_id: &str,
) -> bool {
    !binding_enabled || (!governance_domain.trim().is_empty() && !store_id.trim().is_empty())
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_confirmation_matches(dry_run: bool, confirmation_phrase: &str) -> bool {
    dry_run || confirmation_phrase.trim() == ENDPOINT_RECORD_CONFIRMATION
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn endpoint_upsert_review_from_values(
    endpoint_id: &str,
    kind: &str,
    validation_state: &str,
    object_service_url: &str,
    binding_enabled: bool,
    store_id: &str,
) -> String {
    format!(
        "{} · {} · validation {} · {} · {}",
        if endpoint_id.trim().is_empty() {
            "endpoint pending"
        } else {
            endpoint_id.trim()
        },
        kind,
        validation_state,
        object_service_url.trim(),
        if binding_enabled {
            format!("binding to {}", store_id.trim())
        } else {
            "no active binding".to_string()
        }
    )
}

#[cfg(target_arch = "wasm32")]
use crate::api::{
    EndpointBindingUpsertRequest, EndpointInventoryUpsertRequest, EndpointInventoryUpsertResponse,
    EndpointValidationUpsertRequest, EndpointsWorkspaceResponse,
};
#[cfg(target_arch = "wasm32")]
use crate::components::{TaskPane, TaskPaneMode};
#[cfg(target_arch = "wasm32")]
use crate::workspace::ApiLoadState;
#[cfg(target_arch = "wasm32")]
use web_sys::{HtmlInputElement, HtmlSelectElement};
#[cfg(target_arch = "wasm32")]
use yew::prelude::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct EndpointsWorkspaceProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct EndpointUpsertFormState {
    endpoint_id: String,
    display_name: String,
    kind: String,
    object_service_url: String,
    validation_state: String,
    checked_at_utc: String,
    validation_message: String,
    manager_product_id: String,
    binding_enabled: bool,
    binding_id: String,
    governance_domain: String,
    store_id: String,
    binding_readiness: String,
    dry_run: bool,
    confirmation_phrase: String,
    submitting: bool,
    submitted: Option<EndpointInventoryUpsertResponse>,
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
impl Default for EndpointUpsertFormState {
    fn default() -> Self {
        Self {
            endpoint_id: String::new(),
            display_name: String::new(),
            kind: "dasobjectstore_nfs".to_string(),
            object_service_url: String::new(),
            validation_state: "pending_validation".to_string(),
            checked_at_utc: String::new(),
            validation_message: String::new(),
            manager_product_id: "dasobjectstore".to_string(),
            binding_enabled: false,
            binding_id: String::new(),
            governance_domain: "local".to_string(),
            store_id: String::new(),
            binding_readiness: "ready".to_string(),
            dry_run: true,
            confirmation_phrase: String::new(),
            submitting: false,
            submitted: None,
            error: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl EndpointUpsertFormState {
    fn clear_result(&mut self) {
        self.submitted = None;
        self.error = None;
    }
}

#[cfg(target_arch = "wasm32")]
#[function_component(EndpointsWorkspace)]
pub fn endpoints_workspace(props: &EndpointsWorkspaceProps) -> Html {
    let api_path = endpoints_workspace_api_path(&props.api_base_path);
    let endpoints_state = use_state(|| ApiLoadState::<EndpointsWorkspaceResponse>::Loading);
    let form_state = use_state(EndpointUpsertFormState::default);
    let stores_state = use_state(Vec::<crate::api::ObjectStoreCardResponse>::new);
    let pane_mode = use_state(|| TaskPaneMode::Closed);
    let add_endpoint_trigger_ref = use_node_ref();

    {
        let api_path = api_path.clone();
        let endpoints_state = endpoints_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let next = match crate::api::get_endpoints_workspace(&path).await {
                    Ok(view) => ApiLoadState::success(view),
                    Err(error) if error.is_permission_denied() => {
                        ApiLoadState::permission_denied(error.message)
                    }
                    Err(error) => ApiLoadState::transport_error(error.message),
                };
                endpoints_state.set(next);
            });
            || ()
        });
    }

    {
        let path = crate::workspace::objectstores_workspace_api_path(&props.api_base_path);
        let stores_state = stores_state.clone();
        use_effect_with(path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(view) = crate::api::get_object_stores_dashboard(&path).await {
                    stores_state.set(view.stores);
                }
            });
            || ()
        });
    }

    html! {
        <section class="dos-page dos-endpoints" data-page="endpoints" data-api-route={api_path}>
            <header class="dos-page-header">
                <p>{ "Storage connections" }</p>
                <h1>{ "Connections" }</h1>
                <span>{ "The services through which ObjectStores are reached by applications and external storage systems." }</span>
            </header>
            { render_endpoints_state(&*endpoints_state, endpoints_state.clone(), form_state, pane_mode, add_endpoint_trigger_ref, props.api_base_path.clone(), (*stores_state).clone()) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoints_state(
    state: &ApiLoadState<EndpointsWorkspaceResponse>,
    endpoints_state: UseStateHandle<ApiLoadState<EndpointsWorkspaceResponse>>,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    add_endpoint_trigger_ref: NodeRef,
    api_base_path: String,
    stores: Vec<crate::api::ObjectStoreCardResponse>,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <div class="dos-store-grid">
                { render_endpoint_toolbar(pane_mode, form_state, add_endpoint_trigger_ref, true) }
                { render_endpoint_state_message(
                    "Loading",
                    "Loading endpoint inventory",
                    "The Web console is requesting the daemon-backed endpoint registry snapshot.",
                ) }
            </div>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_endpoint_inventory(
                view,
                endpoints_state,
                form_state,
                pane_mode,
                add_endpoint_trigger_ref,
                api_base_path,
                stores,
            )
        }
        ApiLoadState::Empty(message) => html! {
            <div class="dos-store-grid">
                { render_endpoint_toolbar(pane_mode, form_state, add_endpoint_trigger_ref, true) }
                { render_endpoint_state_message("Inventory", "No endpoints reported yet", message) }
            </div>
        },
        ApiLoadState::PermissionDenied(message) => html! {
            <div class="dos-store-grid">
                { render_endpoint_toolbar(pane_mode, form_state, add_endpoint_trigger_ref, false) }
                { render_endpoint_state_message("Permission denied", "Endpoint inventory requires an authenticated session", message) }
            </div>
        },
        ApiLoadState::TransportError(message) => html! {
            <div class="dos-store-grid">
                { render_endpoint_toolbar(pane_mode, form_state, add_endpoint_trigger_ref, false) }
                { render_endpoint_state_message("Error", "Unable to load endpoint inventory", message) }
            </div>
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_inventory(
    view: &EndpointsWorkspaceResponse,
    endpoints_state: UseStateHandle<ApiLoadState<EndpointsWorkspaceResponse>>,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    add_endpoint_trigger_ref: NodeRef,
    api_base_path: String,
    stores: Vec<crate::api::ObjectStoreCardResponse>,
) -> Html {
    html! {
        <>
            <div class="dos-metric-grid">
                { for endpoint_inventory_summary_cards(view).into_iter().map(render_endpoint_metric_card) }
            </div>
            { render_endpoint_toolbar(pane_mode.clone(), form_state.clone(), add_endpoint_trigger_ref.clone(), true) }
            <div class="dos-card dos-wide-card dos-endpoint-inventory" data-section="endpoint-inventory">
                <div class="dos-table-wrap">
                    <table class="dos-table dos-dense-table dos-endpoints-table">
                        <thead><tr><th>{ "Connection" }</th><th>{ "Type" }</th><th>{ "Used by" }</th><th>{ "Health" }</th><th>{ "Last checked" }</th><th><span class="dos-visually-hidden">{ "Action" }</span></th></tr></thead>
                        <tbody>{ for view.inventory.endpoints.iter().map(|item| {
                            let endpoint = item.clone();
                            let endpoint_for_form = endpoint.clone();
                            let form_state = form_state.clone();
                            let pane_mode = pane_mode.clone();
                            let inspect = Callback::from(move |_| {
                                form_state.set(endpoint_form_state_from_item(&endpoint_for_form));
                                pane_mode.set(TaskPaneMode::Review);
                            });
                            let used_by = if endpoint.active_bindings.is_empty() { "Not attached".to_string() } else { endpoint.active_bindings.iter().map(|binding| binding.store_id.as_str()).collect::<Vec<_>>().join(", ") };
                            html! { <tr data-endpoint-id={endpoint.endpoint_id.clone()}><td><strong>{ endpoint.display_name.clone() }</strong><small>{ endpoint.object_service_url.clone() }</small></td><td>{ endpoint_kind_label(&endpoint.kind) }</td><td>{ used_by }</td><td><span class={classes!("dos-status-pill", format!("is-{}", endpoint.validation.state))}>{ endpoint_validation_label(&endpoint.validation.state) }</span></td><td>{ endpoint.validation.checked_at_utc.clone().unwrap_or_else(|| "Not yet checked".to_string()) }</td><td><button type="button" class="dos-secondary-action" onclick={inspect} aria-label={format!("Open details for {}", endpoint.display_name)}>{ "Open" }</button></td></tr> }
                        }) }</tbody>
                    </table>
                </div>
                if view.inventory.endpoints.is_empty() {
                    { render_endpoint_state_message(
                        "Inventory",
                        "No endpoint records yet",
                        "Create a draft or validated endpoint record to make it visible to Activity and Mneion binding workflows.",
                    ) }
                }
                if !view.inventory.warnings.is_empty() {
                    <section class="dos-card dos-wide-card" data-state="warning">
                        <span class="dos-card-label">{ "Endpoint warnings" }</span>
                        <h2>{ format!("{} warning(s)", view.inventory.warnings.len()) }</h2>
                        { for view.inventory.warnings.iter().take(4).map(|warning| html! {
                            <p>{ format!("{} · {} · {}", warning.severity, warning.code, warning.message) }</p>
                        }) }
                    </section>
                }
            </div>
            { render_endpoint_task_pane(view, endpoints_state, form_state, pane_mode, add_endpoint_trigger_ref, api_base_path, stores) }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_toolbar(
    pane_mode: UseStateHandle<TaskPaneMode>,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    add_endpoint_trigger_ref: NodeRef,
    enabled: bool,
) -> Html {
    let open_add = Callback::from(move |_| {
        form_state.set(EndpointUpsertFormState::default());
        pane_mode.set(TaskPaneMode::Create);
    });
    html! {
        <section class="dos-card dos-wide-card dos-endpoints-toolbar" data-section="endpoints-toolbar">
            <div><span class="dos-card-label">{ "Connection inventory" }</span><h2>{ "Available connections" }</h2><p>{ "See how ObjectStores are exposed, whether each connection is trusted, and which stores use it." }</p></div>
            <button type="button" class="dos-auth-submit" ref={add_endpoint_trigger_ref} onclick={open_add} disabled={!enabled}>{ if enabled { "Add connection" } else { "Connection actions unavailable" } }</button>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn endpoint_form_state_from_item(
    item: &crate::api::EndpointInventoryItemResponse,
) -> EndpointUpsertFormState {
    let binding = item.active_bindings.first();
    EndpointUpsertFormState {
        endpoint_id: item.endpoint_id.clone(),
        display_name: item.display_name.clone(),
        kind: item.kind.clone(),
        object_service_url: item.object_service_url.clone(),
        validation_state: item.validation.state.clone(),
        checked_at_utc: item.validation.checked_at_utc.clone().unwrap_or_default(),
        validation_message: item.validation.message.clone().unwrap_or_default(),
        manager_product_id: item.manager_product_id.clone(),
        binding_enabled: binding.is_some(),
        binding_id: binding
            .map(|binding| binding.binding_id.clone())
            .unwrap_or_default(),
        governance_domain: binding
            .map(|binding| binding.governance_domain.clone())
            .unwrap_or_else(|| "local".to_string()),
        store_id: binding
            .map(|binding| binding.store_id.clone())
            .unwrap_or_default(),
        binding_readiness: binding
            .map(|binding| binding.readiness.clone())
            .unwrap_or_else(|| "ready".to_string()),
        dry_run: true,
        confirmation_phrase: String::new(),
        submitting: false,
        submitted: None,
        error: None,
    }
}

#[cfg(target_arch = "wasm32")]
fn refresh_endpoints_workspace(
    api_base_path: String,
    endpoints_state: UseStateHandle<ApiLoadState<EndpointsWorkspaceResponse>>,
) {
    let path = endpoints_workspace_api_path(&api_base_path);
    wasm_bindgen_futures::spawn_local(async move {
        endpoints_state.set(match crate::api::get_endpoints_workspace(&path).await {
            Ok(view) => ApiLoadState::success(view),
            Err(error) if error.is_permission_denied() => {
                ApiLoadState::permission_denied(error.message)
            }
            Err(error) => ApiLoadState::transport_error(error.message),
        });
    });
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_task_pane(
    _view: &EndpointsWorkspaceResponse,
    endpoints_state: UseStateHandle<ApiLoadState<EndpointsWorkspaceResponse>>,
    form_state: UseStateHandle<EndpointUpsertFormState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    add_endpoint_trigger_ref: NodeRef,
    api_base_path: String,
    stores: Vec<crate::api::ObjectStoreCardResponse>,
) -> Html {
    let mode = (*pane_mode).clone();
    if matches!(mode, TaskPaneMode::Closed) {
        return Html::default();
    }
    let on_close = {
        let pane_mode = pane_mode.clone();
        Callback::<()>::from(move |_| pane_mode.set(TaskPaneMode::Closed))
    };
    html! {
        <TaskPane mode={mode.clone()} title={if matches!(mode, TaskPaneMode::Create) { "Add connection".to_string() } else if matches!(mode, TaskPaneMode::Review) { form_state.display_name.clone() } else { "Edit connection".to_string() }} selected_context={Some(form_state.endpoint_id.clone())} return_focus_to={Some(add_endpoint_trigger_ref)} on_close={on_close}>
            if matches!(mode, TaskPaneMode::Review) {
                { render_endpoint_detail(form_state, pane_mode) }
            } else {
                { render_endpoint_upsert_card(form_state, api_base_path, endpoints_state, pane_mode, stores) }
            }
        </TaskPane>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_detail(
    form_state: UseStateHandle<EndpointUpsertFormState>,
    pane_mode: UseStateHandle<TaskPaneMode>,
) -> Html {
    let state = (*form_state).clone();
    let edit = {
        let pane_mode = pane_mode.clone();
        let endpoint_id = state.endpoint_id.clone();
        Callback::from(move |_| pane_mode.set(TaskPaneMode::Edit(endpoint_id.clone())))
    };
    html! {
        <div class="dos-connection-detail">
            <section class="dos-task-pane__section">
                <span class="dos-card-label">{ "Connection" }</span>
                <div class="dos-connection-detail__hero">
                    <div><h2>{ state.display_name.clone() }</h2><p>{ endpoint_kind_label(&state.kind) }</p></div>
                    <span class={classes!("dos-status-pill", format!("is-{}", state.validation_state))}>{ endpoint_validation_label(&state.validation_state) }</span>
                </div>
                <dl class="dos-connection-facts">
                    <div><dt>{ "Service address" }</dt><dd>{ state.object_service_url.clone() }</dd></div>
                    <div><dt>{ "Used by" }</dt><dd>{ if state.binding_enabled { state.store_id.clone() } else { "No ObjectStore attached".to_string() } }</dd></div>
                    <div><dt>{ "Last checked" }</dt><dd>{ if state.checked_at_utc.is_empty() { "Not yet checked".to_string() } else { state.checked_at_utc.clone() } }</dd></div>
                    if !state.validation_message.is_empty() { <div><dt>{ "Evidence" }</dt><dd>{ state.validation_message.clone() }</dd></div> }
                </dl>
            </section>
            <section class="dos-task-pane__section dos-technical-details">
                <details><summary>{ "Technical details" }</summary><dl class="dos-connection-facts"><div><dt>{ "Endpoint ID" }</dt><dd>{ state.endpoint_id.clone() }</dd></div><div><dt>{ "Manager" }</dt><dd>{ state.manager_product_id.clone() }</dd></div>{ if state.binding_enabled { html! { <><div><dt>{ "Binding ID" }</dt><dd>{ state.binding_id.clone() }</dd></div><div><dt>{ "Governance domain" }</dt><dd>{ state.governance_domain.clone() }</dd></div></> } } else { Html::default() } }</dl></details>
            </section>
            <button type="button" class="dos-auth-submit" onclick={edit}>{ "Edit connection" }</button>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_metric_card(metric: (String, String, String)) -> Html {
    html! {
        <section class="dos-card dos-metric-card">
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.0 }</span>
                <span class="dos-status-pill">{ "Inventory" }</span>
            </div>
            <strong>{ metric.1 }</strong>
            <p>{ metric.2 }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_upsert_card(
    form_state: UseStateHandle<EndpointUpsertFormState>,
    api_base_path: String,
    endpoints_state: UseStateHandle<ApiLoadState<EndpointsWorkspaceResponse>>,
    pane_mode: UseStateHandle<TaskPaneMode>,
    stores: Vec<crate::api::ObjectStoreCardResponse>,
) -> Html {
    let state = (*form_state).clone();
    let fields_ready = endpoint_upsert_fields_ready(
        &state.endpoint_id,
        &state.display_name,
        &state.object_service_url,
    );
    let binding_ready = endpoint_binding_fields_ready(
        state.binding_enabled,
        &state.binding_id,
        &state.governance_domain,
        &state.store_id,
    );
    let confirmation_ready =
        endpoint_upsert_confirmation_matches(state.dry_run, &state.confirmation_phrase);
    let can_submit = fields_ready && binding_ready && confirmation_ready && !state.submitting;
    let submit = {
        let form_state = form_state.clone();
        let api_base_path = api_base_path.clone();
        let endpoints_state = endpoints_state.clone();
        let pane_mode = pane_mode.clone();
        Callback::from(move |_| {
            let mut pending = (*form_state).clone();
            pending.submitting = true;
            pending.submitted = None;
            pending.error = None;
            form_state.set(pending.clone());

            let form_state = form_state.clone();
            let api_base_path = api_base_path.clone();
            let endpoints_state = endpoints_state.clone();
            let pane_mode = pane_mode.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = endpoint_upsert_request_from_state(&pending);
                let result =
                    crate::api::submit_endpoint_inventory_upsert(&api_base_path, &request).await;
                let mut next = (*form_state).clone();
                next.submitting = false;
                match result {
                    Ok(response) => {
                        next.submitted = Some(response);
                        next.error = None;
                        form_state.set(next);
                        pane_mode.set(TaskPaneMode::Closed);
                        refresh_endpoints_workspace(api_base_path, endpoints_state);
                        return;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                form_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-endpoint-upsert" data-action="endpoint_inventory_upsert">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ if matches!(*pane_mode, TaskPaneMode::Create) { "Add a storage connection" } else { "Update connection" } }</h2>
            <p>{ "Describe where the storage service is reached. New connections remain untrusted until the daemon records validation evidence." }</p>
            <span class="dos-status-pill">{ if state.dry_run { "Dry run" } else { "Live update" } }</span>
            <div class="dos-objectstore-form">
                <section class="dos-task-pane__section" data-section="endpoint-identity">
                <span class="dos-card-label">{ "Connection details" }</span>
                <div class="dos-form-grid">
                    { endpoint_text_field("Connection ID", state.endpoint_id.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.endpoint_id = value;
                            });
                        })
                    }) }
                    { endpoint_text_field("Display name", state.display_name.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.display_name = value;
                            });
                        })
                    }) }
                    <label class="dos-form-field">
                        <span>{ "What are you connecting?" }</span>
                        <select onchange={{
                            let form_state = form_state.clone();
                            Callback::from(move |event: Event| {
                                update_endpoint_form_from_select(&form_state, event, |state, value| {
                                    state.kind = value;
                                });
                            })
                        }} value={state.kind.clone()}>
                            <option value="dasobjectstore_das">{ "DASObjectStore DAS" }</option>
                            <option value="dasobjectstore_nfs">{ "DASObjectStore NAS/NFS" }</option>
                            <option value="s3_compatible">{ "S3 compatible" }</option>
                        </select>
                    </label>
                    { endpoint_text_field(if state.kind == "s3_compatible" { "S3 service URL" } else if state.kind == "dasobjectstore_nfs" { "NAS / NFS gateway URL" } else { "DASObjectStore service URL" }, state.object_service_url.clone(), {
                        let form_state = form_state.clone();
                        Callback::from(move |event: InputEvent| {
                            update_endpoint_form_from_input(&form_state, event, |state, value| {
                                state.object_service_url = value;
                            });
                        })
                    }) }
                    <div class="dos-form-field dos-readonly-field"><span>{ "Connection health" }</span><strong>{ endpoint_validation_label(&state.validation_state) }</strong><small>{ "Validation evidence is maintained by the daemon, not declared in this form." }</small></div>
                </div>
                </section>
                <section class="dos-risk-review dos-task-pane__section" data-section="endpoint-binding">
                    <span class="dos-card-label">{ "Active binding" }</span>
                    <label>
                        <input
                            type="checkbox"
                            checked={state.binding_enabled}
                            onchange={{
                                let form_state = form_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*form_state).clone();
                                    next.binding_enabled = input.checked();
                                    next.clear_result();
                                    form_state.set(next);
                                })
                            }}
                        />
                        <span>{ "Make an ObjectStore available through this connection." }</span>
                    </label>
                    if state.binding_enabled {
                        <div class="dos-form-grid">
                            <label class="dos-form-field"><span>{ "ObjectStore" }</span><select onchange={{
                                    let form_state = form_state.clone();
                                    Callback::from(move |event: Event| {
                                        update_endpoint_form_from_select(&form_state, event, |state, value| {
                                            state.store_id = value.clone();
                                            state.binding_id = String::new();
                                        });
                                    })
                                }} value={state.store_id.clone()}><option value="">{ "Select an ObjectStore" }</option>{ for stores.iter().map(|store| html! { <option value={store.store_id.clone()}>{ store.display_name.clone() }</option> }) }</select></label>
                            <p class="dos-form-guidance">{ "DASObjectStore generates the binding identity and uses the local governance domain. These technical values remain available in connection details." }</p>
                        </div>
                    }
                </section>
                <section class="dos-plan-result dos-task-pane__section" data-section="endpoint-review">
                    <span class="dos-card-label">{ "Submission review" }</span>
                    <p>{ endpoint_upsert_review_from_values(
                        &state.endpoint_id,
                        &state.kind,
                        &state.validation_state,
                        &state.object_service_url,
                        state.binding_enabled,
                        &state.store_id,
                    ) }</p>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input
                                type="checkbox"
                                checked={state.dry_run}
                                onchange={{
                                    let form_state = form_state.clone();
                                    Callback::from(move |event: Event| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*form_state).clone();
                                        next.dry_run = input.checked();
                                        next.clear_result();
                                        form_state.set(next);
                                    })
                                }}
                            />
                            <span>{ "Dry run only" }</span>
                        </label>
                    </div>
                    if !state.dry_run {
                    <label class="dos-form-field">
                        <span>{ "Confirmation phrase" }</span>
                        <input
                            placeholder={ENDPOINT_RECORD_CONFIRMATION}
                            value={state.confirmation_phrase.clone()}
                            oninput={{
                                let form_state = form_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    update_endpoint_form_from_input(&form_state, event, |state, value| {
                                        state.confirmation_phrase = value;
                                    });
                                })
                            }}
                        />
                    </label>
                    }
                    if let Some(error) = &state.error {
                        <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                    }
                    <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                        { if state.submitting { "Submitting..." } else if state.dry_run { "Submit dry run" } else { "Record endpoint" } }
                    </button>
                    if let Some(submitted) = &state.submitted {
                        <section class="dos-plan-result" data-job-state="accepted">
                            <span class="dos-card-label">{ "Daemon job accepted" }</span>
                            <h3>{ format!("{} recorded as {}.", submitted.endpoint_id, submitted.validation_state) }</h3>
                            <p>{ format!("Job {} · {} · dry run {}", submitted.accepted.job_id, submitted.accepted.kind, submitted.accepted.dry_run) }</p>
                            <code>{ format!("{} · {} · {}", submitted.display_name, submitted.kind, submitted.registry_path) }</code>
                            if let Some(actor) = &submitted.administrator_actor {
                                <p class="dos-job-message">{ format!("Administrator actor: {actor}") }</p>
                            }
                        </section>
                    }
                </section>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn endpoint_upsert_request_from_state(
    state: &EndpointUpsertFormState,
) -> EndpointInventoryUpsertRequest {
    EndpointInventoryUpsertRequest {
        endpoint_id: state.endpoint_id.trim().to_string(),
        display_name: state.display_name.trim().to_string(),
        kind: state.kind.clone(),
        object_service_url: state.object_service_url.trim().to_string(),
        validation: EndpointValidationUpsertRequest {
            state: state.validation_state.clone(),
            checked_at_utc: (!state.checked_at_utc.trim().is_empty())
                .then(|| state.checked_at_utc.trim().to_string()),
            message: (!state.validation_message.trim().is_empty())
                .then(|| state.validation_message.trim().to_string()),
        },
        manager_product_id: state.manager_product_id.trim().to_string(),
        active_bindings: if state.binding_enabled {
            vec![EndpointBindingUpsertRequest {
                binding_id: if state.binding_id.trim().is_empty() {
                    format!("{}--{}", state.endpoint_id.trim(), state.store_id.trim())
                } else {
                    state.binding_id.trim().to_string()
                },
                governance_domain: state.governance_domain.trim().to_string(),
                store_id: state.store_id.trim().to_string(),
                readiness: state.binding_readiness.clone(),
            }]
        } else {
            Vec::new()
        },
        dry_run: state.dry_run,
        client_request_id: None,
        confirmation_marker: (!state.confirmation_phrase.trim().is_empty())
            .then(|| state.confirmation_phrase.trim().to_string()),
    }
}

#[cfg(target_arch = "wasm32")]
fn update_endpoint_form_from_input<F>(
    form_state: &UseStateHandle<EndpointUpsertFormState>,
    event: InputEvent,
    update: F,
) where
    F: FnOnce(&mut EndpointUpsertFormState, String),
{
    let input: HtmlInputElement = event.target_unchecked_into();
    let mut next = (**form_state).clone();
    update(&mut next, input.value());
    next.clear_result();
    form_state.set(next);
}

#[cfg(target_arch = "wasm32")]
fn update_endpoint_form_from_select<F>(
    form_state: &UseStateHandle<EndpointUpsertFormState>,
    event: Event,
    update: F,
) where
    F: FnOnce(&mut EndpointUpsertFormState, String),
{
    let input: HtmlSelectElement = event.target_unchecked_into();
    let mut next = (**form_state).clone();
    update(&mut next, input.value());
    next.clear_result();
    form_state.set(next);
}

#[cfg(target_arch = "wasm32")]
fn endpoint_text_field(label: &'static str, value: String, oninput: Callback<InputEvent>) -> Html {
    html! {
        <label class="dos-form-field">
            <span>{ label }</span>
            <input value={value} oninput={oninput} />
        </label>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_endpoint_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        endpoint_binding_fields_ready, endpoint_card_summaries, endpoint_inventory_summary_cards,
        endpoint_inventory_upsert_action_api_path, endpoint_upsert_confirmation_matches,
        endpoint_upsert_fields_ready, endpoint_upsert_review_from_values,
        endpoints_workspace_api_path, ENDPOINTS_WORKSPACE_ROUTE, ENDPOINT_INVENTORY_UPSERT_ROUTE,
        ENDPOINT_RECORD_CONFIRMATION,
    };
    use crate::api::{
        EndpointBindingResponse, EndpointInventoryItemResponse, EndpointInventoryResponse,
        EndpointValidationResponse, EndpointsWorkspaceResponse,
    };

    #[test]
    fn builds_endpoints_workspace_api_path() {
        assert_eq!(
            endpoints_workspace_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints"
        );
    }

    #[test]
    fn builds_endpoint_inventory_upsert_action_api_path() {
        assert_eq!(
            endpoint_inventory_upsert_action_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints/upsert"
        );
        assert_eq!(ENDPOINTS_WORKSPACE_ROUTE, "workspaces/endpoints");
        assert_eq!(
            ENDPOINT_INVENTORY_UPSERT_ROUTE,
            "workspaces/endpoints/upsert"
        );
    }

    #[test]
    fn endpoint_upsert_form_gates_required_fields_binding_and_confirmation() {
        assert!(endpoint_upsert_fields_ready(
            "nas-staging",
            "NAS staging",
            "https://nas.example.test:9443"
        ));
        assert!(!endpoint_upsert_fields_ready(
            "nas-staging",
            "",
            "https://nas.example.test:9443"
        ));
        assert!(endpoint_binding_fields_ready(false, "", "", ""));
        assert!(endpoint_binding_fields_ready(
            true,
            "binding-1",
            "local",
            "zymo"
        ));
        assert!(!endpoint_binding_fields_ready(true, "", "", "zymo"));
        assert!(endpoint_binding_fields_ready(true, "", "local", "zymo"));
        assert!(endpoint_upsert_confirmation_matches(true, ""));
        assert!(endpoint_upsert_confirmation_matches(
            false,
            ENDPOINT_RECORD_CONFIRMATION
        ));
        assert!(!endpoint_upsert_confirmation_matches(
            false,
            "record endpoint"
        ));
    }

    #[test]
    fn endpoint_upsert_review_captures_validation_and_binding() {
        let review = endpoint_upsert_review_from_values(
            "nas-staging",
            "dasobjectstore_nfs",
            "validated",
            "https://nas.example.test:9443",
            true,
            "zymo",
        );

        assert_eq!(
            review,
            "nas-staging · dasobjectstore_nfs · validation validated · https://nas.example.test:9443 · binding to zymo"
        );
    }

    #[test]
    fn endpoint_inventory_payload_maps_to_cards_and_metrics() {
        let view = EndpointsWorkspaceResponse {
            inventory: EndpointInventoryResponse {
                schema_version: "dasobjectstore.endpoint_inventory.v1".to_string(),
                endpoint_count: 1,
                degraded_endpoint_count: 0,
                binding_count: 1,
                endpoints: vec![EndpointInventoryItemResponse {
                    endpoint_id: "nas-staging".to_string(),
                    display_name: "NAS staging".to_string(),
                    kind: "dasobjectstore_nfs".to_string(),
                    manager_product_id: "dasobjectstore".to_string(),
                    object_service_url: "https://nas.example.test:9443".to_string(),
                    validation: EndpointValidationResponse {
                        state: "validated".to_string(),
                        checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                        message: Some("validated".to_string()),
                    },
                    active_bindings: vec![EndpointBindingResponse {
                        binding_id: "binding-1".to_string(),
                        governance_domain: "local".to_string(),
                        store_id: "zymo".to_string(),
                        readiness: "ready".to_string(),
                    }],
                    warnings: Vec::new(),
                }],
                warnings: Vec::new(),
            },
        };

        let cards = endpoint_card_summaries(&view);
        let metrics = endpoint_inventory_summary_cards(&view);

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "nas-staging");
        assert_eq!(cards[0].bindings, "1 binding(s)");
        assert_eq!(cards[0].checked_at, "2026-07-09T00:00:00Z");
        assert_eq!(metrics[0].1, "1");
        assert_eq!(metrics[1].1, "1");
    }
}
