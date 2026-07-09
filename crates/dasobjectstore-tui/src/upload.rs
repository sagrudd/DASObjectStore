use crate::{format_rate_label, format_size_label};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use dasobjectstore_daemon::{
    DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestStage, DaemonSsdPressure,
    SubmitIngestFilesResponse,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Terminal,
};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadTuiContext {
    pub endpoint: String,
    pub source_path: PathBuf,
    pub object_type: String,
    pub conflict_policy: String,
    pub dry_run: bool,
}

#[derive(Debug)]
pub struct UploadTui<'a, W>
where
    W: Write,
{
    terminal: Terminal<CrosstermBackend<&'a mut W>>,
    context: UploadTuiContext,
    started_at: Instant,
    last_render_at: Instant,
    last_rate_sample: Option<RateSample>,
    upload_rate_bytes_per_second: Option<u64>,
    active: bool,
    terminal_controls: bool,
    last_event: Option<DaemonIngestProgressEvent>,
}

#[derive(Clone, Copy, Debug)]
struct RateSample {
    at: Instant,
    source_bytes_done: u64,
}

impl<'a, W> UploadTui<'a, W>
where
    W: Write,
{
    pub fn start(writer: &'a mut W, context: UploadTuiContext) -> io::Result<Self> {
        execute!(writer, EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(writer);
        let terminal = Terminal::new(backend)?;
        Self::start_with_terminal(terminal, context, true)
    }

    #[doc(hidden)]
    pub fn start_with_fixed_viewport(
        writer: &'a mut W,
        context: UploadTuiContext,
        area: ratatui::layout::Rect,
    ) -> io::Result<Self> {
        let backend = CrosstermBackend::new(writer);
        let terminal = Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(area),
            },
        )?;
        Self::start_with_terminal(terminal, context, false)
    }

    fn start_with_terminal(
        terminal: Terminal<CrosstermBackend<&'a mut W>>,
        context: UploadTuiContext,
        terminal_controls: bool,
    ) -> io::Result<Self> {
        let mut tui = Self {
            terminal,
            context,
            started_at: Instant::now(),
            last_render_at: Instant::now(),
            last_rate_sample: None,
            upload_rate_bytes_per_second: None,
            active: true,
            terminal_controls,
            last_event: None,
        };
        if tui.terminal_controls {
            tui.terminal.clear()?;
        }
        tui.render_frame(None, "waiting for daemon progress")?;
        Ok(tui)
    }

    pub fn render_progress(&mut self, event: DaemonIngestProgressEvent) -> io::Result<()> {
        self.update_rate(&event);
        self.render_frame(
            Some(&event),
            event.message.as_deref().unwrap_or("upload running"),
        )?;
        self.last_event = Some(event);
        Ok(())
    }

    pub fn render_heartbeat(&mut self) -> io::Result<()> {
        if self.last_render_at.elapsed() < Duration::from_millis(500) {
            return Ok(());
        }
        let last_event = self.last_event.clone();
        let status = if last_event.is_some() {
            "waiting for next daemon progress frame"
        } else {
            "waiting for daemon progress"
        };
        self.render_frame(last_event.as_ref(), status)
    }

    pub fn finish(mut self, response: &SubmitIngestFilesResponse) -> io::Result<()> {
        let message = if response.dry_run {
            "dry run complete"
        } else {
            "upload complete"
        };
        let last_event = self.last_event.clone();
        self.render_frame(last_event.as_ref(), message)?;
        self.restore_terminal()?;
        writeln!(
            self.terminal.backend_mut(),
            "Final response: job={} accepted_at_utc={} dry_run={}",
            response.job_id,
            response.accepted_at_utc,
            response.dry_run
        )
    }

    pub fn fail(mut self, error: impl Display) -> io::Result<()> {
        let message = format!("upload failed: {error}");
        let last_event = self.last_event.clone();
        self.render_frame(last_event.as_ref(), &message)?;
        self.restore_terminal()?;
        writeln!(self.terminal.backend_mut(), "{message}")
    }

    pub fn cancel(mut self, message: impl Display) -> io::Result<()> {
        let message = format!("upload cancelled: {message}");
        let last_event = self.last_event.clone();
        self.render_frame(last_event.as_ref(), &message)?;
        self.restore_terminal()?;
        writeln!(self.terminal.backend_mut(), "{message}")
    }

    fn render_frame(
        &mut self,
        event: Option<&DaemonIngestProgressEvent>,
        status: &str,
    ) -> io::Result<()> {
        self.last_render_at = Instant::now();
        let context = self.context.clone();
        let elapsed = self.started_at.elapsed().as_secs_f64();
        let speed = speed_label(
            self.upload_rate_bytes_per_second,
            event,
            self.started_at.elapsed(),
        );
        let event = event.cloned();
        self.terminal
            .draw(|frame| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(8),
                        Constraint::Length(3),
                        Constraint::Length(8),
                        Constraint::Length(6),
                        Constraint::Min(6),
                    ])
                    .split(frame.area());

                let context_lines = vec![
                    Line::from(vec![Span::styled(
                        "DASObjectStore Upload",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )]),
                    Line::from(format!("Endpoint: {}", context.endpoint)),
                    Line::from(format!("Source: {}", context.source_path.to_string_lossy())),
                    Line::from(format!("Object type: {}", context.object_type)),
                    Line::from(format!("Conflict policy: {}", context.conflict_policy)),
                    Line::from(format!("Dry run: {}", context.dry_run)),
                    Line::from(format!("Status: {status}")),
                    Line::from(format!("Elapsed: {elapsed:.1}s    Speed: {speed}")),
                ];
                frame.render_widget(
                    Paragraph::new(context_lines)
                        .block(Block::default().borders(Borders::ALL).title("Context"))
                        .wrap(Wrap { trim: true }),
                    chunks[0],
                );

                let percent = event
                    .as_ref()
                    .and_then(DaemonIngestProgressEvent::percent_complete)
                    .unwrap_or(0);
                frame.render_widget(
                    Gauge::default()
                        .block(Block::default().borders(Borders::ALL).title("Progress"))
                        .gauge_style(Style::default().fg(Color::Green))
                        .percent(u16::from(percent)),
                    chunks[1],
                );

                let detail_lines = event
                    .as_ref()
                    .map(|event| detail_lines(event, &speed))
                    .unwrap_or_else(|| vec![Line::from("Waiting for first daemon event")]);
                frame.render_widget(
                    Paragraph::new(detail_lines)
                        .block(Block::default().borders(Borders::ALL).title("Transfer"))
                        .wrap(Wrap { trim: true }),
                    chunks[2],
                );

                let queue_lines = event.as_ref().map(queue_lines).unwrap_or_else(|| {
                    vec![Line::from(
                        "Queue state unavailable until daemon planning completes",
                    )]
                });
                frame.render_widget(
                    Paragraph::new(queue_lines)
                        .block(Block::default().borders(Borders::ALL).title("Queues"))
                        .wrap(Wrap { trim: true }),
                    chunks[3],
                );

                let landing_lines = event
                    .as_ref()
                    .map(active_hdd_landing_lines)
                    .unwrap_or_else(|| vec![Line::from("Active landing: waiting for daemon")]);
                frame.render_widget(
                    Paragraph::new(landing_lines)
                        .block(Block::default().borders(Borders::ALL).title("HDD Landing"))
                        .wrap(Wrap { trim: true }),
                    chunks[4],
                );
            })
            .map(|_| ())
    }

    fn update_rate(&mut self, event: &DaemonIngestProgressEvent) {
        let now = Instant::now();
        if let Some(sample) = self.last_rate_sample {
            let elapsed = now.duration_since(sample.at).as_secs_f64();
            let delta = event
                .source_bytes_done
                .unwrap_or(event.work_bytes_done)
                .saturating_sub(sample.source_bytes_done);
            if elapsed > 0.0 {
                self.upload_rate_bytes_per_second = Some((delta as f64 / elapsed).round() as u64);
            }
        }
        self.last_rate_sample = Some(RateSample {
            at: now,
            source_bytes_done: event.source_bytes_done.unwrap_or(event.work_bytes_done),
        });
    }

    fn restore_terminal(&mut self) -> io::Result<()> {
        if self.active && self.terminal_controls {
            execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
            self.terminal.show_cursor()?;
            self.active = false;
        } else {
            self.active = false;
        }
        Ok(())
    }
}

