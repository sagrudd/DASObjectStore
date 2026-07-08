use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[test]
fn documented_performance_recommendation_artifact_covers_ingress_decision_contract() {
    let artifact = read_recommendation_fixture();

    assert_eq!(
        artifact["schema"],
        "dasobjectstore.performance_test.recommendation.v1"
    );
    assert_eq!(artifact["artifact_kind"], "ingress_recommendation");

    assert_string(&artifact, &["run", "run_id"]);
    assert_string(&artifact, &["run", "generated_at_utc"]);
    assert_string(&artifact, &["run", "repository_revision"]);
    assert_string(&artifact, &["run", "cli_version"]);
    assert_non_empty_array(&artifact, &["run", "command"]);
    assert_string(&artifact, &["run", "parameters", "workload_kind"]);
    assert_positive_u64(&artifact, &["run", "parameters", "file_size_bytes"]);
    assert_positive_u64(&artifact, &["run", "parameters", "file_count"]);
    assert_positive_u64(&artifact, &["run", "parameters", "total_source_bytes"]);
    assert!(
        artifact["run"]["parameters"]["source_cap_bytes"].is_null()
            || artifact["run"]["parameters"]["source_cap_bytes"]
                .as_u64()
                .is_some(),
        "source_cap_bytes is null or u64"
    );
    assert_positive_u64(&artifact, &["run", "parameters", "discovered_file_count"]);
    assert_positive_u64(&artifact, &["run", "parameters", "discovered_total_bytes"]);
    assert_positive_u64(&artifact, &["run", "parameters", "max_hdd_concurrency"]);
    assert_string(&artifact, &["run", "artifacts", "pdf_path"]);
    assert_string(&artifact, &["run", "artifacts", "qr_path"]);
    assert_string(&artifact, &["run", "artifacts", "json_path"]);

    assert_string(&artifact, &["hardware", "roots", "ssd_root"]);
    assert_string(&artifact, &["hardware", "roots", "hdd_root"]);
    assert_string(&artifact, &["hardware", "roots", "tmp_dir"]);
    let disks = artifact["hardware"]["disks"]
        .as_array()
        .expect("hardware.disks is an array");
    assert!(!disks.is_empty(), "hardware.disks is populated");
    for disk in disks {
        assert_string(disk, &["disk_id"]);
        assert_string(disk, &["role"]);
        assert_string(disk, &["root_path"]);
    }

    assert_ssd_only_metrics(&artifact);
    assert_pipeline_metrics_cover_concurrency_range(&artifact, "ssd_stage_then_drain_pipeline");
    assert_pipeline_metrics_cover_concurrency_range(&artifact, "ssd_hdd_pipeline");
    assert_pipeline_metrics_cover_concurrency_range(&artifact, "direct_hdd_pipeline");

    let strategy = artifact["recommendation"]["strategy"]
        .as_str()
        .expect("recommendation.strategy is a string");
    assert!(
        matches!(
            strategy,
            "ssd_hdd_pipeline" | "direct_hdd_pipeline" | "ssd_only"
        ),
        "unexpected recommendation.strategy {strategy}"
    );
    assert_positive_u64(&artifact, &["recommendation", "hdd_concurrency"]);
    assert_positive_u64(
        &artifact,
        &["recommendation", "estimated_aggregate_bytes_per_second"],
    );
    assert!(
        artifact["recommendation"]["ssd_read_limited"].is_boolean(),
        "recommendation.ssd_read_limited is boolean"
    );
    assert_non_empty_array(&artifact, &["recommendation", "rationale"]);
}

