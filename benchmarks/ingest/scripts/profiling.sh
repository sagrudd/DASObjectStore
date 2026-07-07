#!/usr/bin/env sh
set -eu

output_path="${1:-}"
profile_env="${2:-}"

if [ -z "$output_path" ]; then
  echo "usage: $0 <output-path> [profile-env]" >&2
  exit 64
fi

if [ -n "$profile_env" ] && [ -f "$profile_env" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$profile_env"
  set +a
fi

emit_field() {
  field="$1"
  variable="$2"
  default="$3"

  eval "value=\${$variable:-$default}"
  printf '%s\t%s\n' "$field" "$value"
}

{
  printf 'field\tvalue\n'
  emit_field scenario DASOBJECTSTORE_INGEST_BENCH_SCENARIO not_configured
  emit_field run_id DASOBJECTSTORE_INGEST_BENCH_RUN_ID not_configured
  emit_field cpu_bottleneck DASOBJECTSTORE_INGEST_PROFILE_CPU_BOTTLENECK not_collected
  emit_field cpu_total_percent DASOBJECTSTORE_INGEST_PROFILE_CPU_TOTAL_PERCENT not_collected
  emit_field cpu_hash_percent DASOBJECTSTORE_INGEST_PROFILE_CPU_HASH_PERCENT not_collected
  emit_field cpu_verify_percent DASOBJECTSTORE_INGEST_PROFILE_CPU_VERIFY_PERCENT not_collected
  emit_field memory_bottleneck DASOBJECTSTORE_INGEST_PROFILE_MEMORY_BOTTLENECK not_collected
  emit_field memory_rss_peak_bytes DASOBJECTSTORE_INGEST_PROFILE_MEMORY_RSS_PEAK_BYTES not_collected
  emit_field memory_budget_bytes DASOBJECTSTORE_INGEST_PROFILE_MEMORY_BUDGET_BYTES not_collected
  emit_field memory_buffer_pool_peak_bytes DASOBJECTSTORE_INGEST_PROFILE_MEMORY_BUFFER_POOL_PEAK_BYTES not_collected
  emit_field memory_queue_depth_peak DASOBJECTSTORE_INGEST_PROFILE_MEMORY_QUEUE_DEPTH_PEAK not_collected
  emit_field memory_growth_class DASOBJECTSTORE_INGEST_PROFILE_MEMORY_GROWTH_CLASS not_collected
  emit_field ssd_bottleneck DASOBJECTSTORE_INGEST_PROFILE_SSD_BOTTLENECK not_collected
  emit_field ssd_stage_bytes_per_second DASOBJECTSTORE_INGEST_PROFILE_SSD_STAGE_BYTES_PER_SECOND not_collected
  emit_field ssd_staged_bytes DASOBJECTSTORE_INGEST_PROFILE_SSD_STAGED_BYTES not_collected
  emit_field ssd_used_bytes DASOBJECTSTORE_INGEST_PROFILE_SSD_USED_BYTES not_collected
  emit_field ssd_free_bytes DASOBJECTSTORE_INGEST_PROFILE_SSD_FREE_BYTES not_collected
  emit_field ssd_reserve_bytes DASOBJECTSTORE_INGEST_PROFILE_SSD_RESERVE_BYTES not_collected
  emit_field ssd_pressure_state DASOBJECTSTORE_INGEST_PROFILE_SSD_PRESSURE_STATE not_collected
  emit_field ssd_throttle_state DASOBJECTSTORE_INGEST_PROFILE_SSD_THROTTLE_STATE not_collected
  emit_field ssd_block_state DASOBJECTSTORE_INGEST_PROFILE_SSD_BLOCK_STATE not_collected
  emit_field hdd_bottleneck DASOBJECTSTORE_INGEST_PROFILE_HDD_BOTTLENECK not_collected
  emit_field hdd_fanout_bytes_per_second DASOBJECTSTORE_INGEST_PROFILE_HDD_FANOUT_BYTES_PER_SECOND not_collected
  emit_field hdd_backlog_bytes DASOBJECTSTORE_INGEST_PROFILE_HDD_BACKLOG_BYTES not_collected
  emit_field hdd_retry_count DASOBJECTSTORE_INGEST_PROFILE_HDD_RETRY_COUNT not_collected
  emit_field hdd_saturated_targets DASOBJECTSTORE_INGEST_PROFILE_HDD_SATURATED_TARGETS not_collected
  emit_field verification_bottleneck DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_BOTTLENECK not_collected
  emit_field verification_bytes_per_second DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_BYTES_PER_SECOND not_collected
  emit_field verification_verified_bytes DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_VERIFIED_BYTES not_collected
  emit_field verification_verified_files DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_VERIFIED_FILES not_collected
  emit_field verification_retry_count DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_RETRY_COUNT not_collected
  emit_field verification_failures DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_FAILURES not_collected
  emit_field verification_limiting_finalization DASOBJECTSTORE_INGEST_PROFILE_VERIFICATION_LIMITING_FINALIZATION not_collected
} > "$output_path"
