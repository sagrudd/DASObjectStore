providers="garage rustfs"
workloads="large-object small-object concurrent-client crash-restart-ingest interrupted-write metadata-recovery disk-full simulated-disk-removal ssd-ingest-hdd-destage"

is_supported_provider() {
  case "$1" in
    garage|rustfs) return 0 ;;
    *) return 1 ;;
  esac
}

is_supported_workload() {
  case "$1" in
    large-object|small-object|concurrent-client|crash-restart-ingest|interrupted-write|metadata-recovery|disk-full|simulated-disk-removal|ssd-ingest-hdd-destage) return 0 ;;
    *) return 1 ;;
  esac
}

expected_report_path() {
  output_root="$1"
  provider="$2"
  workload="$3"

  case "$workload" in
    concurrent-client)
      echo "$output_root/$provider/workloads/$workload/summary.tsv"
      ;;
    *)
      echo "$output_root/$provider/workloads/$workload/report.tsv"
      ;;
  esac
}
