#!/usr/bin/env bash
# C1: ros2-client talker -> ros2 topic echo /chatter
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.topic_pub interop: ros2-client talker -> ros2 topic echo /chatter sees hello-zenoh"
log=$(mktemp)
out=$(mktemp)
spawn_log "$log" rust_interop talker --count 20 --timeout 25
if ! wait_for_line "$log" "Talking, count=" 15; then
  fail "$id (talker did not start)"
  echo "--- talker log ---" >&2
  cat "$log" >&2 || true
  exit 1
fi
timeout -s INT -k 5 20 ros2 topic echo /chatter std_msgs/msg/String --once >"$out" 2>&1 || true
if grep -q 'hello-zenoh' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- echo ---" >&2
  cat "$out" >&2 || true
  exit 1
fi
