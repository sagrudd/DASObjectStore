#!/usr/bin/env bash
# shellcheck shell=bash

# Resolve Cargo output outside the source tree when a formal builder provides
# CARGO_TARGET_DIR, while retaining target/ for ordinary local package builds.
das_cargo_target_dir() {
  local repo_root="$1" cargo_target_dir
  cargo_target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
  case "$cargo_target_dir" in
    /*) printf '%s\n' "$cargo_target_dir" ;;
    *) printf '%s/%s\n' "$repo_root" "$cargo_target_dir" ;;
  esac
}
