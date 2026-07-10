#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_store_configure_card(
    view: &ObjectStoresPageResponse,
    configure_state: UseStateHandle<ObjectStoreConfigureFormState>,
    api_base_path: String,
) -> Html {
    let state = (*configure_state).clone();
    let enabled = view.create_object_store.enabled && !view.stores.is_empty();
    let status = if enabled {
        "Available"
    } else if view.stores.is_empty() {
        "No stores"
    } else {
        "Admin only"
    };
    let detail = if enabled {
        "Select an existing ObjectStore and review policy changes before daemon-owned execution."
    } else if view.stores.is_empty() {
        "Configuration is available after at least one ObjectStore exists."
    } else {
        view.create_object_store
            .blocked_reason
            .as_deref()
            .unwrap_or("Current user must be an administrator to configure ObjectStores.")
    };
    let group_options = view.groups.clone();
    let store_class_options = view.create_object_store.store_class_options.clone();
    let copy_count_options = if view.create_object_store.copy_count_options.is_empty() {
        vec![1, 2, 3]
    } else {
        view.create_object_store.copy_count_options.clone()
    };
    let can_plan = enabled
        && !state.planning
        && !state.selected_store_id.trim().is_empty()
        && !state.store_class.trim().is_empty()
        && !state.writer_group.trim().is_empty();
    let can_apply = enabled
        && !state.submitting
        && !state.selected_store_id.trim().is_empty()
        && (state.ingest_mode != "direct_to_hdd"
            || state.confirmation_marker.trim() == "confirm direct hdd ingest");

    let open_form = {
        let configure_state = configure_state.clone();
        let initial = ObjectStoreConfigureFormState::from_view(Some(view));
        Callback::from(move |_| {
            let mut next = (*configure_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            configure_state.set(next);
        })
    };

    let plan = {
        let configure_state = configure_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*configure_state).clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            configure_state.set(pending.clone());

            let configure_state = configure_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "store_configure".to_string(),
                    store_id: Some(pending.selected_store_id.trim().to_string()),
                    store_class: Some(pending.store_class.clone()),
                    store_copies: Some(pending.required_copies),
                    bucket: None,
                    writer_group: Some(pending.writer_group.trim().to_string()),
                    ssd_root: Some(pending.ssd_root.trim().to_string()),
                    public: Some(pending.public),
                    writeable: Some(pending.writeable),
                    capacity_behavior: Some(pending.capacity_behavior.clone()),
                    retention: Some(pending.retention.clone()),
                    endpoint_export_mode: Some(pending.endpoint_export_mode.clone()),
                    subobject_name: None,
                    parent_store_id: None,
                    parent_subobject_name: None,
                    subobject_object_type: None,
                    subobject_inherits_object_type: None,
                    subobject_s3_routing: None,
                    ssd_device: None,
                    hdd_devices: Vec::new(),
                    mount_root: None,
                    filesystem: None,
                    owner: None,
                    allow_format: false,
                    existing_data_acknowledged: false,
                    confirmation_phrase: None,
                };
                let result = crate::api::plan_gui_action(&api_base_path, &request).await;
                let mut next = (*configure_state).clone();
                next.planning = false;
                match result {
                    Ok(plan) => {
                        next.plan = Some(plan);
                        next.error = None;
                    }
                    Err(error) => {
                        next.plan = None;
                        next.error = Some(error.message);
                    }
                }
                configure_state.set(next);
            });
        })
    };

    let apply_ingest_policy = {
        let configure_state = configure_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*configure_state).clone();
            pending.submitting = true;
            pending.error = None;
            pending.submitted = None;
            configure_state.set(pending.clone());
            let configure_state = configure_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = crate::api::submit_object_store_ingest_policy(
                    &api_base_path,
                    &ObjectStoreIngestPolicyRequest {
                        store_id: pending.selected_store_id.trim().to_string(),
                        ingest_mode: pending.ingest_mode.clone(),
                        dry_run: false,
                        client_request_id: Some(format!(
                            "web-store-policy-{}",
                            pending.selected_store_id.trim()
                        )),
                        confirmation_marker: (!pending.confirmation_marker.trim().is_empty())
                            .then(|| pending.confirmation_marker.trim().to_string()),
                    },
                )
                .await;
                let mut next = (*configure_state).clone();
                next.submitting = false;
                match result {
                    Ok(response) => {
                        next.submitted = Some(response);
                        next.error = None;
                    }
                    Err(error) => {
                        next.submitted = None;
                        next.error = Some(error.message);
                    }
                }
                configure_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-objectstore-configure" data-action="store_configure">
            <span class="dos-create-mark">{ "!" }</span>
            <h2>{ "Configure ObjectStore" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close policy editor" } else { "Edit policy" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        <label class="dos-form-field">
                            <span>{ "ObjectStore" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                let stores = view.stores.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    if let Some(store) = stores.iter().find(|store| store.store_id == input.value()) {
                                        next.apply_store(store);
                                    } else {
                                        next.selected_store_id = input.value();
                                        next.reset_plan();
                                    }
                                    configure_state.set(next);
                                })
                            }} value={state.selected_store_id.clone()}>
                                { for view.stores.iter().map(|store| html! {
                                    <option value={store.store_id.clone()}>{ format!("{} ({})", store.display_name, store.store_id) }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Store class" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.store_class = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.store_class.clone()}>
                                { for store_class_options.iter().map(|option| html! {
                                    <option value={option.value.clone()}>{ option.label.clone() }</option>
                                }) }
                                if store_class_options.is_empty() {
                                    <option value={state.store_class.clone()}>{ state.store_class.clone() }</option>
                                }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Ingest landing mode" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.ingest_mode = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.ingest_mode.clone()}>
                                <option value="ssd_first">{ "SSD first (safe default)" }</option>
                                <option value="direct_to_hdd">{ "Direct to HDD (local sources only)" }</option>
                            </select>
                        </label>
                        if state.ingest_mode == "direct_to_hdd" {
                            { object_store_text_field("Confirmation", state.confirmation_marker.clone(), {
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: InputEvent| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.confirmation_marker = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }) }
                        }
                        <label class="dos-form-field">
                            <span>{ "Redundancy" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.required_copies = input.value().parse::<u8>().unwrap_or(1);
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.required_copies.to_string()}>
                                { for copy_count_options.iter().map(|copies| html! {
                                    <option value={copies.to_string()}>{ format!("{copies} copy/copies") }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Writer group" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.writer_group = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.writer_group.clone()}>
                                <option value="">{ "Select writer group" }</option>
                                { for group_options.iter().map(|group| html! {
                                    <option value={group.group_name.clone()}>
                                        { format!("{} ({})", group.display_name, group.group_name) }
                                    </option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Capacity behavior" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.capacity_behavior = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.capacity_behavior.clone()}>
                                <option value="reject_writes">{ "Reject writes" }</option>
                                <option value="backpressure_by_priority">{ "Backpressure by priority" }</option>
                                <option value="mark_redownload_required">{ "Mark redownload required" }</option>
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Retention" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.retention = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.retention.clone()}>
                                <option value="immediate_delete">{ "Immediate delete" }</option>
                                <option value="tombstone_then_gc">{ "Tombstone then GC" }</option>
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Export mode" }</span>
                            <select onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.endpoint_export_mode = input.value();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} value={state.endpoint_export_mode.clone()}>
                                <option value="s3">{ "S3" }</option>
                                <option value="read_only_file_export">{ "Read-only file export" }</option>
                                <option value="disabled">{ "Disabled" }</option>
                            </select>
                        </label>
                        { object_store_text_field("SSD root", state.ssd_root.clone(), {
                            let configure_state = configure_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*configure_state).clone();
                                next.ssd_root = input.value();
                                next.reset_plan();
                                configure_state.set(next);
                            })
                        }) }
                    </div>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input type="checkbox" checked={state.public} onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.public = input.checked();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} />
                            <span>{ "Publicly visible within the appliance policy boundary" }</span>
                        </label>
                        <label>
                            <input type="checkbox" checked={state.writeable} onchange={{
                                let configure_state = configure_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*configure_state).clone();
                                    next.writeable = input.checked();
                                    next.reset_plan();
                                    configure_state.set(next);
                                })
                            }} />
                            <span>{ "Writeable by the selected writer group" }</span>
                        </label>
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Configuration review" }</span>
                        <p>{ object_store_configure_review(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review configuration plan" } }
                        </button>
                        <button class="dos-primary-action" type="button" disabled={!can_apply} onclick={apply_ingest_policy}>
                            { if state.submitting { "Applying..." } else { "Apply ingest mode" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                        if let Some(response) = &state.submitted {
                            <p class="dos-job-message">{ format!("Applied {} for {} (job {})", response.ingest_mode, response.store_id, response.job_id) }</p>
                        }
                    </section>
                </div>
            }
        </section>
    }
}
