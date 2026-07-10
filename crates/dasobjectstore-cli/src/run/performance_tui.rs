use super::performance_plan::{
    performance_scenario_bounds, performance_scenario_objective, PerformanceScenarioKind,
    PerformanceWorkload,
};
use super::*;

pub(super) struct PerformanceTuiSnapshot<'a> {
    pub(super) phase: &'a str,
    pub(super) scenario: &'a str,
    pub(super) activity: String,
    pub(super) objective: String,
    pub(super) bounds: String,
    pub(super) scenario_done: usize,
    pub(super) scenario_total: usize,
    pub(super) file_done: u32,
    pub(super) current_file: Option<u32>,
    pub(super) file_count: u32,
    pub(super) processed_bytes: u64,
    pub(super) total_bytes: u64,
    pub(super) hdd_concurrency: usize,
    pub(super) current_rate: Option<f64>,
    pub(super) ssd_write_rate: Option<f64>,
    pub(super) ssd_read_rate: Option<f64>,
    pub(super) hdd_write_rate: Option<f64>,
    pub(super) hdd_disk_rates: Vec<String>,
    pub(super) active_hdd_landing: Vec<String>,
    pub(super) aggregate_rate: Option<f64>,
    pub(super) report_path: &'a Path,
    pub(super) json_path: &'a Path,
}

#[derive(Clone, Copy)]
pub(super) struct PerformanceTuiContext<'a> {
    pub(super) scenario_done: usize,
    pub(super) scenario_total: usize,
    pub(super) report_path: &'a Path,
    pub(super) json_path: &'a Path,
}

pub(super) fn render_performance_tui_snapshot(
    writer: &mut (impl Write + ?Sized),
    snapshot: &PerformanceTuiSnapshot<'_>,
) -> Result<(), CliError> {
    let visible_active_landing_rows = snapshot.active_hdd_landing.len().min(8);
    let hidden_active_landing_rows = snapshot
        .active_hdd_landing
        .len()
        .saturating_sub(visible_active_landing_rows);
    let landing_height = if snapshot.active_hdd_landing.is_empty() {
        5
    } else {
        (3 + visible_active_landing_rows + usize::from(hidden_active_landing_rows > 0)).clamp(6, 12)
            as u16
    };
    let mut area = performance_tui_area();
    area.height = area.height.max(landing_height.saturating_add(31));
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal =
        Terminal::new(backend).expect("test backend terminal creation is infallible");
    let current_fraction = if snapshot.file_count == 0 {
        0.0
    } else {
        f64::from(snapshot.file_done.min(snapshot.file_count)) / f64::from(snapshot.file_count)
    };
    let percent = if snapshot.scenario_total == 0 {
        0
    } else {
        ((((snapshot.scenario_done as f64 + current_fraction) / snapshot.scenario_total as f64)
            * 100.0)
            .round()
            .clamp(0.0, 100.0)) as u16
    };
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(landing_height),
                Constraint::Length(6),
                Constraint::Length(5),
                Constraint::Min(4),
            ])
            .split(frame.area());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "DASObjectStore Performance Test",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!("Phase: {}", snapshot.phase)),
                Line::from(format!("Scenario: {}", snapshot.scenario)),
                Line::from(format!("Now: {}", snapshot.activity)),
            ])
            .block(Block::default().borders(Borders::ALL).title("Context")),
            chunks[0],
        );
        frame.render_widget(
            Gauge::default()
                .block(Block::default().borders(Borders::ALL).title("Scenario Progress"))
                .gauge_style(Style::default().fg(Color::Green))
                .percent(percent),
            chunks[1],
        );
        let rate = snapshot
            .aggregate_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let current_rate = snapshot
            .current_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let inferred_ssd_write_rate = snapshot.ssd_write_rate.or_else(|| {
            if snapshot.phase.contains("staging")
                || snapshot.activity.starts_with("Writing")
                || snapshot.activity.starts_with("Queued")
            {
                snapshot.current_rate
            } else {
                None
            }
        });
        let inferred_ssd_read_rate = snapshot.ssd_read_rate.or_else(|| {
            if snapshot.phase.contains("readback") || snapshot.activity.starts_with("Reading") {
                snapshot.current_rate
            } else {
                None
            }
        });
        let ssd_write_rate = inferred_ssd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let ssd_read_rate = inferred_ssd_read_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_write_rate = snapshot
            .hdd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_disk_rates = if snapshot.hdd_disk_rates.is_empty() {
            "pending".to_string()
        } else {
            snapshot.hdd_disk_rates.join("; ")
        };
        let active_landing_lines = if snapshot.active_hdd_landing.is_empty() {
            vec![Line::from("Active landing: idle")]
        } else {
            std::iter::once(Line::from("Active landing:"))
                .chain(
                    snapshot
                        .active_hdd_landing
                        .iter()
                        .take(visible_active_landing_rows)
                        .map(|line| Line::from(format!("  {line}"))),
                )
                .chain((hidden_active_landing_rows > 0).then(|| {
                    Line::from(format!("  ... {hidden_active_landing_rows} more active transfer(s)"))
                }))
                .collect::<Vec<_>>()
        };
        let current_file = snapshot
            .current_file
            .map(|file| file.to_string())
            .unwrap_or_else(|| "-".to_string());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("Scenarios: {}/{}", snapshot.scenario_done, snapshot.scenario_total)),
                Line::from(format!("Current scenario files: {}/{} (active {})", snapshot.file_done, snapshot.file_count, current_file)),
                Line::from(format!("Current scenario bytes: {}/{}", format_bytes(snapshot.processed_bytes as f64), format_bytes(snapshot.total_bytes as f64))),
                Line::from(format!("HDD concurrency: {}", snapshot.hdd_concurrency)),
                Line::from(format!("Current operation rate: {current_rate}")),
            ])
            .block(Block::default().borders(Borders::ALL).title("Workload")),
            chunks[2],
        );
        frame.render_widget(
            Paragraph::new(active_landing_lines)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title("HDD Landing")),
            chunks[3],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("SSD write rate: {ssd_write_rate}")),
                Line::from(format!("SSD read rate: {ssd_read_rate}")),
                Line::from(format!("HDD aggregate average: {hdd_write_rate}")),
                Line::from(format!("HDD active disk writes: {hdd_disk_rates}")),
                Line::from(format!("Scenario aggregate rate: {rate}")),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Rates")),
            chunks[4],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("Objective: {}", snapshot.objective)),
                Line::from(format!("Bounds: {}", snapshot.bounds)),
                Line::from("Ctrl-C requests cancellation and temporary objectstore cleanup."),
                Line::from("SSD pipeline scenarios stage to SSD while HDD drain workers consume the FIFO queue."),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Scenario Details")),
            chunks[5],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("PDF: {}", snapshot.report_path.display())),
                Line::from(format!("JSON: {}", snapshot.json_path.display())),
            ])
            .block(Block::default().borders(Borders::ALL).title("Artifacts")),
            chunks[6],
        );
    })
    .expect("test backend drawing is infallible");
    write!(writer, "\x1b[2J\x1b[H")?;
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            write!(writer, "{}", buffer[(x, y)].symbol())?;
        }
        if y + 1 < buffer.area.height {
            write!(writer, "\r\n")?;
        }
    }
    writer.flush()?;
    Ok(())
}

