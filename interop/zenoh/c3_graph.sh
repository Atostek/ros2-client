#!/usr/bin/env bash
# C3: ros2 node list / topic list see the ros2-client talker
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.graph discovery: ros2 node list and topic list show zenoh_talker /chatter"
log=$(mktemp)
spawn_log "$log" rust_interop talker --count 40 --timeout 30
if ! wait_for_line "$log" "Talking, count=" 15; then
  fail "$id (talker did not start)"
  cat "$log" >&2 || true
  exit 1
fi

nodes=""
topics=""
ok=0
for _ in $(seq 1 20); do
  nodes=$(timeout -s INT -k 5 10 ros2 node list 2>/dev/null || true)
  topics=$(timeout -s INT -k 5 10 ros2 topic list 2>/dev/null || true)
  if grep -q 'zenoh_talker' <<<"$nodes" && grep -q '/chatter' <<<"$topics"; then
    ok=1
    break
  fi
  sleep 1
done
if [[ "$ok" == 1 ]]; then
  pass "$id"
else
  fail "$id"
  echo "--- node list ---" >&2
  echo "$nodes" >&2
  echo "--- topic list ---" >&2
  echo "$topics" >&2
  exit 1
fi