fn assert_ssd_only_metrics(artifact: &Value) {
    for path in [
        ["scenarios", "ssd_only", "file_count"],
        ["scenarios", "ssd_only", "file_size_bytes"],
        ["scenarios", "ssd_only", "total_bytes"],
        ["scenarios", "ssd_only", "median_generate_bytes_per_second"],
        ["scenarios", "ssd_only", "median_ssd_write_bytes_per_second"],
        ["scenarios", "ssd_only", "median_ssd_read_bytes_per_second"],
    ] {
        assert_positive_u64(artifact, &path);
    }

    let files = artifact["scenarios"]["ssd_only"]["files"]
        .as_array()
        .expect("ssd_only.files is an array");
    assert!(!files.is_empty(), "ssd_only.files is populated");
    for file in files {
        assert_u64(file, &["file_index"]);
        assert_positive_u64(file, &["generated_bytes"]);
        assert_positive_u64(file, &["generate_bytes_per_second"]);
        assert_positive_u64(file, &["ssd_write_bytes_per_second"]);
        assert_positive_u64(file, &["ssd_read_bytes_per_second"]);
    }
}

fn assert_pipeline_metrics_cover_concurrency_range(artifact: &Value, scenario: &str) {
    let max_hdd_concurrency = artifact["run"]["parameters"]["max_hdd_concurrency"]
        .as_u64()
        .expect("max_hdd_concurrency is u64");
    let rows = artifact["scenarios"][scenario]["concurrency"]
        .as_array()
        .unwrap_or_else(|| panic!("{scenario}.concurrency is an array"));
    assert!(!rows.is_empty(), "{scenario}.concurrency is populated");

    let observed = rows
        .iter()
        .map(|row| {
            let concurrency = row["concurrency"]
                .as_u64()
                .unwrap_or_else(|| panic!("{scenario}.concurrency row has numeric concurrency"));
            assert!(concurrency > 0, "{scenario} concurrency is positive");
            assert_string(row, &["scenario"]);
            assert!(
                row["hdd_drain_started_before_all_ssd_staged"].is_boolean(),
                "{scenario} row records overlap evidence"
            );
            assert_positive_u64(row, &["aggregate_assigned_bytes"]);
            assert_positive_u64(row, &["aggregate_write_bytes_per_second"]);
            assert_number(row, &["slowest_member_seconds"]);
            assert_non_empty_array(row, &["members"]);

            let per_disk = row["per_disk"]
                .as_array()
                .unwrap_or_else(|| panic!("{scenario}.per_disk is an array"));
            assert_eq!(
                per_disk.len(),
                concurrency as usize,
                "{scenario}.per_disk length matches concurrency"
            );
            for disk in per_disk {
                assert_string(disk, &["disk_id"]);
                assert_positive_u64(disk, &["assigned_bytes"]);
                assert_positive_u64(disk, &["write_bytes_per_second"]);
            }

            concurrency
        })
        .collect::<BTreeSet<_>>();

    let expected = (1..=max_hdd_concurrency).collect::<BTreeSet<_>>();
    assert_eq!(observed, expected, "{scenario} covers concurrency 1..N");
}

fn assert_string(value: &Value, path: &[&str]) {
    assert!(
        at(value, path)
            .as_str()
            .is_some_and(|string| !string.is_empty()),
        "{} is a non-empty string",
        path.join(".")
    );
}

fn assert_non_empty_array(value: &Value, path: &[&str]) {
    assert!(
        at(value, path)
            .as_array()
            .is_some_and(|array| !array.is_empty()),
        "{} is a non-empty array",
        path.join(".")
    );
}

fn assert_positive_u64(value: &Value, path: &[&str]) {
    let number = at(value, path)
        .as_u64()
        .unwrap_or_else(|| panic!("{} is an unsigned integer", path.join(".")));
    assert!(number > 0, "{} is positive", path.join("."));
}

fn assert_u64(value: &Value, path: &[&str]) {
    assert!(
        at(value, path).as_u64().is_some(),
        "{} is an unsigned integer",
        path.join(".")
    );
}

fn assert_number(value: &Value, path: &[&str]) {
    assert!(
        at(value, path).is_number(),
        "{} is a number",
        path.join(".")
    );
}

fn at<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(value, |current, key| &current[*key])
}

fn read_recommendation_fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/user/examples/performance-recommendation.v1.json");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}
