scenarios="small-file large-file mixed-file slow-hdd full-ssd interrupted-import"

is_supported_scenario() {
  case "$1" in
    small-file|large-file|mixed-file|slow-hdd|full-ssd|interrupted-import) return 0 ;;
    *) return 1 ;;
  esac
}

scenario_kind() {
  case "$1" in
    small-file) echo "many-small-files" ;;
    large-file) echo "few-large-files" ;;
    mixed-file) echo "mixed-file-sizes" ;;
    slow-hdd) echo "hdd-pressure" ;;
    full-ssd) echo "ssd-pressure" ;;
    interrupted-import) echo "interruption-recovery" ;;
  esac
}

scenario_file_count() {
  case "$1" in
    small-file) echo "100000" ;;
    large-file) echo "8" ;;
    mixed-file) echo "20000" ;;
    slow-hdd) echo "5000" ;;
    full-ssd) echo "5000" ;;
    interrupted-import) echo "10000" ;;
  esac
}

scenario_total_bytes() {
  case "$1" in
    small-file) echo "10737418240" ;;
    large-file) echo "68719476736" ;;
    mixed-file) echo "107374182400" ;;
    slow-hdd) echo "53687091200" ;;
    full-ssd) echo "53687091200" ;;
    interrupted-import) echo "53687091200" ;;
  esac
}

scenario_pressure() {
  case "$1" in
    small-file) echo "metadata-and-verification" ;;
    large-file) echo "source-to-ssd-and-hdd-fan-out" ;;
    mixed-file) echo "balanced" ;;
    slow-hdd) echo "hdd-write-saturation" ;;
    full-ssd) echo "ssd-reserve-throttle" ;;
    interrupted-import) echo "journal-recovery" ;;
  esac
}

expected_metrics_path() {
  output_root="$1"
  scenario="$2"
  run_id="$3"

  echo "$output_root/$scenario/$run_id/metrics.tsv"
}
