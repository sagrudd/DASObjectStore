#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
#[function_component(RemoteUploadPage)]
pub fn remote_upload_page(props: &RemoteUploadPageProps) -> Html {
    let api_path = WorkspacePage::RemoteUpload.api_path(&props.api_base_path);
    let remote_upload_state = use_state(|| ApiLoadState::<RemoteUploadWorkspaceResponse>::Loading);
    let selected_store = use_state(String::new);
    let selected_files = use_state(Vec::<RemoteUploadSelectedFile>::new);

    {
        let api_path = api_path.clone();
        let remote_upload_state = remote_upload_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                remote_upload_state.set(page_load_state_from_result(
                    crate::api::get_remote_upload_workspace(&path).await,
                    |view| {
                        view.stores.is_empty().then(|| {
                            view.warnings
                                .first()
                                .map(|warning| warning.message.clone())
                                .unwrap_or_else(|| {
                                    "No ObjectStores are available for remote upload.".to_string()
                                })
                        })
                    },
                ));
            });
            || ()
        });
    }

    html! {
        <section class="dos-page" data-page="remote-upload" data-api-route={api_path}>
            <PageHeader
                eyebrow="Easyconnect"
                title="Remote Upload"
                summary="Browser-approved ObjectStore selection for paired remote upload agents."
            />
            { render_remote_upload_state(&*remote_upload_state, selected_store, selected_files) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_state(
    state: &ApiLoadState<RemoteUploadWorkspaceResponse>,
    selected_store: UseStateHandle<String>,
    selected_files: UseStateHandle<Vec<RemoteUploadSelectedFile>>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_remote_upload_state_message(
            "Loading",
            "Loading remote-upload workspace",
            "The Web console is requesting accessible ObjectStores and writer readiness.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_remote_upload_workspace(view, selected_store, selected_files)
        }
        ApiLoadState::Empty(message) => {
            render_remote_upload_state_message("Inventory", "No remote uploads available", message)
        }
        ApiLoadState::PermissionDenied(message) => render_remote_upload_state_message(
            "Permission denied",
            "Remote upload requires an authenticated easyconnect session",
            message,
        ),
        ApiLoadState::TransportError(message) => render_remote_upload_state_message(
            "Error",
            "Unable to load remote-upload workspace",
            message,
        ),
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_workspace(
    view: &RemoteUploadWorkspaceResponse,
    selected_store: UseStateHandle<String>,
    selected_files: UseStateHandle<Vec<RemoteUploadSelectedFile>>,
) -> Html {
    let ready_count = view
        .stores
        .iter()
        .filter(|store| store.upload_allowed)
        .count();
    html! {
        <div class="dos-store-grid">
            <section class="dos-card dos-wide-card" data-state="ready">
                <span class="dos-card-label">{ "Current session" }</span>
                <h2>{ format!("{} ObjectStore(s) visible", view.stores.len()) }</h2>
                <p>{ format!("{} store(s) are ready for upload by {}. Upload execution remains delegated to the paired dasobjectstore-remote process.", ready_count, view.actor.username) }</p>
                <div class="dos-card-row">
                    <span class="dos-status-pill">{ if view.actor.sudo_administrator { "administrator" } else { "standard user" } }</span>
                    <span class="dos-status-pill">{ format!("{} group(s)", view.actor.groups.len()) }</span>
                </div>
            </section>
            { for view.warnings.iter().map(render_remote_upload_warning) }
            { for view.stores.iter().map(render_remote_upload_store_card) }
            { render_remote_upload_selection_panel(view, selected_store, selected_files) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_selection_panel(
    view: &RemoteUploadWorkspaceResponse,
    selected_store: UseStateHandle<String>,
    selected_files: UseStateHandle<Vec<RemoteUploadSelectedFile>>,
) -> Html {
    let ready_stores = view
        .stores
        .iter()
        .filter(|store| store.upload_allowed)
        .collect::<Vec<_>>();
    let effective_store_id = if selected_store.trim().is_empty() {
        ready_stores
            .first()
            .map(|store| store.store_id.clone())
            .unwrap_or_default()
    } else {
        (*selected_store).clone()
    };
    let selected_target = ready_stores
        .iter()
        .find(|store| store.store_id == effective_store_id)
        .copied();
    let summary = RemoteUploadSelectionSummary::from_files(&selected_files);
    let ready_for_handoff = selected_target.is_some() && summary.file_count > 0;
    let on_store_change = {
        let selected_store = selected_store.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            selected_store.set(input.value());
        })
    };
    let on_file_input = {
        let selected_files = selected_files.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Some(files) = input.files() {
                selected_files.set(remote_upload_selected_files_from_list(files));
            }
        })
    };
    let on_drop = {
        let selected_files = selected_files.clone();
        Callback::from(move |event: DragEvent| {
            event.prevent_default();
            if let Some(data_transfer) = event.data_transfer() {
                if let Some(files) = data_transfer.files() {
                    selected_files.set(remote_upload_selected_files_from_list(files));
                }
            }
        })
    };
    let on_drag_over = Callback::from(|event: DragEvent| event.prevent_default());
    let on_clear = {
        let selected_files = selected_files.clone();
        Callback::from(move |_| selected_files.set(Vec::new()))
    };

    html! {
        <section class="dos-card dos-wide-card dos-remote-upload-panel" data-state={if ready_for_handoff { "ready" } else { "waiting" }}>
            <div class="dos-card-row">
                <div>
                    <span class="dos-card-label">{ "Agent handoff" }</span>
                    <h2>{ "Select files or folders for remote ingress" }</h2>
                </div>
                <span class="dos-status-pill">{ if ready_for_handoff { "ready to confirm" } else { "waiting" } }</span>
            </div>
            <div class="dos-remote-upload-grid">
                <div class="dos-form-field">
                    <span>{ "Target ObjectStore" }</span>
                    <select onchange={on_store_change} value={effective_store_id.clone()} disabled={ready_stores.is_empty()}>
                        { for ready_stores.iter().map(|store| html! {
                            <option value={store.store_id.clone()}>{ format!("{} · {}", store.display_name, store.object_type) }</option>
                        }) }
                    </select>
                </div>
                <label class="dos-remote-upload-dropzone" ondrop={on_drop} ondragover={on_drag_over}>
                    <strong>{ "Drop files or folders here" }</strong>
                    <span>{ "The browser records local metadata only; bytes transfer through the paired dasobjectstore-remote agent after confirmation." }</span>
                    <input
                        type="file"
                        multiple=true
                        webkitdirectory=true
                        directory=true
                        onchange={on_file_input}
                        disabled={ready_stores.is_empty()}
                    />
                </label>
            </div>
            { render_remote_upload_selection_summary(&summary, selected_target.map(|store| store.display_name.as_str())) }
            <div class="dos-job-actions">
                <button type="button" onclick={on_clear} disabled={summary.file_count == 0}>{ "Clear selection" }</button>
                <button type="button" disabled=true title="Loopback agent coordination is the next remote-upload task.">
                    { if ready_for_handoff { "Confirm with local agent" } else { "Select a writable store and files" } }
                </button>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_selection_summary(
    summary: &RemoteUploadSelectionSummary,
    target_name: Option<&str>,
) -> Html {
    let target_name = target_name.unwrap_or("no writable ObjectStore selected");
    html! {
        <div class="dos-remote-upload-summary">
            <div><dt>{ "Target" }</dt><dd>{ target_name.to_string() }</dd></div>
            <div><dt>{ "Files" }</dt><dd>{ summary.file_count }</dd></div>
            <div><dt>{ "Folders" }</dt><dd>{ summary.folder_count }</dd></div>
            <div><dt>{ "Bytes selected" }</dt><dd>{ summary.total_size_label() }</dd></div>
            <div><dt>{ "Largest file" }</dt><dd>{ summary.largest_file_label() }</dd></div>
            <div class="dos-remote-upload-samples">
                <dt>{ "Sample paths" }</dt>
                <dd>
                    { if summary.sample_paths.is_empty() {
                        html! { <span>{ "no browser selection yet" }</span> }
                    } else {
                        html! {
                            <ol>
                                { for summary.sample_paths.iter().map(|path| html! { <li>{ path.clone() }</li> }) }
                            </ol>
                        }
                    } }
                </dd>
            </div>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn remote_upload_selected_files_from_list(
    files: web_sys::FileList,
) -> Vec<RemoteUploadSelectedFile> {
    (0..files.length())
        .filter_map(|index| files.item(index))
        .map(|file| RemoteUploadSelectedFile {
            display_path: remote_upload_file_display_path(&file),
            size_bytes: file.size() as u64,
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn remote_upload_file_display_path(file: &File) -> String {
    js_sys::Reflect::get(
        file.as_ref(),
        &wasm_bindgen::JsValue::from_str("webkitRelativePath"),
    )
    .ok()
    .and_then(|value| value.as_string())
    .filter(|path| !path.trim().is_empty())
    .unwrap_or_else(|| file.name())
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_warning(warning: &crate::api::DashboardWarning) -> Html {
    html! {
        <section class="dos-card dos-wide-card" data-state="warning">
            <span class="dos-card-label">{ warning.code.clone() }</span>
            <h2>{ "Remote-upload attention" }</h2>
            <p>{ warning.message.clone() }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_store_card(
    store: &crate::api::RemoteUploadObjectStoreResponse,
) -> Html {
    html! {
        <section class="dos-card dos-store-card" data-state={store.upload_state.clone()} data-object-type={store.object_type.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ store.store_class.clone() }</span>
                <span class="dos-status-pill">{ store.upload_state.clone() }</span>
            </div>
            <h2>{ store.display_name.clone() }</h2>
            <p>{ store.upload_message.clone() }</p>
            <dl class="dos-definition-list">
                <div>
                    <dt>{ "Bucket" }</dt>
                    <dd>{ store.bucket.clone() }</dd>
                </div>
                <div>
                    <dt>{ "Object type" }</dt>
                    <dd>{ store.object_type.clone() }</dd>
                </div>
                <div>
                    <dt>{ "Capacity used" }</dt>
                    <dd>{ format!("{} TiB · {} bps", store.capacity.used_tib, store.capacity.used_percent_basis_points) }</dd>
                </div>
                <div>
                    <dt>{ "Writer group" }</dt>
                    <dd>{ store.writer_group.clone().unwrap_or_else(|| "not configured".to_string()) }</dd>
                </div>
                <div>
                    <dt>{ "Export" }</dt>
                    <dd>{ format!("{} · {}", store.endpoint_export_mode, if store.public { "public" } else { "restricted" }) }</dd>
                </div>
            </dl>
            { if store.warnings.is_empty() {
                Html::default()
            } else {
                html! {
                    <div class="dos-card-warnings">
                        { for store.warnings.iter().map(|warning| html! {
                            <p>{ warning.message.clone() }</p>
                        }) }
                    </div>
                }
            } }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_remote_upload_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label.to_string() }</span>
            <h2>{ title.to_string() }</h2>
            <p>{ message.to_string() }</p>
        </section>
    }
}
