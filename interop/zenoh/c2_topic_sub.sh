#!/usr/bin/env bash
# C2: demo_nodes_cpp talker -> ros2-client listener
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.topic_sub interop: demo_nodes_cpp talker -> ros2-client listener prints message len="
spawn ros2 run demo_nodes_cpp talker
sleep 3
out=$(mktemp)
if rust_interop listener --count 1 --timeout 20 >"$out" 2>&1; then
  if grep -q 'message len=' "$out"; then
    pass "$id"
    exit 0
  fi
fi
fail "$id"
echo "--- listener ---" >&2
cat "$out" >&2 || true
exit 1
