#[path = "performance_report_impl/mod.rs"]
mod implementation;
pub(super) use implementation::*;

pub(super) fn write_report_qr_svg(path: &Path, payload: &str) -> Result<String, CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if ProcessCommand::new("qrencode")
        .args(["-t", "SVG", "-o"])
        .arg(path)
        .arg(payload)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return Ok("qrencode SVG".to_string());
    }
    fs::write(path, fallback_qr_svg(payload))?;
    Ok("fallback SVG; install qrencode for a scan-ready QR code".to_string())
}

pub(super) fn fallback_qr_svg(payload: &str) -> String {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    for byte in payload.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100_0000_01b3);
    }
    let cells = 29_usize;
    let scale = 6_usize;
    let size = cells * scale;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}"><rect width="100%" height="100%" fill="white"/>"#
    );
    for y in 0..cells {
        for x in 0..cells {
            let finder = (x < 7 && (y < 7 || y >= cells - 7)) || (x >= cells - 7 && y < 7);
            let on = if finder {
                x == 0
                    || x == 6
                    || y == 0
                    || y == 6
                    || (x >= 2 && x <= 4 && y >= 2 && y <= 4)
                    || (x >= cells - 5 && x <= cells - 3 && y >= 2 && y <= 4)
                    || (x >= 2 && x <= 4 && y >= cells - 5 && y <= cells - 3)
            } else {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 63) & 1) == 1
            };
            if on {
                svg.push_str(&format!(
                    r#"<rect x="{}" y="{}" width="{scale}" height="{scale}" fill="black"/>"#,
                    x * scale,
                    y * scale
                ));
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

pub(super) fn write_pdf_report(
    markdown_path: &Path,
    pdf_path: &Path,
    report: &PerformanceReport,
) -> Result<(), CliError> {
    if let Some(parent) = pdf_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_formal_performance_pdf_report(markdown_path, pdf_path, report)
}

pub(super) const REPORT_RENDERER_ENV: &str = "DASOBJECTSTORE_REPORT_RENDERER";
pub(super) const PACKAGED_REPORT_RENDERER: &str =
    "/usr/libexec/dasobjectstore/gnostikon-workflow-control";

pub(super) fn report_renderer_command() -> OsString {
    if let Some(command) = std::env::var_os(REPORT_RENDERER_ENV) {
        return command;
    }
    let packaged = Path::new(PACKAGED_REPORT_RENDERER);
    if packaged.exists() {
        return packaged.as_os_str().to_os_string();
    }
    OsString::from("gnostikon-workflow-control")
}

pub(super) fn write_formal_performance_pdf_report(
    markdown_path: &Path,
    pdf_path: &Path,
    report: &PerformanceReport,
) -> Result<(), CliError> {
    let metadata_json = performance_report_metadata_json(report);
    let status = ProcessCommand::new(report_renderer_command())
        .arg("render-report-pdf")
        .arg("--provider")
        .arg("container")
        .arg("--input")
        .arg(markdown_path)
        .arg("--output")
        .arg(pdf_path)
        .arg("--title")
        .arg("DASObjectStore Performance Test Report")
        .arg("--title-explanation")
        .arg("Reproducible DAS performance evidence for SSD staging, drain-time SSD reads, and concurrent HDD settlement planning.")
        .arg("--metadata-json")
        .arg(&metadata_json)
        .arg("--provenance-qr-payload")
        .arg(performance_report_qr_payload(report))
        .arg("--report-template")
        .arg("dasobjectstore-performance")
        .arg("--footer-label")
        .arg("DASObjectStore performance")
        .arg("--generated-at-utc")
        .arg(&report.generated_at_utc)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering failed with status {status}; install/repair the DASObjectStore packaged report renderer, Docker/container runtime, and the Grammateus report provider, then rebuild with `dasobjectstore performance-report --json-artifact {}`",
            report.json_path.display()
        ))),
        Err(error) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering requires the DASObjectStore packaged report renderer or an external gnostikon-workflow-control command with Grammateus support plus a Docker/container runtime: {error}; rebuild later with `dasobjectstore performance-report --json-artifact {}`",
            report.json_path.display()
        ))),
    }
}

