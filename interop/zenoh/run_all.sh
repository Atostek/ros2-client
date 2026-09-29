#!/usr/bin/env bash
# Run C1–C9 against a local ROS 2 Jazzy + rmw_zenoh install and write a report.
set -uo pipefail

DIR=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$DIR/../.." && pwd)
desc=$(git -C "$ROOT" describe --tags --always --dirty 2>/dev/null || git -C "$ROOT" rev-parse --short HEAD)
report="$ROOT/interop/results/report-zenoh-jazzy-${desc}.txt"
mkdir -p "$(dirname "$report")"

echo "Building zenoh_interop..."
if ! cargo build --no-default-features --features zenoh \
  --manifest-path "$ROOT/Cargo.toml" \
  --example zenoh_interop; then
  echo "cargo build failed" >&2
  exit 1
fi

cases=(
  c1_topic_pub.sh
  c2_topic_sub.sh
  c3_graph.sh
  c4_service_server.sh
  c5_service_client.sh
  c6_action_server.sh
  c7_action_client.sh
  c8_params.sh
  c9_rosout.sh
)

tmp=$(mktemp)
passes=0
fails=0
{
  echo "===== distro: jazzy (rmw_zenoh) ====="
  echo "  git: $desc"
  if [[ -f /opt/ros/jazzy/setup.bash ]]; then
    # shellcheck disable=SC1091
    set +u
    source /opt/ros/jazzy/setup.bash
    set -u
  fi
  if command -v dpkg-query >/dev/null 2>&1; then
    rmw_ver=$(dpkg-query -W -f='${Package} ${Version}' ros-jazzy-rmw-zenoh-cpp 2>/dev/null || echo "not installed")
    echo "  rmw_zenoh: $rmw_ver"
  fi
  echo
} | tee "$tmp"

for case in "${cases[@]}"; do
  if "$DIR/$case" 2>&1 | tee -a "$tmp"; then
    passes=$((passes + 1))
  else
    fails=$((fails + 1))
  fi
done

{
  echo "== jazzy: $passes pass, 0 note(s), $fails fail(s) =="
  echo "===== TOTAL: $passes pass, 0 note(s), $fails fail(s) across 1 distro(s) ====="
} | tee -a "$tmp"

cp "$tmp" "$report"
echo "Wrote $report"
if [[ "$fails" -ne 0 ]]; then
  exit 1
fi
