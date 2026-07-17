use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_panel(
    view: &ObjectStoresPageResponse,
    browser_state: &ApiLoadState<ObjectBrowserResponse>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
    browser_endpoint: UseStateHandle<String>,
    browser_prefix: UseStateHandle<String>,
    browser_search: UseStateHandle<String>,
    browser_sort: UseStateHandle<String>,
) -> Html {
    let selected_endpoint = (*browser_endpoint).clone();
    let search_value = (*browser_search).clone();
    let sort_value = (*browser_sort).clone();
    let on_search = {
        let browser_search = browser_search.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            browser_search.set(input.value());
        })
    };
    let on_sort = {
        let browser_sort = browser_sort.clone();
        Callback::from(move |event: Event| {
            let input: HtmlSelectElement = event.target_unchecked_into();
            browser_sort.set(input.value());
        })
    };

    html! {
        <section class="dos-object-browser" data-state={browser_state.state_name()} data-objectstore={selected_endpoint.clone()}>
            <div class="dos-card-row">
                <div>
                    <span class="dos-card-label">{ "Browse objects" }</span>
                    <h2>{ view.stores.iter().find(|store| store.store_id == selected_endpoint).map(|store| store.display_name.clone()).unwrap_or_else(|| "ObjectStore contents".to_string()) }</h2>
                </div>
                <span class="dos-status-pill">{ browser_state.state_name() }</span>
            </div>
            <div class="dos-object-browser-controls">
                <label>
                    <span>{ "Search" }</span>
                    <input
                        type="search"
                        value={search_value}
                        oninput={on_search}
                        placeholder="Object name or path"
                    />
                </label>
                <label>
                    <span>{ "Sort" }</span>
                    <select onchange={on_sort} value={sort_value}>
                        <option value="name_asc">{ "Name A-Z" }</option>
                        <option value="name_desc">{ "Name Z-A" }</option>
                        <option value="size_desc">{ "Size largest" }</option>
                        <option value="size_asc">{ "Size smallest" }</option>
                        <option value="modified_desc">{ "Modified newest" }</option>
                        <option value="modified_asc">{ "Modified oldest" }</option>
                    </select>
                </label>
            </div>
            { render_object_browser_download_state(&*browser_download_state) }
            { render_object_browser_state(
                browser_state,
                browser_prefix,
                api_base_path,
                browser_download_state,
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_state(
    state: &ApiLoadState<ObjectBrowserResponse>,
    browser_prefix: UseStateHandle<String>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_object_browser_message(
            "Loading",
            "Requesting daemon-authorized object metadata.",
        ),
        ApiLoadState::Empty(message) => render_object_browser_message("Empty", message),
        ApiLoadState::PermissionDenied(message) => {
            render_object_browser_message("Permission denied", message)
        }
        ApiLoadState::TransportError(message) => render_object_browser_message("Error", message),
        ApiLoadState::Success(response) => render_object_browser_body(
            response,
            browser_prefix,
            api_base_path,
            browser_download_state,
        ),
        ApiLoadState::StaleData { value, message } => html! {
            <>
                { render_object_browser_message("Stale", message) }
                { render_object_browser_body(
                    value,
                    browser_prefix,
                    api_base_path,
                    browser_download_state,
                ) }
            </>
        },
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_download_state(state: &ObjectBrowserDownloadState) -> Html {
    match state {
        ObjectBrowserDownloadState::Idle => html! {},
        ObjectBrowserDownloadState::Starting { label } => html! {
            <div class="dos-object-browser-message" data-download-state="starting">
                <span class="dos-card-label">{ "Preparing download" }</span>
                <p>{ format!("{label} is being requested from the daemon-authorized Web API.") }</p>
            </div>
        },
        ObjectBrowserDownloadState::Started { filename, detail } => html! {
            <div class="dos-object-browser-message" data-download-state="started">
                <span class="dos-card-label">{ "Download started" }</span>
                <p>{ format!("{filename} has been sent to the browser download manager. {detail}") }</p>
            </div>
        },
        ObjectBrowserDownloadState::PermissionDenied { message } => html! {
            <div class="dos-object-browser-message" data-download-state="permission-denied">
                <span class="dos-card-label">{ "Permission denied" }</span>
                <p>{ message }</p>
            </div>
        },
        ObjectBrowserDownloadState::Error { message } => html! {
            <div class="dos-object-browser-message" data-download-state="error">
                <span class="dos-card-label">{ "Download failed" }</span>
                <p>{ message }</p>
            </div>
        },
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_message(label: &str, message: &str) -> Html {
    html! {
        <div class="dos-object-browser-message">
            <span class="dos-card-label">{ label }</span>
            <p>{ message }</p>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_body(
    response: &ObjectBrowserResponse,
    browser_prefix: UseStateHandle<String>,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    let folders = object_browser_folder_summaries(&response.folders);
    let files = object_browser_file_summaries(&response.files);
    html! {
        <div class="dos-object-browser-body" data-endpoint={response.endpoint.clone()} data-prefix={response.prefix.clone()}>
            { render_object_browser_breadcrumbs(response, browser_prefix.clone()) }
            <div class="dos-object-browser-summary">
                <span>{ format!("{} folder(s)", folders.len()) }</span>
                <span>{ format!("{} file(s)", files.len()) }</span>
                <span>{ response.total_entries.map(|entries| format!("{entries} total entries")).unwrap_or_else(|| "total pending".to_string()) }</span>
            </div>
            { render_object_browser_folders(
                folders,
                response.endpoint.clone(),
                api_base_path.clone(),
                browser_prefix.clone(),
                browser_download_state.clone(),
            ) }
            { render_object_browser_files(
                files,
                response.endpoint.clone(),
                api_base_path,
                browser_download_state,
            ) }
            {
                if response.next_cursor.is_some() {
                    html! { <p class="dos-object-browser-note">{ "More entries are available; pagination controls will be enabled in the download/action slice." }</p> }
                } else {
                    html! {}
                }
            }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_breadcrumbs(
    response: &ObjectBrowserResponse,
    browser_prefix: UseStateHandle<String>,
) -> Html {
    let root_click = {
        let browser_prefix = browser_prefix.clone();
        Callback::from(move |_| browser_prefix.set(String::new()))
    };
    html! {
        <nav class="dos-object-browser-breadcrumbs" aria-label="ObjectStore folder path">
            <button type="button" onclick={root_click}>{ response.endpoint.clone() }</button>
            { for response.breadcrumbs.iter().map(|breadcrumb| {
                let prefix = breadcrumb.prefix.clone();
                let label = breadcrumb.name.clone();
                let browser_prefix = browser_prefix.clone();
                html! {
                    <button type="button" onclick={Callback::from(move |_| browser_prefix.set(prefix.clone()))}>{ label }</button>
                }
            }) }
        </nav>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_folders(
    folders: Vec<ObjectBrowserFolderSummary>,
    endpoint: String,
    api_base_path: String,
    browser_prefix: UseStateHandle<String>,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    if folders.is_empty() {
        return html! {};
    }
    html! {
        <div class="dos-object-browser-folders">
            { for folders.into_iter().map(|folder| {
                let name = folder.name.clone();
                let objects = folder.objects.clone();
                let size = folder.size.clone();
                let readiness = folder.readiness.clone();
                let prefix = folder.prefix.clone();
                let download_prefix = folder.prefix.clone();
                let download_enabled = object_browser_folder_download_available(&readiness);
                let download_title =
                    object_browser_download_disabled_reason(&readiness, &[], None);
                let browser_prefix = browser_prefix.clone();
                let endpoint = endpoint.clone();
                let api_base_path = api_base_path.clone();
                let browser_download_state = browser_download_state.clone();
                html! {
                    <div class="dos-object-browser-folder">
                        <button type="button" class="dos-object-browser-folder-open" onclick={Callback::from(move |_| browser_prefix.set(prefix.clone()))}>
                            <strong>{ name.clone() }</strong>
                        </button>
                        <span>{ objects.clone() }</span>
                        <span>{ size.clone() }</span>
                        <span class="dos-status-pill">{ readiness }</span>
                        <button
                            type="button"
                            class="dos-object-browser-download"
                            disabled={!download_enabled}
                            title={download_title}
                            onclick={Callback::from(move |_| {
                                let confirmed = confirm_large_folder_download(&download_prefix, &objects, &size);
                                if confirmed {
                                    start_object_browser_download(
                                        api_base_path.clone(),
                                        endpoint.clone(),
                                        download_prefix.clone(),
                                        true,
                                        format!("folder {}", download_prefix),
                                        format!("{name}.tar.gz"),
                                        browser_download_state.clone(),
                                    );
                                }
                            })}
                        >
                            { "Download folder" }
                        </button>
                    </div>
                }
            }) }
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_files(
    files: Vec<ObjectBrowserFileSummary>,
    endpoint: String,
    api_base_path: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) -> Html {
    if files.is_empty() {
        return render_object_browser_message("Files", "No files in this folder.");
    }
    html! {
        <div class="dos-table-wrap dos-object-browser-table-wrap">
            <table class="dos-table dos-object-browser-table">
                <thead>
                    <tr>
                        <th>{ "Name" }</th>
                        <th>{ "Type" }</th>
                        <th>{ "Size" }</th>
                        <th>{ "Readiness" }</th>
                        <th>{ "Lifecycle" }</th>
                        <th>{ "Copies" }</th>
                        <th>{ "Placement" }</th>
                        <th>{ "Modified" }</th>
                        <th>{ "Actions" }</th>
                    </tr>
                </thead>
                <tbody>
                    { for files.into_iter().map(|file| {
                        let download_enabled = object_browser_file_download_available(file.download_source.as_deref());
                        let download_title = object_browser_download_disabled_reason(&file.readiness, &file.placements, file.download_source.as_deref());
                        let object_id = file.object_id.clone();
                        let label = file.name.clone();
                        let fallback_filename = file.name.clone();
                        let endpoint = endpoint.clone();
                        let api_base_path = api_base_path.clone();
                        let browser_download_state = browser_download_state.clone();
                        html! {
                            <tr title={file.path.clone()}>
                                <td><strong>{ file.name }</strong><span>{ file.object_id }</span></td>
                                <td>{ file.object_type }</td>
                                <td>{ file.size }</td>
                                <td><span class="dos-status-pill" data-state={object_browser_state_key(&file.readiness)}>{ file.readiness }</span></td>
                                <td>{ file.lifecycle }</td>
                                <td>{ file.copies }</td>
                                <td>{ render_object_browser_placements(&file.placement_summary, &file.placements) }</td>
                                <td>{ file.modified }</td>
                                <td>
                                    <button
                                        type="button"
                                        class="dos-object-browser-download"
                                        disabled={!download_enabled}
                                        title={download_title}
                                        onclick={Callback::from(move |_| {
                                            start_object_browser_download(
                                                api_base_path.clone(),
                                                endpoint.clone(),
                                                object_id.clone(),
                                                false,
                                                label.clone(),
                                                fallback_filename.clone(),
                                                browser_download_state.clone(),
                                            );
                                        })}
                                    >
                                        { "Download" }
                                    </button>
                                </td>
                            </tr>
                        }
                    }) }
                </tbody>
            </table>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_object_browser_placements(
    summary: &str,
    placements: &[ObjectBrowserPlacementResponse],
) -> Html {
    if placements.is_empty() {
        return html! {
            <div class="dos-object-browser-placement-stack">
                <span class="dos-object-browser-placement-summary" data-state="pending">{ summary }</span>
                <span class="dos-object-browser-placement" data-state="pending">{ "placement pending" }</span>
            </div>
        };
    }
    html! {
        <div class="dos-object-browser-placement-stack">
            <span class="dos-object-browser-placement-summary" data-state={object_browser_placement_summary_state(placements)}>{ summary }</span>
            <div class="dos-object-browser-placements">
                { for placements.iter().map(|placement| {
                    let location = labelize_state(&placement.location);
                    let state = labelize_state(&placement.state);
                    let disk = placement
                        .disk_label
                        .as_deref()
                        .or(placement.disk_id.as_deref())
                        .unwrap_or("external endpoint");
                    let size = format_browser_bytes(placement.size_bytes);
                    html! {
                        <span
                            class="dos-object-browser-placement"
                            data-location={placement.location.clone()}
                            data-state={placement.state.clone()}
                            title={format!("{} · {} · {} · {}", disk, location, state, size)}
                        >
                            { format!("{} · {} · {} · {}", disk, location, state, size) }
                        </span>
                    }
                }) }
            </div>
        </div>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_placement_summary_state(
    placements: &[ObjectBrowserPlacementResponse],
) -> String {
    if placements
        .iter()
        .any(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
    {
        "degraded".to_string()
    } else if placements
        .iter()
        .any(|placement| placement.location == "ssd_landing")
        && !placements
            .iter()
            .any(|placement| placement.location == "hdd_settled" && placement.state == "verified")
    {
        "ssd_only".to_string()
    } else if placements
        .iter()
        .any(|placement| placement.state == "pending")
    {
        "pending".to_string()
    } else {
        "verified".to_string()
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_file_download_available(download_source: Option<&str>) -> bool {
    matches!(download_source, Some("hdd_settled" | "provider_stream"))
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_folder_download_available(readiness: &str) -> bool {
    readiness.eq_ignore_ascii_case("Available")
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn object_browser_download_disabled_reason(
    readiness: &str,
    placements: &[ObjectBrowserPlacementResponse],
    download_source: Option<&str>,
) -> String {
    if download_source == Some("provider_stream") {
        return "Download through the daemon-authorized provider stream.".to_string();
    }
    if download_source == Some("hdd_settled") {
        return "Download through the daemon-authorized verified HDD copy.".to_string();
    }
    let readiness_key = object_browser_state_key(readiness);
    if readiness_key == "redownload_required" {
        return "Download disabled: daemon metadata marks this object redownload-required."
            .to_string();
    }
    if readiness_key == "unavailable" {
        return "Download disabled: no available local or external object copy is reported."
            .to_string();
    }
    if readiness_key == "ssd_only" {
        return "Download disabled until the object has a verified settled HDD copy.".to_string();
    }
    if readiness_key == "degraded" {
        return "Download disabled until degraded or missing placements are repaired.".to_string();
    }
    if readiness_key != "available" {
        return format!(
            "Download disabled until daemon readiness is Available; current state is {readiness}."
        );
    }
    if placements
        .iter()
        .any(|placement| matches!(placement.state.as_str(), "degraded" | "missing"))
    {
        return "Download disabled because at least one placement is degraded or missing."
            .to_string();
    }
    if !placements.is_empty()
        && !placements
            .iter()
            .any(|placement| placement.location == "hdd_settled" && placement.state == "verified")
    {
        if placements
            .iter()
            .any(|placement| placement.location == "ssd_landing")
        {
            return "Download disabled: only SSD landing placement is currently reported."
                .to_string();
        }
        return "Download disabled until a verified settled HDD copy is available.".to_string();
    }
    "Download through the daemon-authorized Web API.".to_string()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn confirm_large_folder_download(prefix: &str, objects: &str, size: &str) -> bool {
    let large = objects
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|count| count >= 100)
        || size.ends_with("GiB")
        || size.ends_with("TiB");
    if !large {
        return true;
    }
    web_sys::window()
        .and_then(|window| {
            window
                .confirm_with_message(&format!(
                    "Prepare archive download for folder {prefix} ({objects}, {size})?"
                ))
                .ok()
        })
        .unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn start_object_browser_download(
    api_base_path: String,
    endpoint: String,
    object_or_prefix: String,
    folder: bool,
    label: String,
    fallback_filename: String,
    browser_download_state: UseStateHandle<ObjectBrowserDownloadState>,
) {
    browser_download_state.set(ObjectBrowserDownloadState::Starting {
        label: label.clone(),
    });
    wasm_bindgen_futures::spawn_local(async move {
        let path = if folder {
            crate::api::object_folder_download_api_path(
                &api_base_path,
                &endpoint,
                &object_or_prefix,
            )
        } else {
            crate::api::object_download_api_path(&api_base_path, &endpoint, &object_or_prefix)
        };
        match crate::api::download_object_browser_asset(&path, &fallback_filename).await {
            Ok(download) => {
                let detail = object_browser_download_detail(&download);
                match download_bytes_to_host(
                    &download.filename,
                    &download.bytes,
                    &download.content_type,
                ) {
                    Ok(()) => browser_download_state.set(ObjectBrowserDownloadState::Started {
                        filename: download.filename,
                        detail,
                    }),
                    Err(message) => {
                        browser_download_state.set(ObjectBrowserDownloadState::Error { message })
                    }
                }
            }
            Err(error) if error.is_permission_denied() => {
                browser_download_state.set(ObjectBrowserDownloadState::PermissionDenied {
                    message: error.message,
                });
            }
            Err(error) => {
                browser_download_state.set(ObjectBrowserDownloadState::Error {
                    message: error.message,
                });
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
pub(super) fn object_browser_download_detail(
    download: &crate::api::ObjectBrowserDownload,
) -> String {
    if let Some(files) = download.archive_files {
        let bytes = download
            .archive_source_bytes
            .or(download.content_length)
            .map(format_browser_bytes)
            .unwrap_or_else(|| "size pending".to_string());
        format!("Archive preflight reported {files} file(s), {bytes}.")
    } else {
        download
            .content_length
            .map(|bytes| format!("Reported size: {}.", format_browser_bytes(bytes)))
            .unwrap_or_else(|| "Reported size pending.".to_string())
    }
}
