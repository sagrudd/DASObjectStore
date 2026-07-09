use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const PERFORMANCE_REPORT_JSON_SCHEMA: &str =
    "dasobjectstore.performance_test.recommendation.v1";
pub const PERFORMANCE_REPORT_UPLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_REPORT_COMMAND_TIMEOUT: Duration = Duration::from_secs(180);
const REPORT_COMMAND_ENV: &str = "DASOBJECTSTORE_REPORT_REBUILD_COMMAND";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerformanceReportPdf {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum PerformanceReportRebuildError {
    EmptyUpload,
    TooLarge { actual: usize, max: usize },
    InvalidJson(String),
    UnsupportedSchema(String),
    Io(String),
    RendererFailed(String),
}

impl std::fmt::Display for PerformanceReportRebuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyUpload => write!(f, "uploaded benchmark JSON is empty"),
            Self::TooLarge { actual, max } => write!(
                f,
                "uploaded benchmark JSON is {actual} bytes; maximum accepted size is {max} bytes"
            ),
            Self::InvalidJson(err) => {
                write!(f, "uploaded benchmark artifact is not valid JSON: {err}")
            }
            Self::UnsupportedSchema(schema) => write!(
                f,
                "unsupported benchmark JSON schema '{schema}'; expected {PERFORMANCE_REPORT_JSON_SCHEMA}"
            ),
            Self::Io(err) => write!(f, "report rebuild file IO failed: {err}"),
            Self::RendererFailed(err) => {
                write!(f, "performance report PDF rendering failed: {err}")
            }
        }
    }
}

impl std::error::Error for PerformanceReportRebuildError {}

pub fn rebuild_performance_report_pdf_from_upload(
    upload_bytes: &[u8],
    uploaded_filename: Option<&str>,
    operator: &str,
) -> Result<PerformanceReportPdf, PerformanceReportRebuildError> {
    rebuild_performance_report_pdf_with_command(
        upload_bytes,
        uploaded_filename,
        operator,
        report_command(),
        DEFAULT_REPORT_COMMAND_TIMEOUT,
    )
}

fn rebuild_performance_report_pdf_with_command(
    upload_bytes: &[u8],
    uploaded_filename: Option<&str>,
    operator: &str,
    report_command: OsString,
    timeout: Duration,
) -> Result<PerformanceReportPdf, PerformanceReportRebuildError> {
    validate_performance_json_upload(upload_bytes)?;
    let artifact = serde_json::from_slice::<Value>(upload_bytes)
        .map_err(|err| PerformanceReportRebuildError::InvalidJson(err.to_string()))?;
    validate_performance_schema(&artifact)?;

    let run_id = json_string(&artifact, &["run", "run_id"]).unwrap_or_else(|| "performance".into());
    let upload_stem = uploaded_filename
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .map(sanitize_filename_component)
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| sanitize_filename_component(&run_id));
    let report_filename = format!("{upload_stem}.pdf");

    let temp_root =
        std::env::temp_dir().join(format!("dasobjectstore-report-rebuild-{}", Uuid::new_v4()));
    let json_path = temp_root.join(format!("{upload_stem}.json"));
    let report_path = temp_root.join(&report_filename);
    fs::create_dir_all(&temp_root)
        .map_err(|err| PerformanceReportRebuildError::Io(err.to_string()))?;

    let result = (|| {
        fs::write(&json_path, upload_bytes)
            .map_err(|err| PerformanceReportRebuildError::Io(err.to_string()))?;
        run_report_rebuild_command(
            &report_command,
            &json_path,
            &report_path,
            &temp_root,
            operator,
            timeout,
        )?;
        fs::read(&report_path).map_err(|err| PerformanceReportRebuildError::Io(err.to_string()))
    })();

    let _ = fs::remove_dir_all(&temp_root);
    result.map(|bytes| PerformanceReportPdf {
        filename: report_filename,
        bytes,
    })
}

fn validate_performance_json_upload(bytes: &[u8]) -> Result<(), PerformanceReportRebuildError> {
    if bytes.is_empty() {
        return Err(PerformanceReportRebuildError::EmptyUpload);
    }
    if bytes.len() > PERFORMANCE_REPORT_UPLOAD_MAX_BYTES {
        return Err(PerformanceReportRebuildError::TooLarge {
            actual: bytes.len(),
            max: PERFORMANCE_REPORT_UPLOAD_MAX_BYTES,
        });
    }
    Ok(())
}