impl<W> Drop for UploadTui<'_, W>
where
    W: Write,
{
    fn drop(&mut self) {
        let _ = self.restore_terminal();
    }
}

fn detail_lines(event: &DaemonIngestProgressEvent, speed: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("Job: {}", event.job_id)),
        Line::from(format!("Stage: {}", stage_label(&event.stage))),
        Line::from(format!(
            "Pipeline: {}",
            event
                .pipeline_stage
                .map(pipeline_stage_label)
                .unwrap_or("unknown")
        )),
        Line::from(format!(
            "Files: {}/{}",
            event.files_done,
            event
                .files_total
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!(
            "Data: {}/{}    Rate: {}",
            format_size_label(event.source_bytes_done.unwrap_or(event.work_bytes_done)),
            event
                .source_bytes_total
                .or(event.work_bytes_total)
                .map(format_size_label)
                .unwrap_or_else(|| "unknown".to_string()),
            speed
        )),
        Line::from(format!(
            "Work: {}/{}",
            format_size_label(event.work_bytes_done),
            event
                .work_bytes_total
                .map(format_size_label)
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!(
            "Current object: {}",
            event
                .current_object_id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none".to_string())
        )),
        Line::from(format!(
            "Message: {}",
            event.message.clone().unwrap_or_else(|| "none".to_string())
        )),
    ]
}

