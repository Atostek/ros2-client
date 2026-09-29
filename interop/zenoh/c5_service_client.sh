#!/usr/bin/env bash
# C5: ros2-client client -> examples_rclpy_minimal_service
set -euo pipefail
source "$(dirname "$0")/common.sh"
require_ros
ensure_router

id="jazzy.service_client interop: ros2-client client -> ROS 2 AddTwoInts service, sum 42"
spawn ros2 run examples_rclpy_minimal_service service
sleep 3
out=$(mktemp)
if rust_interop service-client --timeout 25 >"$out" 2>&1 && grep -q 'Response received: sum=42' "$out"; then
  pass "$id"
else
  fail "$id"
  echo "--- client ---" >&2
  cat "$out" >&2 || true
  exit 1
fi
