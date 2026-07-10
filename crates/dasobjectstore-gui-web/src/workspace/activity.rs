use super::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReportUploadState {
    Idle,
    Rendering {
        filename: String,
        size_label: String,
    },
    Downloaded {
        filename: String,
    },
    Failed {
        message: String,
    },
}

#[cfg(target_arch = "wasm32")]
#[function_component(ActivityPage)]
pub fn activity_page(props: &ActivityPageProps) -> Html {
    let api_path = WorkspacePage::Activity.api_path(&props.api_base_path);
    let report_upload_path =
        crate::api::activity_performance_report_upload_path(&props.api_base_path);
    let activity_state = use_state(|| ApiLoadState::<ActivityWorkspaceResponse>::Loading);
    let report_upload_state = use_state(|| ReportUploadState::Idle);

    {
        let api_path = api_path.clone();
        let activity_state = activity_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                activity_state.set(page_load_state_from_result(
                    crate::api::get_activity_workspace(&path).await,
                    |view| {
                        view.categories.is_empty().then(|| {
                            "No daemon activity categories were reported by the workspace API."
                                .to_string()
                        })
                    },
                ));
            });
            || ()
        });
    }

    let submit_report_file = {
        let report_upload_path = report_upload_path.clone();
        let report_upload_state = report_upload_state.clone();
        Callback::from(move |file: File| {
            let filename = file.name();
            let size_label = format_file_size(file.size());
            if !filename.to_ascii_lowercase().ends_with(".json") {
                report_upload_state.set(ReportUploadState::Failed {
                    message: "Select a DASObjectStore benchmarking JSON artifact.".to_string(),
                });
                return;
            }
            report_upload_state.set(ReportUploadState::Rendering {
                filename: filename.clone(),
                size_label,
            });
            let report_upload_path = report_upload_path.clone();
            let report_upload_state = report_upload_state.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match crate::api::upload_performance_report_json(&report_upload_path, file).await {
                    Ok(download) => match download_pdf_to_host(&download.filename, &download.bytes)
                    {
                        Ok(()) => report_upload_state.set(ReportUploadState::Downloaded {
                            filename: download.filename,
                        }),
                        Err(message) => {
                            report_upload_state.set(ReportUploadState::Failed { message })
                        }
                    },
                    Err(err) => report_upload_state.set(ReportUploadState::Failed {
                        message: err.message,
                    }),
                }
            });
        })
    };

    let on_report_file_change = {
        let submit_report_file = submit_report_file.clone();
        Callback::from(move |event: Event| {
            let Some(input) = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlInputElement>().ok())
            else {
                return;
            };
            if let Some(file) = input.files().and_then(|files| files.item(0)) {
                submit_report_file.emit(file);
            }
            input.set_value("");
        })
    };

    let on_report_drag_over = Callback::from(|event: DragEvent| {
        event.prevent_default();
    });
    let on_report_drop = {
        let submit_report_file = submit_report_file.clone();
        Callback::from(move |event: DragEvent| {
            event.prevent_default();
            if let Some(file) = event
                .data_transfer()
                .and_then(|transfer| transfer.files())
                .and_then(|files| files.item(0))
            {
                submit_report_file.emit(file);
            }
        })
    };

    html! {
        <section class="dos-page" data-page="activity" data-api-route={api_path}>
            <PageHeader
                eyebrow="Daemon jobs"
                title="Activity"
                summary="Administrator work, ingest, settlement, repair, and endpoint validation from the shared daemon job model."
            />
            { render_activity_state(
                &*activity_state,
                &*report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            ) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_state(
    state: &ApiLoadState<ActivityWorkspaceResponse>,
    report_upload_state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    match state {
        ApiLoadState::Loading => render_activity_state_message(
            "Loading",
            "Loading daemon activity",
            "The Web console is requesting the shared daemon activity workspace.",
        ),
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => {
            render_activity_workspace(
                view,
                report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            )
        }
        ApiLoadState::Empty(message) => {
            render_activity_state_message("Inventory", "No daemon activity data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_activity_state_message(
            "Permission denied",
            "Activity requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_activity_state_message("Error", "Unable to load Activity", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_workspace(
    view: &ActivityWorkspaceResponse,
    report_upload_state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    html! {
        <>
            <div class="dos-metric-grid dos-activity-queues">
                { for activity_queue_summary(view).into_iter().map(render_metric_card) }
            </div>
            <div class="dos-activity-grid">
                { for activity_category_summaries(view).into_iter().map(render_activity_category_card) }
            </div>
            { render_activity_reporting_card(
                report_upload_state,
                on_report_file_change,
                on_report_drag_over,
                on_report_drop,
            ) }
            <section class="dos-card dos-wide-card dos-activity-tasks">
                <div class="dos-card-row">
                    <span class="dos-card-label">{ "Daemon task stream" }</span>
                    <span class="dos-status-pill">{ format!("{} task(s)", view.tasks.len()) }</span>
                </div>
                if view.tasks.is_empty() {
                    <p>{ "No active administrator, ingest, destage, repair, or endpoint validation tasks are currently reported." }</p>
                } else {
                    <div class="dos-task-list">
                        { for view.tasks.iter().map(render_activity_task) }
                    </div>
                }
            </section>
            if !view.warnings.is_empty() {
                <section class="dos-card dos-wide-card" data-state="warning">
                    <span class="dos-card-label">{ "Activity warnings" }</span>
                    <div class="dos-task-list">
                        { for view.warnings.iter().map(|warning| html! {
                            <p>{ format!("{}: {}", warning.code, warning.message) }</p>
                        }) }
                    </div>
                </section>
            }
        </>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_reporting_card(
    state: &ReportUploadState,
    on_report_file_change: Callback<Event>,
    on_report_drag_over: Callback<DragEvent>,
    on_report_drop: Callback<DragEvent>,
) -> Html {
    let disabled = matches!(state, ReportUploadState::Rendering { .. });
    html! {
        <section class="dos-card dos-wide-card dos-reporting-card" data-panel="reporting">
            <div class="dos-card-row">
                <span class="dos-card-label">{ "Reporting" }</span>
                <span class="dos-status-pill">{ report_upload_state_label(state) }</span>
            </div>
            <h2>{ "Rebuild performance report" }</h2>
            <p>{ "Drop a DASObjectStore benchmarking JSON artifact to regenerate the formal Mnemosyne PDF report. The PDF downloads automatically when rendering completes." }</p>
            <label
                class={classes!("dos-report-dropzone", disabled.then_some("disabled"))}
                ondragover={on_report_drag_over}
                ondrop={on_report_drop}
            >
                <strong>{ "Drop benchmarking JSON here" }</strong>
                <span>{ "or choose a .json artifact generated by dasobjectstore performance-test" }</span>
                <input
                    type="file"
                    accept=".json,application/json"
                    disabled={disabled}
                    onchange={on_report_file_change}
                />
            </label>
            { render_report_upload_progress(state) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn report_upload_state_label(state: &ReportUploadState) -> &'static str {
    match state {
        ReportUploadState::Idle => "ready",
        ReportUploadState::Rendering { .. } => "rendering",
        ReportUploadState::Downloaded { .. } => "downloaded",
        ReportUploadState::Failed { .. } => "review",
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_report_upload_progress(state: &ReportUploadState) -> Html {
    match state {
        ReportUploadState::Idle => html! {
            <div class="dos-report-progress" data-state="idle">
                <span>{ "Accepted input: DASObjectStore performance-test JSON." }</span>
            </div>
        },
        ReportUploadState::Rendering {
            filename,
            size_label,
        } => html! {
            <div class="dos-report-progress" data-state="rendering">
                <div class="dos-report-progress-meta">
                    <span>{ filename.clone() }</span>
                    <span>{ size_label.clone() }</span>
                </div>
                <div class="dos-report-progress-bar">
                    <span class="dos-report-progress-fill"></span>
                </div>
                <span>{ "Uploading JSON and rendering the formal PDF report." }</span>
            </div>
        },
        ReportUploadState::Downloaded { filename } => html! {
            <div class="dos-report-progress" data-state="downloaded">
                <strong>{ "PDF report prepared" }</strong>
                <span>{ format!("{filename} has been sent to the browser download manager.") }</span>
            </div>
        },
        ReportUploadState::Failed { message } => html! {
            <div class="dos-report-progress" data-state="error">
                <strong>{ "Report rebuild failed" }</strong>
                <span>{ message.clone() }</span>
            </div>
        },
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn download_pdf_to_host(filename: &str, bytes: &[u8]) -> Result<(), String> {
    download_bytes_to_host(filename, bytes, "application/pdf")
}

#[cfg(target_arch = "wasm32")]
pub(super) fn download_bytes_to_host(
    filename: &str,
    bytes: &[u8],
    content_type: &str,
) -> Result<(), String> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array);
    let options = BlobPropertyBag::new();
    options.set_type(content_type);
    let blob = Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|_| "could not prepare browser download blob".to_string())?;
    let url = Url::create_object_url_with_blob(&blob)
        .map_err(|_| "could not create browser download URL".to_string())?;
    let result = (|| {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "browser document is unavailable".to_string())?;
        let anchor = document
            .create_element("a")
            .map_err(|_| "could not create browser download link".to_string())?
            .dyn_into::<HtmlAnchorElement>()
            .map_err(|_| "browser download link is not an anchor".to_string())?;
        anchor.set_href(&url);
        anchor.set_download(filename);
        let body = document
            .body()
            .ok_or_else(|| "browser document body is unavailable".to_string())?;
        body.append_child(&anchor)
            .map_err(|_| "could not attach browser download link".to_string())?;
        anchor.click();
        anchor.remove();
        Ok(())
    })();
    let _ = Url::revoke_object_url(&url);
    result
}

#[cfg(target_arch = "wasm32")]
pub(super) fn format_file_size(bytes: f64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else {
        format!("{:.0} B", bytes)
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_category_card(summary: ActivityCategorySummary) -> Html {
    html! {
        <section class="dos-card dos-activity-card" data-kind={summary.kind.clone()} data-state={summary.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ summary.label.clone() }</span>
                <span class="dos-status-pill">{ summary.state.clone() }</span>
            </div>
            <strong>{ format!("{} active", summary.active_count) }</strong>
            <p>{ summary.description }</p>
            <div class="dos-drive-meta">
                <span>{ format!("{} waiting", summary.waiting_count) }</span>
                <span>{ format!("{} failed", summary.failed_count) }</span>
                <span>{ format!("{} complete", summary.complete_count) }</span>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_task(task: &crate::api::ActivityTaskResponse) -> Html {
    html! {
        <article class="dos-task-card" data-state={task.state.clone()} data-kind={task.kind.clone()}>
            <div>
                <span class="dos-card-label">{ activity_task_kind_label(&task.kind) }</span>
                <strong>{ task.label.clone() }</strong>
                <p>{ format!("{} · updated {}", task.task_id, task.updated_at_utc) }</p>
                if let Some(progress) = &task.progress {
                    <p>{ activity_task_progress_label(progress) }</p>
                }
            </div>
            <span class="dos-status-pill">{ activity_task_state_label(&task.state) }</span>
        </article>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn activity_task_progress_label(
    progress: &crate::api::ActivityTaskProgressResponse,
) -> String {
    let mut parts = Vec::new();
    if !progress.stage.is_empty() {
        parts.push(progress.stage.clone());
    }
    if let Some(percent) = progress.percent_complete {
        parts.push(format!("{percent}%"));
    }
    if progress.work_bytes_total > 0 {
        parts.push(format!(
            "{} / {} bytes",
            progress.work_bytes_done, progress.work_bytes_total
        ));
    } else if progress.work_units_total > 0 {
        parts.push(format!(
            "{} / {} units",
            progress.work_units_done, progress.work_units_total
        ));
    }
    if parts.is_empty() {
        progress
            .message
            .clone()
            .unwrap_or_else(|| "Progress pending".to_string())
    } else {
        parts.join(" · ")
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_activity_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsReadinessSummary {
    pub object_type: String,
    pub label: String,
    pub category: String,
    pub state: String,
    pub state_label: String,
    pub primary_workflow: String,
    pub handoff: String,
    pub metadata: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsContextSummary {
    pub section: String,
    pub label: String,
    pub state: String,
    pub state_label: String,
    pub summary: String,
    pub detail: String,
    pub evidence: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BioinformaticsDerivationSourceSummary {
    pub source_kind: String,
    pub source_id: String,
    pub display_name: String,
    pub object_type: String,
    pub parent: String,
    pub endpoint_export: String,
    pub binding: String,
    pub workflow_roles: String,
    pub evidence: String,
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_readiness_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsReadinessSummary> {
    if !view.readiness_cards.is_empty() {
        return view
            .readiness_cards
            .iter()
            .map(|card| BioinformaticsReadinessSummary {
                object_type: card.object_type.clone(),
                label: card.label.clone(),
                category: card.category.clone(),
                state: card.state.clone(),
                state_label: bioinformatics_readiness_state_label(&card.state).to_string(),
                primary_workflow: card.primary_workflow.clone(),
                handoff: card.handoff.clone(),
                metadata: bioinformatics_metadata_summary(&card.required_metadata),
            })
            .collect();
    }

    view.supported_object_types
        .iter()
        .map(|object_type| BioinformaticsReadinessSummary {
            object_type: object_type.to_ascii_lowercase().replace(['/', '.'], "_"),
            label: object_type.clone(),
            category: "Supported type".to_string(),
            state: "reserved".to_string(),
            state_label: "Reserved".to_string(),
            primary_workflow: "Workflow handoff metadata has not yet been published by the API."
                .to_string(),
            handoff: "Pending workflow contract".to_string(),
            metadata: "metadata contract pending".to_string(),
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn bioinformatics_context_summaries(
    view: &BioinformaticsWorkspaceResponse,
) -> Vec<BioinformaticsContextSummary> {
    let mut summaries = Vec::new();
    summaries.extend(bioinformatics_context_group(
        "Sequencing Runs",
        &view.sequencing_runs,
    ));
    summaries.extend(bioinformatics_context_group(
        "Object Lineage",
        &view.object_lineage,
    ));
    summaries.extend(bioinformatics_context_group(
        "Workflow Handoff",
        &view.workflow_handoffs,
    ));
    summaries.extend(bioinformatics_context_group(
        "Governance",
        &view.governance_bindings,
    ));
    summaries
}