fn validate_performance_schema(artifact: &Value) -> Result<(), PerformanceReportRebuildError> {
    let schema = json_string(artifact, &["schema"]).unwrap_or_default();
    if schema == PERFORMANCE_REPORT_JSON_SCHEMA {
        Ok(())
    } else {
        Err(PerformanceReportRebuildError::UnsupportedSchema(schema))
    }
}

fn run_report_rebuild_command(
    report_command: &OsString,
    json_path: &Path,
    report_path: &Path,
    tmp_dir: &Path,
    operator: &str,
    timeout: Duration,
) -> Result<(), PerformanceReportRebuildError> {
    let mut child = Command::new(report_command)
        .arg("performance-report")
        .arg("--json-artifact")
        .arg(json_path)
        .arg("--report")
        .arg(report_path)
        .arg("--tmp-dir")
        .arg(tmp_dir)
        .env("USER", operator)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| PerformanceReportRebuildError::RendererFailed(err.to_string()))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let output = child.wait_with_output().map_err(|err| {
                    PerformanceReportRebuildError::RendererFailed(err.to_string())
                })?;
                return Err(PerformanceReportRebuildError::RendererFailed(format!(
                    "renderer timed out after {} seconds; stdout: {}; stderr: {}",
                    timeout.as_secs(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            Err(err) => {
                return Err(PerformanceReportRebuildError::RendererFailed(
                    err.to_string(),
                ))
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| PerformanceReportRebuildError::RendererFailed(err.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PerformanceReportRebuildError::RendererFailed(format!(
            "renderer exited with {}; stdout: {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn report_command() -> OsString {
    std::env::var_os(REPORT_COMMAND_ENV).unwrap_or_else(|| OsString::from("dasobjectstore"))
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(ToString::to_string)
}

fn sanitize_filename_component(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        rebuild_performance_report_pdf_with_command, validate_performance_schema,
        PerformanceReportRebuildError, PERFORMANCE_REPORT_JSON_SCHEMA,
    };
    use serde_json::json;
    use std::ffi::OsString;
    use std::fs;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn rejects_unsupported_benchmark_json_schema() {
        let artifact = json!({"schema": "other"});
        let error = validate_performance_schema(&artifact).expect_err("schema rejected");
        assert!(matches!(
            error,
            PerformanceReportRebuildError::UnsupportedSchema(_)
        ));
    }

    #[test]
    fn accepts_expected_benchmark_json_schema() {
        let artifact = json!({"schema": PERFORMANCE_REPORT_JSON_SCHEMA});
        validate_performance_schema(&artifact).expect("schema accepted");
    }

    #[test]
    fn rejects_empty_upload_before_renderer_invocation() {
        let error = rebuild_performance_report_pdf_with_command(
            b"",
            Some("empty.json"),
            "operator",
            OsString::from("definitely-not-used"),
            Duration::from_secs(1),
        )
        .expect_err("empty upload rejected");
        assert!(matches!(error, PerformanceReportRebuildError::EmptyUpload));
    }

    #[cfg(unix)]
    #[test]
    fn rebuild_invokes_renderer_and_returns_pdf_bytes() {
        use std::os::unix::fs::PermissionsExt;

        let temp_root =
            std::env::temp_dir().join(format!("dasobjectstore-reporting-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_root).expect("temp root");
        let renderer = temp_root.join("fake-renderer.sh");
        fs::write(
            &renderer,
            "#!/bin/sh\nif [ \"$1\" != \"performance-report\" ]; then exit 2; fi\nprintf '%s' '%PDF-FAKE' > \"$5\"\n",
        )
        .expect("fake renderer written");
        let mut permissions = fs::metadata(&renderer).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&renderer, permissions).expect("permissions");

        let artifact = json!({
            "schema": PERFORMANCE_REPORT_JSON_SCHEMA,
            "run": {"run_id": "fake-run"}
        })
        .to_string();
        let rebuilt = rebuild_performance_report_pdf_with_command(
            artifact.as_bytes(),
            Some("benchmark.json"),
            "stephen",
            renderer.into_os_string(),
            Duration::from_secs(5),
        )
        .expect("report rebuild succeeds");

        assert_eq!(rebuilt.filename, "benchmark.pdf");
        assert_eq!(rebuilt.bytes, b"%PDF-FAKE");

        let _ = fs::remove_dir_all(temp_root);
    }
}