fn queue_lines(event: &DaemonIngestProgressEvent) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(queue_label(event)),
        Line::from(format!("SSD pressure: {}", ssd_pressure_label(event))),
        Line::from(format!("SSD settling: {}", ssd_settling_label(event))),
        Line::from(format!("HDD migration: {}", hdd_migration_label(event))),
    ];
    if let Some(telemetry) = event.telemetry {
        lines.push(Line::from(format!(
            "HDD workers: active {}, idle {}",
            telemetry.workers.hdd_write.active, telemetry.workers.hdd_write.idle
        )));
    }
    lines
}

fn active_hdd_landing_lines(event: &DaemonIngestProgressEvent) -> Vec<Line<'static>> {
    if event.active_hdd_transfers.is_empty() {
        return vec![Line::from("Active landing: idle")];
    }

    std::iter::once(Line::from("Active landing:"))
        .chain(event.active_hdd_transfers.iter().take(8).map(|transfer| {
            let phase = if transfer.bytes_done >= transfer.bytes_total && transfer.bytes_total > 0 {
                format!(
                    "settling; avg {}",
                    format_rate_label(transfer.bytes_per_second)
                )
            } else if transfer.bytes_done == 0 {
                "copying".to_string()
            } else {
                format_rate_label(transfer.bytes_per_second)
            };
            Line::from(format!(
                "file {}/{} copy {} -> {}: {}/{} @ {} {}",
                transfer.file_index,
                transfer
                    .files_total
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                transfer.copy_number,
                transfer.disk_id,
                format_size_label(transfer.bytes_done),
                format_size_label(transfer.bytes_total),
                phase,
                transfer.relative_path
            ))
        }))
        .chain((event.active_hdd_transfers.len() > 8).then(|| {
            Line::from(format!(
                "... {} more active transfer(s)",
                event.active_hdd_transfers.len() - 8
            ))
        }))
        .collect()
}

fn stage_label(stage: &DaemonIngestStage) -> String {
    match stage {
        DaemonIngestStage::Queued => "queued".to_string(),
        DaemonIngestStage::SsdIngest => "ssd-ingest".to_string(),
        DaemonIngestStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy:{disk_id}:{copy_number}"),
        DaemonIngestStage::Complete => "complete".to_string(),
        DaemonIngestStage::Failed => "failed".to_string(),
        DaemonIngestStage::Cancelled => "cancelled".to_string(),
    }
}

fn pipeline_stage_label(stage: DaemonIngestPipelineStage) -> &'static str {
    match stage {
        DaemonIngestPipelineStage::Scan => "scan",
        DaemonIngestPipelineStage::SourceRead => "source-read",
        DaemonIngestPipelineStage::SsdStage => "ssd-stage",
        DaemonIngestPipelineStage::SsdFlush => "ssd-flush",
        DaemonIngestPipelineStage::ChecksumManifestCapture => "checksum-manifest-capture",
        DaemonIngestPipelineStage::HddPlacement => "hdd-placement",
        DaemonIngestPipelineStage::HddWrite => "hdd-write",
        DaemonIngestPipelineStage::Verification => "verification",
        DaemonIngestPipelineStage::Finalization => "finalization",
    }
}

