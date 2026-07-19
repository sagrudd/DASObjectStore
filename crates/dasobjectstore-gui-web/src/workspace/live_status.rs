use super::*;
#[cfg(target_arch = "wasm32")]
use crate::api::{
    LiveStatusHddTransferResponse, LiveStatusHostResponse, LiveStatusProgressResponse,
};
use crate::api::{LiveStatusTracePointResponse, LiveStatusWorkspaceResponse};

pub(super) const LIVE_STATUS_REFRESH_MS: u32 = 1_000;
pub(super) const LIVE_STATUS_TRACE_POINTS: usize = 60;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq, Properties)]
pub struct LiveStatusPageProps {
    pub api_base_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, PartialEq)]
enum LiveStatusViewState {
    Loading,
    Current(LiveStatusWorkspaceResponse),
    Stale(LiveStatusWorkspaceResponse, String),
    Unavailable(String),
}

#[cfg(target_arch = "wasm32")]
#[function_component(LiveStatusPage)]
pub fn live_status_page(props: &LiveStatusPageProps) -> Html {
    use std::{cell::Cell, rc::Rc};

    let api_path = live_status_workspace_api_path(&props.api_base_path);
    let state = use_state(|| LiveStatusViewState::Loading);
    let selected_transfer = use_state(|| None::<String>);
    let in_flight = use_mut_ref(|| false);
    let request_sequence = use_mut_ref(|| 0_u64);
    let accepted_sequence = use_mut_ref(|| 0_u64);

    {
        let api_path = api_path.clone();
        let state = state.clone();
        let in_flight = in_flight.clone();
        let request_sequence = request_sequence.clone();
        let accepted_sequence = accepted_sequence.clone();
        use_effect_with(api_path.clone(), move |_| {
            let mounted = Rc::new(Cell::new(true));
            let refresh = Rc::new({
                let api_path = api_path.clone();
                let state = state.clone();
                let in_flight = in_flight.clone();
                let request_sequence = request_sequence.clone();
                let accepted_sequence = accepted_sequence.clone();
                let mounted = mounted.clone();
                move || {
                    // Single-flight polling prevents slow requests from accumulating and
                    // sequence rejection prevents older snapshots from moving the UI back.
                    if *in_flight.borrow() {
                        return;
                    }
                    *in_flight.borrow_mut() = true;
                    *request_sequence.borrow_mut() += 1;
                    let request_id = *request_sequence.borrow();
                    let api_path = api_path.clone();
                    let state = state.clone();
                    let in_flight = in_flight.clone();
                    let accepted_sequence = accepted_sequence.clone();
                    let mounted = mounted.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let result = crate::api::get_live_status_workspace(&api_path).await;
                        if !mounted.get() {
                            return;
                        }
                        *in_flight.borrow_mut() = false;
                        match result {
                            Ok(mut snapshot)
                                if snapshot.sequence >= *accepted_sequence.borrow() =>
                            {
                                *accepted_sequence.borrow_mut() = snapshot.sequence;
                                let previous = match &*state {
                                    LiveStatusViewState::Current(previous)
                                    | LiveStatusViewState::Stale(previous, _) => Some(previous),
                                    _ => None,
                                };
                                snapshot.ssd_trace = append_trace(
                                    previous.map(|value| value.ssd_trace.as_slice()),
                                    snapshot.sequence,
                                    snapshot.aggregate.ssd_write_bytes_per_second,
                                );
                                snapshot.hdd_trace = append_trace(
                                    previous.map(|value| value.hdd_trace.as_slice()),
                                    snapshot.sequence,
                                    snapshot.aggregate.hdd_write_bytes_per_second,
                                );
                                state.set(LiveStatusViewState::Current(snapshot));
                            }
                            Ok(_) => { /* reject an out-of-order daemon snapshot */ }
                            Err(error) => match &*state {
                                LiveStatusViewState::Current(snapshot)
                                | LiveStatusViewState::Stale(snapshot, _) => {
                                    state.set(LiveStatusViewState::Stale(
                                        snapshot.clone(),
                                        format!("Refresh {request_id} failed: {}", error.message),
                                    ));
                                }
                                _ => state.set(LiveStatusViewState::Unavailable(error.message)),
                            },
                        }
                    });
                }
            });
            refresh();
            let interval = Interval::new(LIVE_STATUS_REFRESH_MS, {
                let refresh = refresh.clone();
                move || refresh()
            });
            move || {
                mounted.set(false);
                drop(interval);
            }
        });
    }

    html! {
        <section class="dos-page dos-live-status" data-page="live-status" data-api-route={api_path}>
            <PageHeader
                eyebrow="Current operations"
                title="Live Status"
                summary="A calm, continuously updated view of clients, ingress, SSD staging, and durable HDD settlement."
            />
            { render_live_status(&state, &selected_transfer) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_live_status(
    state: &LiveStatusViewState,
    selected_transfer: &UseStateHandle<Option<String>>,
) -> Html {
    match state {
        LiveStatusViewState::Loading => {
            status_message("Connecting", "Reading live daemon evidence…")
        }
        LiveStatusViewState::Unavailable(message) => status_message("Unavailable", message),
        LiveStatusViewState::Current(snapshot) => {
            render_snapshot(snapshot, None, selected_transfer)
        }
        LiveStatusViewState::Stale(snapshot, message) => {
            render_snapshot(snapshot, Some(message), selected_transfer)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn status_message(state: &str, message: &str) -> Html {
    html! { <section class="dos-card dos-live-status-message" role="status"><strong>{ state }</strong><p>{ message }</p></section> }
}

#[cfg(target_arch = "wasm32")]
fn render_snapshot(
    snapshot: &LiveStatusWorkspaceResponse,
    stale_message: Option<&String>,
    selected_transfer: &UseStateHandle<Option<String>>,
) -> Html {
    let active = snapshot.ssd_ingests.len() + snapshot.hdd_transfers.len();
    let ingress_rate = snapshot
        .ssd_trace
        .last()
        .map(|point| point.bytes_per_second);
    let settlement_rate = snapshot
        .hdd_trace
        .last()
        .map(|point| point.bytes_per_second);
    let selected = selected_transfer
        .as_ref()
        .and_then(|id| snapshot.ssd_ingests.iter().find(|path| &path.job_id == id));
    html! {
        <>
            <section class="dos-live-evidence" aria-label="Live evidence">
                <div><span>{ "Status" }</span><strong class={classes!(stale_message.is_some().then_some("is-stale"))}>{ if stale_message.is_some() { "Reconnecting" } else { "Live" } }</strong></div>
                <div><span>{ "Connected hosts" }</span><strong>{ snapshot.aggregate.connected_hosts }</strong></div>
                <div><span>{ "Active paths" }</span><strong>{ active }</strong></div>
                <div><span>{ "SSD ingress" }</span><strong>{ format_rate(ingress_rate) }</strong></div>
                <div><span>{ "HDD settlement" }</span><strong>{ format_rate(settlement_rate) }</strong></div>
                <div><span>{ "Observed" }</span><strong>{ snapshot.generated_at_utc.as_deref().map(display_time).unwrap_or_else(|| "Unavailable".into()) }</strong></div>
            </section>
            if let Some(message) = stale_message {
                <p class="dos-live-reconnect" role="status">{ format!("Live connection interrupted. Last good state retained. {message}") }</p>
            }
            <section class="dos-live-primary">
                <div class="dos-card dos-live-paths">
                    <div class="dos-card-row"><div><span class="dos-card-label">{ "Active data paths" }</span><h2>{ "Host → ObjectStore" }</h2></div><span class="dos-status-pill">{ format!("{active} active") }</span></div>
                    if snapshot.ssd_ingests.is_empty() && snapshot.hdd_transfers.is_empty() {
                        <p class="dos-empty-state">{ "No active transfers are reported. Connected hosts remain visible below." }</p>
                    } else {
                        <div class="dos-live-path-list" role="list">
                            { for snapshot.ssd_ingests.iter().map(|path| render_data_path(path, selected_transfer)) }
                            { for snapshot.hdd_transfers.iter().map(render_hdd_path) }
                        </div>
                    }
                </div>
                { render_detail(selected, selected_transfer) }
            </section>
            <section class="dos-live-traces">
                { render_trace("SSD ingress", "Client bytes entering the staging tier", &snapshot.ssd_trace) }
                { render_trace("HDD settlement", "Verified bytes becoming durable", &snapshot.hdd_trace) }
            </section>
            <section class="dos-live-lower">
                { render_hosts(&snapshot.hosts) }
                { render_waiting(snapshot) }
            </section>
        </>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_data_path(
    path: &LiveStatusProgressResponse,
    selected: &UseStateHandle<Option<String>>,
) -> Html {
    let id = path.job_id.clone();
    let selected_for_click = selected.clone();
    let percent = path
        .bytes_total
        .filter(|total| *total > 0)
        .map(|total| ((path.bytes_done as f64 / total as f64) * 100.0).clamp(0.0, 100.0));
    html! {
        <button key={path.job_id.clone()} type="button" class="dos-live-path" role="listitem" onclick={Callback::from(move |_| selected_for_click.set(Some(id.clone())))}>
            <span class="dos-live-path__route"><strong>{ path.host.as_deref().unwrap_or("Host unavailable") }</strong><span aria-hidden="true">{ "→" }</span><strong>{ &path.store_id }</strong></span>
            <span class="dos-live-path__work"><span>{ stage_label(path.pipeline_stage.as_deref().unwrap_or(&path.state)) }</span><strong title={path.current_item.clone()}>{ path.current_item.as_deref().unwrap_or("Object name unavailable") }</strong></span>
            <span class="dos-live-path__measure"><strong>{ format_progress(path.bytes_done, path.bytes_total) }</strong><span>{ format_rate(Some(path.bytes_per_second as f64)) }</span></span>
            <span class="dos-live-progress" aria-label={percent.map(|value| format!("{value:.0}% complete")).unwrap_or_else(|| "Total size unavailable".into())}><i style={format!("width: {:.1}%", percent.unwrap_or(0.0))}></i></span>
        </button>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_hdd_path(path: &LiveStatusHddTransferResponse) -> Html {
    let percent =
        (path.bytes_done as f64 / path.bytes_total.max(1) as f64 * 100.0).clamp(0.0, 100.0);
    html! {
        <div key={format!("{}-{}-{}", path.job_id, path.disk_id, path.copy_number)} class="dos-live-path" role="listitem">
            <span class="dos-live-path__route"><strong>{ format!("HDD copy {}", path.copy_number) }</strong><span aria-hidden="true">{ "→" }</span><strong>{ &path.store_id }</strong></span>
            <span class="dos-live-path__work"><span>{ format!("{} · {}", stage_label(&path.phase), path.disk_id) }</span><strong>{ &path.current_item }</strong></span>
            <span class="dos-live-path__measure"><strong>{ format_progress(path.bytes_done, Some(path.bytes_total)) }</strong><span>{ format_rate(Some(path.bytes_per_second as f64)) }</span></span>
            <span class="dos-live-progress" aria-label={format!("{percent:.0}% complete")}><i style={format!("width: {percent:.1}%")}></i></span>
        </div>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_trace(title: &str, summary: &str, points: &[LiveStatusTracePointResponse]) -> Html {
    let values: Vec<f64> = points
        .iter()
        .rev()
        .take(LIVE_STATUS_TRACE_POINTS)
        .map(|point| point.bytes_per_second.max(0.0))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let max = values.iter().copied().fold(1.0_f64, f64::max);
    let denominator = values.len().saturating_sub(1).max(1) as f64;
    let polyline = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            format!(
                "{:.1},{:.1}",
                index as f64 / denominator * 600.0,
                104.0 - value / max * 88.0
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    html! {
        <section class="dos-card dos-live-trace">
            <div class="dos-card-row"><div><span class="dos-card-label">{ title }</span><p>{ summary }</p></div><strong>{ format_rate(values.last().copied()) }</strong></div>
            <svg viewBox="0 0 600 120" role="img" aria-label={format!("{title}, trailing 60 seconds")} preserveAspectRatio="none"><path d="M0 104 H600" class="dos-live-trace__baseline"/><polyline points={polyline} class="dos-live-trace__line"/></svg>
            <div class="dos-live-trace__axis"><span>{ "60 s ago" }</span><span>{ "now" }</span></div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_hosts(hosts: &[LiveStatusHostResponse]) -> Html {
    html! {
        <section class="dos-card dos-live-hosts">
            <div class="dos-card-row"><div><span class="dos-card-label">{ "Connections" }</span><h2>{ "Connected hosts" }</h2></div><span class="dos-status-pill">{ hosts.len() }</span></div>
            <div class="dos-live-host-list">{ for hosts.iter().map(|host| html! {
                <div key={host.display_name.clone()}>
                    <span class="dos-presence" data-state="active"></span>
                    <strong>{ &host.display_name }</strong>
                    <span>{ if host.actors.is_empty() { format!("{} active", host.active_ingests) } else { format!("{} · {} active", host.actors.join(", "), host.active_ingests) } }</span>
                    <time>{ if host.object_stores.is_empty() { "No active ObjectStore".into() } else { host.object_stores.join(", ") } }</time>
                </div>
            }) }</div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_waiting(snapshot: &LiveStatusWorkspaceResponse) -> Html {
    html! {
        <section class="dos-card dos-live-waiting">
            <div class="dos-card-row"><div><span class="dos-card-label">{ "Queue & attention" }</span><h2>{ "Waiting work" }</h2></div><span class="dos-status-pill">{ snapshot.recent.iter().filter(|item| item.state == "queued").count() + snapshot.warnings.len() }</span></div>
            if !snapshot.recent.iter().any(|item| item.state == "queued") && snapshot.warnings.is_empty() {
                <p class="dos-empty-state">{ "Nothing is waiting and no live warnings are reported." }</p>
            } else {
                <div class="dos-live-wait-list">
                    { for snapshot.recent.iter().filter(|item| item.state == "queued").map(|item| html! {
                        <div key={item.job_id.clone()}>
                            <strong>{ &item.store_id }</strong>
                            <span>{ stage_label(&item.state) }</span>
                            <small>{ "Awaiting capacity" }</small>
                        </div>
                    }) }
                    { for snapshot.warnings.iter().map(|warning| html! {
                        <div key={warning.code.clone()} data-state="warning"><strong>{ &warning.code }</strong><span>{ &warning.message }</span></div>
                    }) }
                </div>
            }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
fn render_detail(
    path: Option<&LiveStatusProgressResponse>,
    selected: &UseStateHandle<Option<String>>,
) -> Html {
    let close = {
        let selected = selected.clone();
        Callback::from(move |_| selected.set(None))
    };
    path.map(|path| html! { <aside class="dos-live-detail" aria-label="Transfer detail"><button type="button" class="dos-icon-button" onclick={close} aria-label="Close transfer detail">{ "×" }</button><span class="dos-card-label">{ "Transfer detail" }</span><h2>{ &path.store_id }</h2><dl><dt>{ "Source" }</dt><dd>{ path.host.as_deref().unwrap_or("Host unavailable") }</dd><dt>{ "Stage" }</dt><dd>{ stage_label(path.pipeline_stage.as_deref().unwrap_or(&path.state)) }</dd><dt>{ "Object" }</dt><dd>{ path.current_item.as_deref().unwrap_or("Unavailable") }</dd><dt>{ "Transferred" }</dt><dd>{ format_progress(path.bytes_done, path.bytes_total) }</dd><dt>{ "Files" }</dt><dd>{ path.files_total.map(|total| format!("{} / {total}", path.files_done)).unwrap_or_else(|| path.files_done.to_string()) }</dd><dt>{ "Current rate" }</dt><dd>{ format_rate(Some(path.bytes_per_second as f64)) }</dd><dt>{ "Updated" }</dt><dd>{ display_time(&path.updated_at_utc) }</dd></dl></aside> }).unwrap_or_default()
}

fn append_trace(
    previous: Option<&[LiveStatusTracePointResponse]>,
    sequence: u64,
    bytes_per_second: u64,
) -> Vec<LiveStatusTracePointResponse> {
    let mut trace = previous.unwrap_or_default().to_vec();
    if trace.last().is_none_or(|point| point.sequence != sequence) {
        trace.push(LiveStatusTracePointResponse {
            sequence,
            bytes_per_second: bytes_per_second as f64,
        });
    }
    if trace.len() > LIVE_STATUS_TRACE_POINTS {
        trace.drain(..trace.len() - LIVE_STATUS_TRACE_POINTS);
    }
    trace
}

fn stage_label(stage: &str) -> String {
    stage
        .replace(['_', '-'], " ")
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn display_time(timestamp: &str) -> String {
    timestamp
        .split('T')
        .nth(1)
        .unwrap_or(timestamp)
        .trim_end_matches('Z')
        .chars()
        .take(8)
        .collect()
}
fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds.div_ceil(60))
    }
}
#[cfg(target_arch = "wasm32")]
fn format_progress(done: u64, total: Option<u64>) -> String {
    total
        .map(|total| {
            format!(
                "{} / {}",
                format_bytes(done as f64),
                format_bytes(total as f64)
            )
        })
        .unwrap_or_else(|| format_bytes(done as f64))
}
#[cfg(target_arch = "wasm32")]
fn format_rate(rate: Option<f64>) -> String {
    rate.map(|value| format!("{}/s", format_bytes(value)))
        .unwrap_or_else(|| "Unavailable".into())
}
#[cfg(target_arch = "wasm32")]
fn format_bytes(bytes: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_polling_contract_are_stable() {
        assert_eq!(
            live_status_workspace_api_path("/api/v1/"),
            "/api/v1/workspaces/live-status"
        );
        assert_eq!(LIVE_STATUS_REFRESH_MS, 1_000);
        assert_eq!(LIVE_STATUS_TRACE_POINTS, 60);
    }

    #[test]
    fn presentation_helpers_are_truthful() {
        assert_eq!(stage_label("waiting_for_capacity"), "Waiting For Capacity");
        assert_eq!(display_time("2026-07-19T10:11:12Z"), "10:11:12");
        assert_eq!(format_duration(61), "2 min");
    }

    #[test]
    fn client_trace_is_bounded_and_deduplicates_daemon_sequences() {
        let mut trace = Vec::new();
        for sequence in 0..70 {
            trace = append_trace(Some(&trace), sequence, sequence * 10);
        }
        assert_eq!(trace.len(), LIVE_STATUS_TRACE_POINTS);
        let unchanged = append_trace(Some(&trace), 69, 999);
        assert_eq!(unchanged, trace);
        assert_eq!(trace.first().map(|point| point.sequence), Some(10));
    }

    #[test]
    fn workspace_contract_decodes_gui_api_projection() {
        let value = serde_json::json!({
            "schema_version": 1, "availability": "available", "sequence": 4,
            "generated_at_utc": "2026-07-19T10:11:12Z", "suggested_refresh_millis": 1000,
            "aggregate": {"connected_hosts": 1, "active_stores": 1, "active_ingests": 1,
                "source_read_bytes_per_second": 10, "ssd_write_bytes_per_second": 9,
                "hdd_write_bytes_per_second": 8, "active_hdd_transfers": 1},
            "hosts": [{"display_name": "sequencer", "active_ingests": 1, "object_stores": ["reads"]}],
            "store_writers": [{"store_id": "reads", "hosts": ["sequencer"], "active_ingests": 1}],
            "ssd_ingests": [{"job_id": "job-1", "store_id": "reads", "host": "sequencer",
                "state": "ssd_ingest", "pipeline_stage": "receiving", "current_item": "sample.pod5",
                "bytes_done": 4, "bytes_total": 10, "files_done": 0, "files_total": 1,
                "bytes_per_second": 9, "updated_at_utc": "2026-07-19T10:11:12Z"}],
            "hdd_transfers": [{"job_id": "job-1", "store_id": "reads", "disk_id": "disk-1",
                "copy_number": 1, "current_item": "sample.pod5", "bytes_done": 3,
                "bytes_total": 10, "bytes_per_second": 8, "phase": "writing"}],
            "recent": [], "warnings": []
        });
        let decoded: LiveStatusWorkspaceResponse = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.sequence, 4);
        assert_eq!(decoded.hosts[0].display_name, "sequencer");
        assert_eq!(decoded.hdd_transfers[0].disk_id, "disk-1");
    }
}
