use super::*;

pub(super) fn run_performance_test(
    args: &PerformanceTestArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_admin_for_performance_test()?;
    let mut workload = plan_performance_workload(args)?;
    if args.max_hdd_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test requires --max-hdd-concurrency greater than 0".to_string(),
        ));
    }
    if !(1..=3).contains(&args.redundancy()) {
        return Err(CliError::CommandFailed(
            "performance-test --redundancy accepts only 1, 2, or 3".to_string(),
        ));
    }

    let ssd_root = args
        .ssd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&ssd_root)?;
    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let disks = discover_managed_hdd_roots(&hdd_root)?;
    if disks.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test found no managed HDD roots under {}",
            hdd_root.display()
        )));
    }
    if args.redundancy() > disks.len() {
        return Err(CliError::CommandFailed(format!(
            "performance-test --redundancy {} requires at least {} managed HDD roots; found {}",
            args.redundancy(),
            args.redundancy(),
            disks.len()
        )));
    }
    let scenario_plan = plan_performance_scenario_matrix(args, disks.len())?;
    if args.authoritative() && scenario_plan.max_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test --authoritative requires at least one HDD landing scenario; include ssd-stage-then-drain, ssd-overlap-drain, or direct-hdd".to_string(),
        ));
    }

    let run_id = timestamped_run_id();
    let ssd_bench_root = ssd_root
        .join(".dasobjectstore")
        .join("performance-test")
        .join(&run_id);
    fs::create_dir_all(&ssd_bench_root)?;
    let mut hdd_bench_roots = Vec::new();
    for disk in &disks {
        let root = disk
            .root_path
            .join(".dasobjectstore")
            .join("performance-test")
            .join(&run_id);
        fs::create_dir_all(&root)?;
        hdd_bench_roots.push((disk.disk_id.clone(), root));
    }
    let _temporary_objectstore = PerformanceTemporaryObjectStore::new(
        ssd_bench_root.clone(),
        hdd_bench_roots
            .iter()
            .map(|(_, root)| root.clone())
            .collect(),
        args.keep_temp(),
        &run_id,
    )?;
    let report_path = args
        .report()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| args.tmp_dir().join(format!("{run_id}-report.pdf")));
    validate_pdf_report_path(&report_path)?;
    let qr_path = report_path.with_extension("qr.svg");
    let markdown_source_path = args.tmp_dir().join(format!("{run_id}-report-source.md"));
    let json_path = args
        .json_artifact()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| report_path.with_extension("json"));
    let max_concurrency = scenario_plan.max_concurrency();
    let file_orders = args.file_orders();
    let scenario_total = scenario_plan
        .scenario_total()
        .saturating_mul(file_orders.len().max(1));
    let reproduction_args = performance_test_reproduction_args(
        args,
        &ssd_root,
        &hdd_root,
        args.tmp_dir(),
        &report_path,
    );
    let reproduce_command = shell_join(&reproduction_args);
    #[cfg(unix)]
    let _interrupt_guard = UploadInterruptGuard::install();

    let _generated_source = materialize_generated_performance_workload(
        &mut workload,
        args.tmp_dir(),
        &run_id,
        writer,
        args.tui(),
        &report_path,
        &json_path,
        scenario_total,
    )?;
    let generated_at_utc = now_utc_string();
    let repository_revision = git_revision();
    let reproduction_payload = serde_json::json!({
        "schema": "dasobjectstore.performance_test.reproduction.v1",
        "brand": "Mnemosyne Biosciences",
        "product": "DASObjectStore",
        "run_id": run_id.clone(),
        "generated_at_utc": generated_at_utc.clone(),
        "repository_revision": repository_revision.clone(),
        "cli_version": dasobjectstore_core::VERSION,
        "command": reproduction_args,
        "parameters": {
            "workload_kind": workload.kind.as_str(),
            "source_path": workload.source_path.as_ref().map(|path| path.to_string_lossy().to_string()),
            "file_size": args.file_size(),
            "file_count": args.file_count(),
            "cap": args.cap(),
            "cap_bytes": workload.source_cap_bytes,
            "file_selection": workload.file_selection.as_str(),
            "file_orders": file_orders.iter().map(|order| order.as_str()).collect::<Vec<_>>(),
            "planned_file_count": workload.file_count(),
            "planned_total_bytes": workload.total_bytes(),
            "discovered_file_count": workload.discovered_file_count,
            "discovered_total_bytes": workload.discovered_total_bytes,
            "max_hdd_concurrency": args.max_hdd_concurrency(),
            "selected_scenarios": scenario_plan.scenario_names(),
            "selected_hdd_concurrency": scenario_plan.concurrency_values(),
            "redundancy": args.redundancy(),
            "ssd_root": ssd_root.to_string_lossy(),
            "hdd_root": hdd_root.to_string_lossy(),
            "tmp_dir": args.tmp_dir().to_string_lossy(),
            "keep_temp": args.keep_temp(),
            "authoritative": args.authoritative(),
        },
        "artifacts": {
            "pdf_path": report_path.to_string_lossy(),
            "qr_path": qr_path.to_string_lossy(),
            "json_path": json_path.to_string_lossy(),
        }
    })
    .to_string();
    let reproduction_payload_sha256 = sha256_hex_bytes(reproduction_payload.as_bytes());

    if !args.tui() {
        writeln!(
            writer,
            "performance-test: workload={} files={} total={} disks={} redundancy={} scenarios={} hdd_concurrency={} report={}",
            workload.kind.as_str(),
            workload.file_count(),
            format_bytes(workload.total_bytes() as f64),
            disks.len(),
            args.redundancy(),
            scenario_plan.scenario_names().join(","),
            format_concurrency_list(&scenario_plan.concurrency_values()),
            report_path.display()
        )?;
    }

    let total_started = Instant::now();

    let results = execute_performance_scenarios(
        writer,
        &workload,
        &file_orders,
        &scenario_plan,
        &ssd_bench_root,
        &hdd_bench_roots,
        args.redundancy(),
        args.tui(),
        scenario_total,
        &report_path,
        &json_path,
    )?;
    let recommendation = recommend_performance_strategy(&results);

    let reproduction_qr_payload =
        format!("mnemosyne-report:DASObjectStore:{run_id}:{reproduction_payload_sha256}");
    let qr_status = write_report_qr_svg(&qr_path, &reproduction_qr_payload)?;
    let performance_report = PerformanceReport {
        run_id,
        generated_at_utc,
        repository_revision,
        file_size: workload.nominal_file_size(),
        file_count: workload.file_count(),
        workload_kind: workload.kind,
        source_path: workload.source_path.clone(),
        source_cap_bytes: workload.source_cap_bytes,
        file_selection: workload.file_selection,
        file_orders: file_orders.clone(),
        discovered_file_count: workload.discovered_file_count,
        discovered_total_bytes: workload.discovered_total_bytes,
        total_source_bytes: workload.total_bytes(),
        ssd_root,
        hdd_root,
        disk_count: disks.len(),
        max_concurrency,
        redundancy: args.redundancy(),
        elapsed_seconds: total_started.elapsed().as_secs_f64(),
        results,
        recommendation,
        authoritative: args.authoritative(),
        authoritative_path: args
            .authoritative()
            .then(|| authoritative_performance_recommendation_path(DEFAULT_DAEMON_STATE_DIR)),
        tmp_dir: args.tmp_dir().to_path_buf(),
        disks: hdd_bench_roots.clone(),
        reproduction_args,
        keep_temp: args.keep_temp(),
        json_path: json_path.clone(),
        qr_path: qr_path.clone(),
        pdf_path: report_path.clone(),
        reproduce_command,
        reproduction_payload_sha256,
        qr_status,
    };
    persist_performance_run_artifacts(&performance_report, &markdown_source_path, writer)?;
    Ok(())
}

fn timestamped_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("dasobjectstore-performance-{nanos}-{}", std::process::id())
}

fn git_revision() -> String {
    let revision = ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(!output.stdout.is_empty())
            } else {
                None
            }
        })
        .unwrap_or(false);
    if dirty && revision != "unknown" {
        format!("{revision}-dirty")
    } else {
        revision
    }
}
