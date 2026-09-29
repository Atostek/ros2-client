#!/usr/bin/env bash
# C8: ros2 param get/set on a ros2-client node
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.params interop: ros2 param get/set /param_holder speed"
log=$(mktemp)
spawn_log "$log" rust_interop params --timeout 30
if ! wait_for_line "$log" "param_holder ready" 15; then
  fail "$id (parameter node did not start)"
  cat "$log" >&2 || true
  exit 1
fi

get1=$(mktemp)
setout=$(mktemp)
get2=$(mktemp)
# Discovery of the six services can lag the ready line.
sleep 2
timeout -s INT -k 5 20 ros2 param get /param_holder speed >"$get1" 2>&1 || true
timeout -s INT -k 5 20 ros2 param set /param_holder speed 2.5 >"$setout" 2>&1 || true
timeout -s INT -k 5 20 ros2 param get /param_holder speed >"$get2" 2>&1 || true
if grep -q '1.0' "$get1" && grep -q '2.5' "$get2"; then
  pass "$id"
else
  fail "$id"
  echo "--- get 1 ---" >&2
  cat "$get1" >&2 || true
  echo "--- set ---" >&2
  cat "$setout" >&2 || true
  echo "--- get 2 ---" >&2
  cat "$get2" >&2 || true
  exit 1
fi
