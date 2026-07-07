use dasobjectstore_daemon::{
    DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestStage,
    SubmitIngestFilesResponse,
};
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

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
    writer: &'a mut W,
    context: UploadTuiContext,
    started_at: Instant,
    active: bool,
    last_event: Option<DaemonIngestProgressEvent>,
}

impl<'a, W> UploadTui<'a, W>
where
    W: Write,
{
    pub fn start(writer: &'a mut W, context: UploadTuiContext) -> io::Result<Self> {
        write!(writer, "\x1b[?1049h\x1b[?25l")?;
        let mut tui = Self {
            writer,
            context,
            started_at: Instant::now(),
            active: true,
            last_event: None,
        };
        tui.render_frame(None, "waiting for daemon progress")?;
        Ok(tui)
    }

    pub fn render_progress(&mut self, event: DaemonIngestProgressEvent) -> io::Result<()> {
        self.render_frame(
            Some(&event),
            event.message.as_deref().unwrap_or("upload running"),
        )?;
        self.last_event = Some(event);
        Ok(())
    }

    pub fn finish(mut self, response: &SubmitIngestFilesResponse) -> io::Result<()> {
        let message = if response.dry_run {
            "dry run complete"
        } else {
            "upload complete"
        };
        let last_event = self.last_event.clone();
        self.render_frame(last_event.as_ref(), message)?;
        writeln!(
            self.writer,
            "\nFinal response: job={} accepted_at_utc={} dry_run={}",
            response.job_id, response.accepted_at_utc, response.dry_run
        )?;
        self.restore_terminal()
    }

    pub fn fail(mut self, error: impl Display) -> io::Result<()> {
        let message = format!("upload failed: {error}");
        let last_event = self.last_event.clone();
        self.render_frame(last_event.as_ref(), &message)?;
        writeln!(self.writer, "\n{message}")?;
        self.restore_terminal()
    }

    fn render_frame(
        &mut self,
        event: Option<&DaemonIngestProgressEvent>,
        status: &str,
    ) -> io::Result<()> {
        write!(self.writer, "\x1b[H\x1b[2J")?;
        writeln!(self.writer, "DASObjectStore Upload TUI")?;
        writeln!(self.writer, "=========================")?;
        writeln!(self.writer, "Endpoint: {}", self.context.endpoint)?;
        writeln!(
            self.writer,
            "Source: {}",
            self.context.source_path.to_string_lossy()
        )?;
        writeln!(self.writer, "Object type: {}", self.context.object_type)?;
        writeln!(
            self.writer,
            "Conflict policy: {}",
            self.context.conflict_policy
        )?;
        writeln!(self.writer, "Dry run: {}", self.context.dry_run)?;
        writeln!(self.writer, "Status: {status}")?;
        writeln!(
            self.writer,
            "Elapsed: {:.1}s",
            self.started_at.elapsed().as_secs_f64()
        )?;

        if let Some(event) = event {
            writeln!(self.writer)?;
            writeln!(self.writer, "Job: {}", event.job_id)?;
            writeln!(self.writer, "Stage: {}", stage_label(&event.stage))?;
            writeln!(
                self.writer,
                "Pipeline: {}",
                event
                    .pipeline_stage
                    .map(pipeline_stage_label)
                    .unwrap_or("unknown")
            )?;
            writeln!(
                self.writer,
                "Files: {}/{}",
                event.files_done,
                event
                    .files_total
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )?;
            writeln!(
                self.writer,
                "Bytes: {}/{}",
                event.work_bytes_done,
                event
                    .work_bytes_total
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )?;
            writeln!(
                self.writer,
                "Progress: {}",
                progress_bar(event.percent_complete())
            )?;
            if let Some(object_id) = &event.current_object_id {
                writeln!(self.writer, "Current object: {object_id}")?;
            }
            if let Some(message) = &event.message {
                writeln!(self.writer, "Message: {message}")?;
            }
        }

        self.writer.flush()
    }

    fn restore_terminal(&mut self) -> io::Result<()> {
        if self.active {
            write!(self.writer, "\x1b[?25h\x1b[?1049l")?;
            self.writer.flush()?;
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

fn progress_bar(percent: Option<u8>) -> String {
    let Some(percent) = percent else {
        return "[????????????????????] n/a".to_string();
    };
    let filled = (usize::from(percent).min(100) * 20) / 100;
    format!(
        "[{}{}] {:>3}%",
        "#".repeat(filled),
        "-".repeat(20 - filled),
        percent
    )
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
        DaemonIngestPipelineStage::ChecksumManifestCapture => "checksum-manifest-capture",
        DaemonIngestPipelineStage::HddPlacement => "hdd-placement",
        DaemonIngestPipelineStage::HddWrite => "hdd-write",
        DaemonIngestPipelineStage::Verification => "verification",
        DaemonIngestPipelineStage::Finalization => "finalization",
    }
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
        DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestStage,
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

        let mut tui = UploadTui::start(&mut output, context).expect("tui starts");
        tui.render_progress(DaemonIngestProgressEvent {
            job_id: IngestJobId::new("ingest-files-1").expect("job id"),
            endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            stage: DaemonIngestStage::HddCopy {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                copy_number: 1,
            },
            pipeline_stage: Some(DaemonIngestPipelineStage::HddWrite),
            work_bytes_done: 50,
            work_bytes_total: Some(100),
            files_done: 1,
            files_total: Some(2),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            resource_policy: None,
            message: Some("copying".to_string()),
        })
        .expect("progress renders");
        tui.finish(&SubmitIngestFilesResponse {
            job_id: IngestJobId::new("ingest-files-1").expect("job id"),
            accepted_at_utc: "2026-07-07T10:27:12Z".to_string(),
            dry_run: false,
        })
        .expect("tui finishes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("DASObjectStore Upload TUI"));
        assert!(output.contains("Stage: hdd-copy:disk-a:1"));
        assert!(output.contains("[##########----------]  50%"));
        assert!(output.contains("Final response: job=ingest-files-1"));
        assert!(output.contains("\u{1b}[?1049h"));
        assert!(output.contains("\u{1b}[?1049l"));
    }
}