fn queue_label(event: &DaemonIngestProgressEvent) -> String {
    if let Some(telemetry) = event.telemetry {
        return format!(
            "source pending {} file(s), SSD active {}, HDD active {}, HDD queued {}, completed {}",
            telemetry.queue_depths.source_read,
            telemetry.workers.ssd_stage.active,
            telemetry.workers.hdd_write.active,
            telemetry.queue_depths.hdd_write,
            event.files_done
        );
    }

    let total = event.files_total.unwrap_or(event.files_done);
    let active = if matches!(
        event.stage,
        DaemonIngestStage::SsdIngest | DaemonIngestStage::HddCopy { .. }
    ) {
        1
    } else {
        0
    };
    let pending = total.saturating_sub(event.files_done.saturating_add(active));
    let ssd_active = matches!(
        event.pipeline_stage,
        Some(DaemonIngestPipelineStage::SsdStage)
    );
    let hdd_active = matches!(
        event.pipeline_stage,
        Some(DaemonIngestPipelineStage::HddWrite)
    );
    format!(
        "source pending {pending} file(s), SSD active {}, HDD active {}, HDD queued 0, completed {}",
        usize::from(ssd_active),
        usize::from(hdd_active),
        event.files_done
    )
}

fn ssd_pressure_label(event: &DaemonIngestProgressEvent) -> &'static str {
    match event
        .ssd_pressure
        .or_else(|| event.telemetry.map(|telemetry| telemetry.pressure.ssd))
        .unwrap_or(DaemonSsdPressure::AcceptingWrites)
    {
        DaemonSsdPressure::AcceptingWrites => "accepting writes",
        DaemonSsdPressure::High => "high - source ingress may pause",
        DaemonSsdPressure::Critical => "critical - source ingress blocked",
    }
}

fn ssd_settling_label(event: &DaemonIngestProgressEvent) -> String {
    if matches!(
        event.pipeline_stage,
        Some(DaemonIngestPipelineStage::SsdStage)
    ) {
        return stage_bytes_label(event);
    }
    if event
        .telemetry
        .is_some_and(|telemetry| telemetry.workers.ssd_stage.active > 0)
    {
        return "active".to_string();
    }
    "idle".to_string()
}

fn hdd_migration_label(event: &DaemonIngestProgressEvent) -> String {
    if matches!(
        event.pipeline_stage,
        Some(DaemonIngestPipelineStage::HddWrite)
    ) {
        return stage_bytes_label(event);
    }
    if let Some(telemetry) = event.telemetry {
        if telemetry.workers.hdd_write.active > 0 {
            return "active".to_string();
        }
        if telemetry.queue_depths.hdd_write > 0 {
            return format!("queued {} file(s)", telemetry.queue_depths.hdd_write);
        }
    }
    "idle".to_string()
}

fn stage_bytes_label(event: &DaemonIngestProgressEvent) -> String {
    match (event.stage_bytes_done, event.stage_bytes_total) {
        (Some(done), Some(total)) => {
            format!("{}/{}", format_size_label(done), format_size_label(total))
        }
        (Some(done), None) => format_size_label(done),
        _ => "waiting for byte progress".to_string(),
    }
}

fn speed_label(
    current_bytes_per_second: Option<u64>,
    event: Option<&DaemonIngestProgressEvent>,
    elapsed: Duration,
) -> String {
    let current = current_bytes_per_second
        .or_else(|| {
            event
                .and_then(|event| event.telemetry.as_ref())
                .map(|telemetry| telemetry.throughput.current_bytes_per_second)
        })
        .unwrap_or(0);
    let average = average_bytes_per_second(event, elapsed);
    format!(
        "current {}, avg {}",
        format_rate_label(current),
        format_rate_label(average)
    )
}

fn average_bytes_per_second(event: Option<&DaemonIngestProgressEvent>, elapsed: Duration) -> u64 {
    let Some(event) = event else {
        return 0;
    };
    let elapsed = elapsed.as_secs_f64();
    if elapsed <= 0.0 {
        return 0;
    }
    (event.source_bytes_done.unwrap_or(event.work_bytes_done) as f64 / elapsed).round() as u64
}

#[derive(Debug)]
pub struct UploadTuiRenderError(io::Error);

impl Display for UploadTuiRenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "upload TUI render failed: {}", self.0)
    }
}

impl std::error::Error for UploadTuiRenderError {}

#[cfg(test)]
mod tests {
    use super::{UploadTui, UploadTuiContext};
    use dasobjectstore_core::ids::{DiskId, IngestJobId, StoreId};
    use dasobjectstore_daemon::{
        DaemonIngestHddActiveTransfer, DaemonIngestPipelineStage, DaemonIngestProgressEvent,
        DaemonIngestQueueDepths, DaemonIngestStage, DaemonIngestTelemetry,
        DaemonIngestWorkerActivity, DaemonIngestWorkerTelemetry, DaemonSsdPressure,
        SubmitIngestFilesResponse,
    };
    use std::path::PathBuf;