pub(super) fn write_formal_performance_pdf_report_from_artifact(
    markdown_path: &Path,
    pdf_path: &Path,
    artifact: &Value,
) -> Result<(), CliError> {
    let metadata_json = performance_report_metadata_json_from_artifact(artifact);
    let generated_at =
        json_string(artifact, &["run", "generated_at_utc"]).unwrap_or_else(|| now_utc_string());
    let qr_payload = performance_report_qr_payload_from_artifact(artifact);
    let status = ProcessCommand::new(report_renderer_command())
        .arg("render-report-pdf")
        .arg("--provider")
        .arg("container")
        .arg("--input")
        .arg(markdown_path)
        .arg("--output")
        .arg(pdf_path)
        .arg("--title")
        .arg("DASObjectStore Performance Test Report")
        .arg("--title-explanation")
        .arg("Reproducible DAS performance evidence for SSD staging, drain-time SSD reads, and concurrent HDD settlement planning.")
        .arg("--metadata-json")
        .arg(metadata_json)
        .arg("--provenance-qr-payload")
        .arg(qr_payload)
        .arg("--report-template")
        .arg("dasobjectstore-performance")
        .arg("--footer-label")
        .arg("DASObjectStore performance")
        .arg("--generated-at-utc")
        .arg(generated_at)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering failed with status {status}; install/repair the DASObjectStore packaged report renderer, Docker/container runtime, and the Grammateus report provider"
        ))),
        Err(error) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering requires the DASObjectStore packaged report renderer or an external gnostikon-workflow-control command with Grammateus support plus a Docker/container runtime: {error}"
        ))),
    }
}

pub(super) fn performance_report_metadata_json(report: &PerformanceReport) -> String {
    let run_id = compact_run_id(&report.run_id);
    let signature = compact_hash(&report.reproduction_payload_sha256);
    serde_json::json!({
        "header": "DASObjectStore performance report",
        "rows": [
            [
                {"label": "Run ID", "value": run_id},
                {"label": "Test", "value": "Disk speed"},
                {"label": "Report state", "value": "FINAL"},
            ],
            [
                {"label": "DeviceID", "value": hostname_for_report()},
                {"label": "Operator", "value": std::env::var("USER").unwrap_or_else(|_| "not recorded".to_string())},
                {"label": "Generated at (UTC)", "value": report.generated_at_utc},
            ],
            [
                {"label": "Repository revision", "value": compact_identifier(&report.repository_revision, 18)},
                {"label": "Version", "value": dasobjectstore_core::VERSION},
                {"label": "Test status", "value": "VALID"},
            ],
            [
                {"label": "Signature of operator", "value": "Pending operator signature"},
                {"label": "Cryptographic signature", "value": signature},
            ],
        ],
    })
    .to_string()
}

pub(super) fn performance_report_qr_payload(report: &PerformanceReport) -> String {
    format!(
        "mnemosyne-report:DASObjectStore:{}:{}",
        report.run_id, report.reproduction_payload_sha256
    )
}

pub(super) fn read_performance_json_artifact(path: &Path) -> Result<Value, CliError> {
    let artifact = fs::read_to_string(path)?;
    let artifact = serde_json::from_str::<Value>(&artifact).map_err(|error| {
        CliError::CommandFailed(format!(
            "could not parse performance JSON artifact {}: {error}",
            path.display()
        ))
    })?;
    let schema = json_string(&artifact, &["schema"]).unwrap_or_default();
    if schema != "dasobjectstore.performance_test.recommendation.v1" {
        return Err(CliError::CommandFailed(format!(
            "unsupported performance JSON schema '{}'; expected dasobjectstore.performance_test.recommendation.v1",
            schema
        )));
    }
    Ok(artifact)
}

pub(super) fn artifact_pdf_path(artifact: &Value) -> Option<PathBuf> {
    json_string(artifact, &["run", "artifacts", "pdf_path"]).map(PathBuf::from)
}

pub(super) fn performance_report_metadata_json_from_artifact(artifact: &Value) -> String {
    let run_id =
        json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "not recorded".to_string());
    let compact_run_id = compact_run_id(&run_id);
    let generated_at = json_string(artifact, &["run", "generated_at_utc"])
        .unwrap_or_else(|| "not recorded".to_string());
    let revision = json_string(artifact, &["run", "repository_revision"])
        .unwrap_or_else(|| "not recorded".to_string());
    let version = json_string(artifact, &["run", "cli_version"])
        .unwrap_or_else(|| dasobjectstore_core::VERSION.to_string());
    let signature = performance_artifact_signature(artifact);
    let compact_signature = compact_hash(&signature);
    serde_json::json!({
        "header": "DASObjectStore performance report",
        "rows": [
            [
                {"label": "Run ID", "value": compact_run_id},
                {"label": "Test", "value": "Disk speed"},
                {"label": "Report state", "value": "FINAL"},
            ],
            [
                {"label": "DeviceID", "value": hostname_for_report()},
                {"label": "Operator", "value": std::env::var("USER").unwrap_or_else(|_| "not recorded".to_string())},
                {"label": "Generated at (UTC)", "value": generated_at},
            ],
            [
                {"label": "Repository revision", "value": compact_identifier(&revision, 18)},
                {"label": "Version", "value": version},
                {"label": "Test status", "value": "VALID"},
            ],
            [
                {"label": "Signature of operator", "value": "Pending operator signature"},
                {"label": "Cryptographic signature", "value": compact_signature},
            ],
        ],
    })
    .to_string()
}

