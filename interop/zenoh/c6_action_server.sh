#!/usr/bin/env bash
# C6: ros2 action send_goal -> ros2-client Fibonacci server
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.action_server interop: ros2 action send_goal -> ros2-client Fibonacci result [0, 1, 1, 2, 3, 5]"
log=$(mktemp)
out=$(mktemp)
spawn_log "$log" rust_interop action-server --timeout 45
if ! wait_for_line "$log" "action-server ready" 15; then
  fail "$id (server did not start)"
  cat "$log" >&2 || true
  exit 1
fi
timeout -s INT -k 5 40 ros2 action send_goal /fibonacci action_tutorials_interfaces/action/Fibonacci "{order: 5}" --feedback >"$out" 2>&1 || true
# Jazzy's CLI prints the result sequence as YAML list items, one per line.
result=$(sed -n '/^Result:/,/^Goal finished/p' "$out" | sed -n 's/^- //p' | paste -sd, -)
if [[ "$result" == "0,1,1,2,3,5" ]] && grep -q 'Goal finished with status: SUCCEEDED' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- send_goal ---" >&2
  cat "$out" >&2 || true
  echo "--- server ---" >&2
  cat "$log" >&2 || true
  exit 1
fi
