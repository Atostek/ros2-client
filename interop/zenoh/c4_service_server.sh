#!/usr/bin/env bash
# C4: ros2 service call -> ros2-client AddTwoInts server
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.service_server interop: ros2 service call -> ros2-client AddTwoInts replies sum=42"
log=$(mktemp)
out=$(mktemp)
spawn_log "$log" rust_interop service-server --timeout 30
if ! wait_for_line "$log" "service-server ready" 15; then
  fail "$id (server did not start)"
  cat "$log" >&2 || true
  exit 1
fi
timeout -s INT -k 5 25 ros2 service call /add_two_ints example_interfaces/srv/AddTwoInts "{a: 2, b: 40}" >"$out" 2>&1 || true
if grep -Eq 'sum[=:][[:space:]]*42' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- service call ---" >&2
  cat "$out" >&2 || true
  echo "--- server ---" >&2
  cat "$log" >&2 || true
  exit 1
fi