pub(super) fn performance_artifact_signature(artifact: &Value) -> String {
    let canonical = serde_json::to_vec(artifact).unwrap_or_default();
    sha256_hex_bytes(&canonical)
}

pub(super) fn performance_report_qr_payload_from_artifact(artifact: &Value) -> String {
    let run_id = json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "unknown".to_string());
    let signature = performance_artifact_signature(artifact);
    format!("mnemosyne-report:DASObjectStore:{run_id}:{signature}")
}

pub(super) fn compact_run_id(value: &str) -> String {
    let value = value
        .strip_prefix("dasobjectstore-performance-")
        .unwrap_or(value);
    compact_identifier(value, 28)
}

pub(super) fn compact_hash(value: &str) -> String {
    compact_identifier(value, 24)
}

pub(super) fn compact_identifier(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars || max_chars < 8 {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let head = keep / 2;
    let tail = keep.saturating_sub(head);
    let prefix = value.chars().take(head).collect::<String>();
    let suffix = value
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

pub(super) fn compact_path(path: &str) -> String {
    let path = path.trim();
    if path.len() <= 42 {
        return path.to_string();
    }
    let Some(file_name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return compact_identifier(path, 42);
    };
    if file_name.is_empty() {
        compact_identifier(path, 42)
    } else {
        format!(".../{file_name}")
    }
}

pub(super) fn humanize_report_token(value: &str) -> String {
    value
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            match part.to_ascii_lowercase().as_str() {
                "das" => return "DAS".to_string(),
                "hdd" => return "HDD".to_string(),
                "id" => return "ID".to_string(),
                "io" => return "IO".to_string(),
                "ssd" => return "SSD".to_string(),
                _ => {}
            }
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn friendly_file_order(value: &str) -> String {
    match value {
        "fifo" => "FIFO".to_string(),
        "size_asc" => "Size ascending".to_string(),
        "size_desc" => "Size descending".to_string(),
        "time_asc" => "Oldest first".to_string(),
        "time_desc" => "Newest first".to_string(),
        other => humanize_report_token(other),
    }
}

pub(super) fn persist_performance_run_artifacts(
    report: &PerformanceReport,
    markdown_source_path: &Path,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let performance_json = render_performance_json(report);
    if let Some(parent) = report.json_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report.json_path, &performance_json)?;
    if let Some(authoritative_path) = &report.authoritative_path {
        if let Some(parent) = authoritative_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(authoritative_path, &performance_json)?;
    }
    let performance_artifact = serde_json::from_str::<Value>(&performance_json)
        .map_err(|err| CliError::CommandFailed(format!("performance JSON did not parse: {err}")))?;
    write_performance_chart_svgs_from_json(&performance_artifact, &report.pdf_path)?;
    let markdown = render_performance_report(report.clone());
    if let Some(parent) = markdown_source_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(markdown_source_path, markdown)?;
    write_pdf_report(markdown_source_path, &report.pdf_path, report)?;
    let _ = fs::remove_file(markdown_source_path);
    writeln!(writer, "Report: {}", report.pdf_path.display())?;
    writeln!(writer, "JSON: {}", report.json_path.display())?;
    if let Some(authoritative_path) = &report.authoritative_path {
        writeln!(
            writer,
            "Authoritative performance policy: {}",
            authoritative_path.display()
        )?;
        writeln!(
            writer,
            "Restart dasobjectstored for the authoritative policy to govern new ingest jobs"
        )?;
    }
    Ok(())
}

use super::*;

pub(super) fn run_performance_report(
    args: &PerformanceReportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let artifact = read_performance_json_artifact(args.json_artifact())?;
    let report_path = args
        .report()
        .map(Path::to_path_buf)
        .or_else(|| artifact_pdf_path(&artifact))
        .ok_or_else(|| {
            CliError::CommandFailed(
                "performance-report requires --report when the JSON artifact does not record a PDF path"
                    .to_string(),
            )
        })?;
    validate_pdf_report_path(&report_path)?;
    let markdown_path = args.tmp_dir().join(format!(
        "{}-rebuilt.md",
        report_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("dasobjectstore-performance-report")
    ));
    if let Some(parent) = markdown_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_performance_chart_svgs_from_json(&artifact, &report_path)?;
    let markdown = render_performance_report_from_json_artifact(&artifact, &report_path);
    fs::write(&markdown_path, markdown)?;
    write_formal_performance_pdf_report_from_artifact(&markdown_path, &report_path, &artifact)?;
    if !args.keep_markdown() {
        let _ = fs::remove_file(&markdown_path);
    }
    writeln!(writer, "Report: {}", report_path.display())?;
    writeln!(writer, "JSON: {}", args.json_artifact().display())?;
    Ok(())
}