    #[test]
    fn renders_live_upload_progress_frame() {
        let mut output = Vec::new();
        let context = UploadTuiContext {
            endpoint: "zymo_fecal_2025.05".to_string(),
            source_path: PathBuf::from("/mnt/external/zymo"),
            object_type: "fastq".to_string(),
            conflict_policy: "strict".to_string(),
            dry_run: false,
        };

        let event = DaemonIngestProgressEvent {
            job_id: IngestJobId::new("ingest-files-1").expect("job id"),
            endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            stage: DaemonIngestStage::HddCopy {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                copy_number: 1,
            },
            pipeline_stage: Some(DaemonIngestPipelineStage::HddWrite),
            work_bytes_done: 5 * 1024 * 1024 * 1024,
            work_bytes_total: Some(4 * 1024 * 1024 * 1024 * 1024),
            source_bytes_done: Some(5 * 1024 * 1024 * 1024),
            source_bytes_total: Some(2 * 1024 * 1024 * 1024 * 1024),
            stage_bytes_done: Some(512 * 1024 * 1024),
            stage_bytes_total: Some(2 * 1024 * 1024 * 1024),
            files_done: 1,
            files_total: Some(2),
            current_object_id: None,
            ssd_pressure: Some(DaemonSsdPressure::High),
            telemetry: Some(DaemonIngestTelemetry {
                queue_depths: DaemonIngestQueueDepths {
                    source_read: 7,
                    hdd_write: 2,
                    ..DaemonIngestQueueDepths::default()
                },
                workers: DaemonIngestWorkerTelemetry {
                    ssd_stage: DaemonIngestWorkerActivity { active: 1, idle: 0 },
                    hdd_write: DaemonIngestWorkerActivity { active: 1, idle: 0 },
                    ..DaemonIngestWorkerTelemetry::default()
                },
                ..DaemonIngestTelemetry::default()
            }),
            active_hdd_transfers: vec![DaemonIngestHddActiveTransfer {
                file_index: 2,
                files_total: Some(9),
                object_id: dasobjectstore_core::ids::ObjectId::new("zymo/read-2.pod5")
                    .expect("object id"),
                relative_path: "raw/read-2.pod5".to_string(),
                disk_id: DiskId::new("qnap-1057").expect("disk id"),
                copy_number: 1,
                bytes_done: 512 * 1024 * 1024,
                bytes_total: 2 * 1024 * 1024 * 1024,
                bytes_per_second: 128 * 1024 * 1024,
            }],
            resource_policy: None,
            message: Some("copying".to_string()),
        };
        let speed = super::speed_label(
            Some(180 * 1024 * 1024),
            Some(&event),
            std::time::Duration::from_secs(10),
        );
        let details = format!("{:?}", super::detail_lines(&event, &speed));
        assert!(details.contains("Data: 5.0 GiB/2.0 TiB"));
        assert!(details.contains("Work: 5.0 GiB/4.0 TiB"));
        assert!(details.contains("Rate: current 180.0 MiB/s, avg 512.0 MiB/s"));
        let queues = format!("{:?}", super::queue_lines(&event));
        assert!(queues.contains(
            "source pending 7 file(s), SSD active 1, HDD active 1, HDD queued 2, completed 1"
        ));
        assert!(queues.contains("SSD pressure: high - source ingress may pause"));
        assert!(queues.contains("SSD settling: active"));
        assert!(queues.contains("HDD migration: 512.0 MiB/2.0 GiB"));
        assert!(queues.contains("HDD workers: active 1, idle 0"));
        assert!(speed.contains("current 180.0 MiB/s, avg 512.0 MiB/s"));
        let landing = format!("{:?}", super::active_hdd_landing_lines(&event));
        assert!(landing.contains(
            "file 2/9 copy 1 -> qnap-1057: 512.0 MiB/2.0 GiB @ 128.0 MiB/s raw/read-2.pod5"
        ));

        let mut tui = UploadTui::start_with_fixed_viewport(
            &mut output,
            context,
            ratatui::layout::Rect::new(0, 0, 100, 34),
        )
        .expect("tui starts");
        tui.render_progress(event).expect("progress renders");
        tui.finish(&SubmitIngestFilesResponse {
            job_id: IngestJobId::new("ingest-files-1").expect("job id"),
            accepted_at_utc: "2026-07-07T10:27:12Z".to_string(),
            dry_run: false,
        })
        .expect("tui finishes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("DASObjectStore Upload"));
        assert!(output.contains("hdd-copy:disk-a:1"));
        assert!(output.contains("qnap-1057"));
        assert!(output.contains("raw/read-2.pod5"));
        assert!(output.contains("Final response: job=ingest-files-1"));
    }
}