pub(super) struct HddDrainTuiState<'a> {
    pub(super) context: PerformanceTuiContext<'a>,
    pub(super) workload: &'a PerformanceWorkload,
    pub(super) kind: PerformanceScenarioKind,
    pub(super) concurrency: usize,
    pub(super) submitted_jobs: usize,
    pub(super) total_jobs: usize,
    pub(super) started_jobs: usize,
    pub(super) completed_jobs: usize,
    pub(super) transferred_bytes: u64,
    pub(super) ssd_read_rate: Option<f64>,
    pub(super) hdd_write_rate: Option<f64>,
    pub(super) hdd_disk_rates: Vec<String>,
    pub(super) active_hdd_landing: Vec<String>,
}

pub(super) fn render_hdd_drain_tui_snapshot(
    writer: &mut impl Write,
    state: HddDrainTuiState<'_>,
) -> Result<(), CliError> {
    let active_jobs = state.started_jobs.saturating_sub(state.completed_jobs);
    let queued_jobs = state.submitted_jobs.saturating_sub(state.started_jobs);
    let pending_submission = state.total_jobs.saturating_sub(state.submitted_jobs);
    let total_bytes = state.workload.total_bytes().saturating_mul(
        state
            .total_jobs
            .checked_div(state.workload.file_count().max(1) as usize)
            .unwrap_or(1)
            .max(1) as u64,
    );
    render_performance_tui_snapshot(
        writer,
        &PerformanceTuiSnapshot {
            phase: "hdd-drain active",
            scenario: state.kind.as_str(),
            activity: format!(
                "HDD drain copy jobs: drained {}/{}, draining {}, queued {}, pending submission {}",
                state.completed_jobs,
                state.total_jobs,
                active_jobs,
                queued_jobs,
                pending_submission
            ),
            objective: performance_scenario_objective(state.kind, state.concurrency),
            bounds: performance_scenario_bounds(state.workload, state.kind, state.concurrency),
            scenario_done: state.context.scenario_done,
            scenario_total: state.context.scenario_total,
            file_done: state.completed_jobs.min(u32::MAX as usize) as u32,
            current_file: None,
            file_count: state.total_jobs.min(u32::MAX as usize) as u32,
            processed_bytes: state.transferred_bytes,
            total_bytes,
            hdd_concurrency: state.concurrency,
            current_rate: state.hdd_write_rate,
            ssd_write_rate: None,
            ssd_read_rate: state.ssd_read_rate,
            hdd_write_rate: state.hdd_write_rate,
            hdd_disk_rates: state.hdd_disk_rates,
            active_hdd_landing: state.active_hdd_landing,
            aggregate_rate: state.hdd_write_rate,
            report_path: state.context.report_path,
            json_path: state.context.json_path,
        },
    )
}

pub(super) fn performance_tui_area() -> Rect {
    let env_size = std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<u16>().ok())
        .zip(
            std::env::var("LINES")
                .ok()
                .and_then(|lines| lines.parse::<u16>().ok()),
        );
    let (width, height) = env_size
        .or_else(performance_terminal_size_from_ioctl)
        .unwrap_or((110, 24));
    Rect::new(0, 0, width.max(80), height.max(32))
}

#[cfg(unix)]
pub(super) fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    let stdout = std::io::stdout();
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) };
    (result == 0 && winsize.ws_col > 0 && winsize.ws_row > 0)
        .then_some((winsize.ws_col, winsize.ws_row))
}

#[cfg(not(unix))]
pub(super) fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    None
}
