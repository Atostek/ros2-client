#!/usr/bin/env bash
# C7: ros2-client action client -> action_tutorials_cpp fibonacci server
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.action_client interop: ros2-client Fibonacci client -> action_tutorials_cpp server"
spawn ros2 run action_tutorials_cpp fibonacci_action_server
sleep 3
out=$(mktemp)
if rust_interop action-client --timeout 40 >"$out" 2>&1 && grep -q '<<< Action Result: \[0, 1, 1, 2, 3, 5\]' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- client ---" >&2
  cat "$out" >&2 || true
  exit 1
fi
