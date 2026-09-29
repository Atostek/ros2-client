#!/usr/bin/env bash
# C9: ros2 topic echo /rosout sees a ros2-client log line
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.rosout interop: ros2 topic echo /rosout shows rosout interop line"
log=$(mktemp)
out=$(mktemp)
spawn_log "$log" rust_interop rosout --timeout 25
if ! wait_for_line "$log" "rosout published" 15; then
  fail "$id (logger did not start)"
  cat "$log" >&2 || true
  exit 1
fi
timeout -s INT -k 5 20 ros2 topic echo /rosout rcl_interfaces/msg/Log --once >"$out" 2>&1 || true
if grep -q 'rosout interop line' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- echo ---" >&2
  cat "$out" >&2 || true
  echo "--- logger ---" >&2
  cat "$log" >&2 || true
  exit 1
fi
