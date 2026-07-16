#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_store_create_card(
    view: Option<&ObjectStoresPageResponse>,
    create_state: UseStateHandle<ObjectStoreCreateFormState>,
    api_base_path: String,
) -> Html {
    let (status, detail) = match view {
        Some(view) if view.create_object_store.enabled => (
            "Available".to_string(),
            "Admin workflow: create an enclosure-anchored Generated Data ObjectStore after daemon plan review.".to_string(),
        ),
        Some(view) => (
            "Admin only".to_string(),
            view.create_object_store
                .blocked_reason
                .clone()
                .unwrap_or_else(|| {
                    "Admin workflow: assign a writer group, choose enclosure, object type, and redundancy, then submit the daemon creation plan.".to_string()
                }),
        ),
        None => (
            "Admin only".to_string(),
            "Admin workflow: assign a writer group, choose enclosure, object type, and redundancy, then submit the daemon creation plan.".to_string(),
        ),
    };
    let state = (*create_state).clone();
    let enabled = view.is_some_and(|view| view.create_object_store.enabled);
    let store_class_options = view
        .map(|view| view.create_object_store.store_class_options.clone())
        .unwrap_or_default();
    let copy_count_options = view
        .map(|view| view.create_object_store.copy_count_options.clone())
        .filter(|options| !options.is_empty())
        .unwrap_or_else(|| vec![1, 2, 3]);
    let group_options = view.map(|view| view.groups.clone()).unwrap_or_default();
    let enclosure_options = view
        .map(|view| view.mounted_enclosures.clone())
        .unwrap_or_default();
    let can_plan = enabled
        && !state.planning
        && object_store_creation_fields_ready(
            &state.store_id,
            &state.writer_group,
            &state.enclosure_id,
        );
    let can_submit = enabled
        && state.plan.is_some()
        && !state.submitting
        && object_store_create_confirmation_matches(&state.confirmation_phrase);

    let open_form = {
        let create_state = create_state.clone();
        let initial = ObjectStoreCreateFormState::from_view(view);
        Callback::from(move |_| {
            let mut next = (*create_state).clone();
            if !next.open {
                let mut seeded = initial.clone();
                seeded.open = true;
                next = seeded;
            } else {
                next.open = false;
            }
            create_state.set(next);
        })
    };
    let plan = {
        let create_state = create_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_state).clone();
            let bucket = if pending.bucket.trim().is_empty() {
                object_store_bucket_default(&pending.store_id)
            } else {
                pending.bucket.trim().to_string()
            };
            pending.bucket = bucket.clone();
            pending.planning = true;
            pending.plan = None;
            pending.error = None;
            create_state.set(pending.clone());

            let create_state = create_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = GuiActionPlanRequest {
                    action: "store_create".to_string(),
                    store_id: Some(pending.store_id.trim().to_string()),
                    store_class: Some(pending.store_class.clone()),
                    store_copies: Some(pending.required_copies),
                    bucket: (!bucket.is_empty()).then_some(bucket),
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
                    subobject_capacity_limit_bytes: None,
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
                let mut next = (*create_state).clone();
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
                create_state.set(next);
            });
        })
    };
    let submit = {
        let create_state = create_state.clone();
        let api_base_path = api_base_path.clone();
        Callback::from(move |_| {
            let mut pending = (*create_state).clone();
            pending.submitting = true;
            pending.error = None;
            pending.submitted = None;
            create_state.set(pending.clone());

            let create_state = create_state.clone();
            let api_base_path = api_base_path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let request = object_store_create_request_from_state(&pending);
                let result = crate::api::submit_object_store_create(&api_base_path, &request).await;
                let mut next = (*create_state).clone();
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
                create_state.set(next);
            });
        })
    };

    html! {
        <section class="dos-card dos-create-card dos-objectstore-create" data-action="store_create">
            <span class="dos-create-mark">{ "+" }</span>
            <h2>{ "Create ObjectStore" }</h2>
            <p>{ detail }</p>
            <span class="dos-status-pill">{ status }</span>
            <button
                class="dos-secondary-action"
                type="button"
                disabled={!enabled}
                onclick={open_form}
            >
                { if state.open { "Close form" } else { "Configure store" } }
            </button>
            if state.open {
                <div class="dos-objectstore-form">
                    <div class="dos-form-grid">
                        { object_store_text_field("Store name", state.store_id.clone(), {
                            let create_state = create_state.clone();
                            Callback::from(move |event: InputEvent| {
                                let input: HtmlInputElement = event.target_unchecked_into();
                                let mut next = (*create_state).clone();
                                next.store_id = input.value();
                                next.bucket = object_store_bucket_default(&next.store_id);
                                next.reset_plan();
                                create_state.set(next);
                            })
                        }) }
                        <label class="dos-form-field">
                            <span>{ "S3 bucket" }</span>
                            <input readonly=true value={object_store_bucket_default(&state.store_id)} />
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Writer group" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.writer_group = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
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
                            <span>{ "Enclosure" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                let enclosure_options = enclosure_options.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let selected = input.value();
                                    let mut next = (*create_state).clone();
                                    next.enclosure_id = selected.clone();
                                    if let Some(enclosure) = enclosure_options
                                        .iter()
                                        .find(|enclosure| enclosure.enclosure_id == selected)
                                    {
                                        next.ssd_root = enclosure_ssd_root(enclosure);
                                    }
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.enclosure_id.clone()}>
                                <option value="">{ "Select mounted enclosure" }</option>
                                { for enclosure_options.iter().map(|enclosure| html! {
                                    <option value={enclosure.enclosure_id.clone()}>
                                        { format!("{} ({})", enclosure.display_name, enclosure.enclosure_id) }
                                    </option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Object type" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.object_type = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.object_type.clone()}>
                                { for ["naive", "bam", "cram", "pod5", "fastq", "fastq_gz", "fasta", "vcf", "bcf", "gff", "gtf", "ena_sra"].iter().map(|value| html! {
                                    <option value={(*value).to_string()}>{ *value }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Store class" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.store_class = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
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
                            <span>{ "Redundancy" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.required_copies = input.value().parse::<u8>().unwrap_or(1);
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.required_copies.to_string()}>
                                { for copy_count_options.iter().map(|copies| html! {
                                    <option value={copies.to_string()}>{ format!("{copies} copy/copies") }</option>
                                }) }
                            </select>
                        </label>
                        <label class="dos-form-field">
                            <span>{ "Export mode" }</span>
                            <select onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlSelectElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.endpoint_export_mode = input.value();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} value={state.endpoint_export_mode.clone()}>
                                <option value="s3_bucket">{ "S3 bucket" }</option>
                                <option value="read_only_export">{ "Read-only export" }</option>
                                <option value="internal_only">{ "Internal only" }</option>
                            </select>
                        </label>
                    </div>
                    <div class="dos-checkbox-list dos-objectstore-flags">
                        <label>
                            <input type="checkbox" checked={state.public} onchange={{
                                let create_state = create_state.clone();
                                Callback::from(move |event: Event| {
                                    let input: HtmlInputElement = event.target_unchecked_into();
                                    let mut next = (*create_state).clone();
                                    next.public = input.checked();
                                    next.reset_plan();
                                    create_state.set(next);
                                })
                            }} />
                            <span>{ "Publicly visible within the appliance policy boundary" }</span>
                        </label>
                    </div>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Derived policy" }</span>
                        <p>{ format!("SSD root: {}", if state.ssd_root.is_empty() { "select an enclosure".to_string() } else { state.ssd_root.clone() }) }</p>
                        <p>{ "Capacity is distributed by available space, retention is until explicitly deleted, and the store remains writeable by its writer group until locked after population." }</p>
                    </section>
                    <section class="dos-plan-result">
                        <span class="dos-card-label">{ "Creation review" }</span>
                        <p>{ object_store_create_review(&state) }</p>
                        <button class="dos-secondary-action" type="button" disabled={!can_plan} onclick={plan}>
                            { if state.planning { "Planning..." } else { "Review daemon plan" } }
                        </button>
                        if let Some(error) = &state.error {
                            <div class="dos-auth-error" role="alert">{ error.clone() }</div>
                        }
                        if let Some(plan) = &state.plan {
                            <code>{ plan.argv.join(" ") }</code>
                            <p class="dos-job-message">{ format!("{} · confirmation required: {}", plan.execution, plan.confirmation_required) }</p>
                        }
                        <label class="dos-form-field">
                            <span>{ "Confirmation phrase" }</span>
                            <input
                                placeholder="confirm create objectstore"
                                value={state.confirmation_phrase.clone()}
                                oninput={{
                                    let create_state = create_state.clone();
                                    Callback::from(move |event: InputEvent| {
                                        let input: HtmlInputElement = event.target_unchecked_into();
                                        let mut next = (*create_state).clone();
                                        next.confirmation_phrase = input.value();
                                        next.submitted = None;
                                        create_state.set(next);
                                    })
                                }}
                            />
                        </label>
                        <button class="dos-auth-submit" type="button" disabled={!can_submit} onclick={submit}>
                            { if state.submitting { "Submitting..." } else { "Submit daemon job" } }
                        </button>
                        if let Some(submitted) = &state.submitted {
                            <section class="dos-plan-result" data-job-state="accepted">
                                <span class="dos-card-label">{ "Daemon job accepted" }</span>
                                <h3>{ "ObjectStore creation submitted to dasobjectstored." }</h3>
                                <p>{ format!("Job {} · {} · dry run {}", submitted.accepted.job_id, submitted.accepted.kind, submitted.accepted.dry_run) }</p>
                                <code>{ format!("{} · class {} · {} copy/copies · writer group {} · actor {}", submitted.store_id, submitted.store_class, submitted.required_copies, submitted.writer_group, submitted.administrator_actor.as_deref().unwrap_or("unknown")) }</code>
                            </section>
                        }
                    </section>
                </div>
            }
        </section>
    }
}
