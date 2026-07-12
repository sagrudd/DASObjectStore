#[cfg(target_arch = "wasm32")]
use super::*;

#[cfg(target_arch = "wasm32")]
#[function_component(HomeDashboard)]
pub fn home_dashboard(props: &HomeDashboardProps) -> Html {
    let selected_telemetry_window = use_state(|| "one_hour".to_string());
    let api_path = home_dashboard_api_path_with_window(
        &props.api_base_path,
        selected_telemetry_window.as_str(),
    );
    let dashboard_state = use_state(|| ApiLoadState::<HomeDashboardResponse>::Loading);

    {
        let api_path = api_path.clone();
        let dashboard_state = dashboard_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            wasm_bindgen_futures::spawn_local(async move {
                dashboard_state.set(page_load_state_from_result(
                    crate::api::get_home_dashboard(&path).await,
                    |_| None,
                ));
            });
            || ()
        });
    }

    {
        let api_path = api_path.clone();
        let dashboard_state = dashboard_state.clone();
        use_effect_with(api_path.clone(), move |path| {
            let path = path.clone();
            let interval = Interval::new(HOME_DASHBOARD_REFRESH_MS, move || {
                let path = path.clone();
                let dashboard_state = dashboard_state.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    dashboard_state.set(page_load_state_from_result(
                        crate::api::get_home_dashboard(&path).await,
                        |_| None,
                    ));
                });
            });
            move || drop(interval)
        });
    }

    html! {
        <section class="dos-page" data-page="home" data-api-route={api_path}>
            <PageHeader
                eyebrow="Appliance"
                title="Home"
                summary="Current operating posture for local storage, ingress, and object service."
            />
            { render_home_dashboard_state(&*dashboard_state, selected_telemetry_window) }
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_home_dashboard_state(
    state: &ApiLoadState<HomeDashboardResponse>,
    selected_telemetry_window: UseStateHandle<String>,
) -> Html {
    match state {
        ApiLoadState::Loading => html! {
            <>
                <div class="dos-metric-grid">
                    { for home_dashboard_loading_cards().into_iter().map(render_loading_metric_card) }
                </div>
                <section class="dos-card dos-wide-card dos-loading-card">
                    <span class="dos-card-label">{ "Loading" }</span>
                    <h2>{ "Loading live dashboard telemetry." }</h2>
                    <p>{ "The Web console is requesting daemon-backed drive, capacity, throughput, memory, and SMART state." }</p>
                </section>
            </>
        },
        ApiLoadState::Success(view) | ApiLoadState::StaleData { value: view, .. } => html! {
            <>
                { render_home_telemetry_window_control(view, selected_telemetry_window) }
                <div class="dos-metric-grid">
                    { for home_dashboard_metrics(view).into_iter().map(render_metric_card) }
                </div>
                { render_home_throughput_chart(view) }
                <div class="dos-attention-grid">
                    { for home_dashboard_attention(view).into_iter().map(render_attention_card) }
                </div>
            </>
        },
        ApiLoadState::Empty(message) => {
            render_home_state_message("Empty", "No dashboard data", message)
        }
        ApiLoadState::PermissionDenied(message) => render_home_state_message(
            "Permission denied",
            "Home dashboard requires an authenticated session",
            message,
        ),
        ApiLoadState::TransportError(message) => {
            render_home_state_message("Error", "Unable to load Home dashboard", message)
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_home_throughput_chart(view: &HomeDashboardResponse) -> Html {
    let points = home_throughput_chart_points(view);
    let first_label = points
        .first()
        .map(|point| point.date.as_str())
        .unwrap_or("No samples");
    let last_label = points
        .last()
        .map(|point| point.date.as_str())
        .unwrap_or("Awaiting telemetry");
    let max_label = home_throughput_chart_max_tib(&points);
    let polyline = home_throughput_chart_polyline(&points);
    let has_points = !points.is_empty();
    let source_label = home_throughput_source_label(&view.throughput_7d.source);
    let source_class = home_throughput_source_class(&view.throughput_7d.source);

    html! {
        <section
            class="dos-card dos-home-chart-card"
            data-throughput-source={view.throughput_7d.source.clone()}
            aria-label="Home throughput telemetry chart"
        >
            <div class="dos-card-row">
                <div>
                    <span class="dos-card-label">{ "Telemetry chart" }</span>
                    <h2>{ "Throughput" }</h2>
                </div>
                <div class="dos-home-chart-badges">
                    <span class="dos-status-pill">{ &view.telemetry_window.selected_label }</span>
                    <span
                        class={format!("dos-status-pill dos-telemetry-source {source_class}")}
                        aria-label={format!("Throughput source: {source_label}")}
                    >
                        { source_label }
                    </span>
                </div>
            </div>
            {
                view.throughput_7d.message.as_deref().map(|message| html! {
                    <p class="dos-chart-message">{ message }</p>
                }).unwrap_or_default()
            }
            <div class="dos-home-chart-frame">
                <svg
                    class="dos-home-throughput-chart"
                    viewBox={format!(
                        "0 0 {:.0} {:.0}",
                        HOME_THROUGHPUT_CHART_WIDTH,
                        HOME_THROUGHPUT_CHART_HEIGHT
                    )}
                    role="img"
                    aria-label="Ingest throughput over selected telemetry window"
                >
                    <line
                        class="dos-chart-axis"
                        x1={HOME_THROUGHPUT_CHART_LEFT.to_string()}
                        y1={HOME_THROUGHPUT_CHART_BOTTOM.to_string()}
                        x2={HOME_THROUGHPUT_CHART_RIGHT.to_string()}
                        y2={HOME_THROUGHPUT_CHART_BOTTOM.to_string()}
                    />
                    <line
                        class="dos-chart-axis"
                        x1={HOME_THROUGHPUT_CHART_LEFT.to_string()}
                        y1={HOME_THROUGHPUT_CHART_TOP.to_string()}
                        x2={HOME_THROUGHPUT_CHART_LEFT.to_string()}
                        y2={HOME_THROUGHPUT_CHART_BOTTOM.to_string()}
                    />
                    <line
                        class="dos-chart-gridline"
                        x1={HOME_THROUGHPUT_CHART_LEFT.to_string()}
                        y1={HOME_THROUGHPUT_CHART_TOP.to_string()}
                        x2={HOME_THROUGHPUT_CHART_RIGHT.to_string()}
                        y2={HOME_THROUGHPUT_CHART_TOP.to_string()}
                    />
                    {
                        if has_points {
                            html! {
                                <>
                                    <polyline
                                        class={format!("dos-chart-line {source_class}")}
                                        points={polyline}
                                    />
                                    { for points.iter().map(|point| html! {
                                        <circle
                                            class="dos-chart-point"
                                            cx={format!("{:.1}", point.x)}
                                            cy={format!("{:.1}", point.y)}
                                            r="3"
                                        />
                                    }) }
                                </>
                            }
                        } else {
                            html! {
                                <text class="dos-chart-empty" x="320" y="88" text-anchor="middle">
                                    { "Awaiting throughput samples" }
                                </text>
                            }
                        }
                    }
                    <text class="dos-chart-label" x="8" y="28">{ max_label }</text>
                    <text class="dos-chart-label" x="8" y="148">{ "0 TiB" }</text>
                    <text class="dos-chart-label" x="48" y="170">{ first_label }</text>
                    <text class="dos-chart-label" x="616" y="170" text-anchor="end">{ last_label }</text>
                </svg>
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_home_telemetry_window_control(
    view: &HomeDashboardResponse,
    selected_telemetry_window: UseStateHandle<String>,
) -> Html {
    let current_window = (*selected_telemetry_window).clone();
    html! {
        <section class="dos-home-telemetry-toolbar" aria-label="Home telemetry time window">
            <span class="dos-card-label">{ "Telemetry window" }</span>
            <div class="dos-window-segments" role="group" aria-label="Home telemetry time window">
                { for view.telemetry_window.options.iter().map(|option| {
                    let value = option.value.clone();
                    let selected_telemetry_window = selected_telemetry_window.clone();
                    let is_selected = option.value == current_window;
                    let class = if is_selected {
                        "dos-window-segment dos-window-segment-selected"
                    } else {
                        "dos-window-segment"
                    };
                    html! {
                        <button
                            type="button"
                            class={class}
                            aria-pressed={is_selected.to_string()}
                            onclick={Callback::from(move |_| selected_telemetry_window.set(value.clone()))}
                        >
                            { &option.label }
                        </button>
                    }
                }) }
            </div>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn home_dashboard_loading_cards() -> Vec<&'static str> {
    vec![
        "Drive inventory",
        "DAS enclosures",
        "Capacity",
        "Throughput",
        "Disk IO",
        "CPU",
        "Logged-in users",
        "S3 service",
        "Memory stress",
        "SMART warnings",
        "ObjectStores",
    ]
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_loading_metric_card(label: &'static str) -> Html {
    html! {
        <section class="dos-card dos-metric-card dos-loading-card" data-state="loading">
            <div class="dos-card-row">
                <span class="dos-card-label">{ label }</span>
                <span class="dos-status-pill">{ "Loading" }</span>
            </div>
            <strong>{ "..." }</strong>
            <p>{ "Awaiting live daemon payload." }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_metric_card(metric: DashboardMetric) -> Html {
    html! {
        <section class="dos-card dos-metric-card" data-state={metric.state.clone()}>
            <div class="dos-card-row">
                <span class="dos-card-label">{ metric.label }</span>
                <span class="dos-status-pill">{ metric.state }</span>
            </div>
            <strong>{ metric.value }</strong>
            <p>{ metric.detail }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_attention_card(item: DashboardAttentionItem) -> Html {
    html! {
        <section class="dos-card dos-wide-card" data-state={item.state.clone()}>
            <span class="dos-card-label">{ "Attention" }</span>
            <h2>{ item.title }</h2>
            <p>{ item.detail }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) fn render_home_state_message(label: &str, title: &str, message: &str) -> Html {
    html! {
        <section class="dos-card dos-wide-card">
            <span class="dos-card-label">{ label }</span>
            <h2>{ title }</h2>
            <p>{ message }</p>
        </section>
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Properties)]
pub struct EnclosuresPageProps {
    pub api_base_path: String,
}
