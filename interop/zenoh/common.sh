# Shared helpers for the local Jazzy + rmw_zenoh interop scripts.
# Source this file; do not execute it.

if [[ -z "${BASH_SOURCE[0]:-}" ]]; then
  echo "common.sh must be sourced from bash" >&2
  return 1 2>/dev/null || exit 1
fi

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PIDS=()

cleanup() {
  local pid
  if ((${#PIDS[@]})); then
    for pid in "${PIDS[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        # setsid makes this pid the process-group leader. Killing the group
        # stops ros2's children (talker, service, action server), which a
        # signal to the launcher alone leaves running.
        # rclpy nodes can ignore SIGTERM; SIGINT is what they shut down on.
        kill -INT -- "-$pid" 2>/dev/null || kill -INT "$pid" 2>/dev/null || true
        local i
        for i in $(seq 1 20); do
          kill -0 -- "-$pid" 2>/dev/null || break
          sleep 0.1
        done
        kill -KILL -- "-$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
      fi
    done
  fi
}
trap cleanup EXIT

require_ros() {
  # Always source Jazzy. ROS_DISTRO=jazzy alone does not put
  # opt/zenoh_cpp_vendor/lib (libzenohc.so) on LD_LIBRARY_PATH.
  if [[ -f /opt/ros/jazzy/setup.bash ]]; then
    # ROS setup scripts read unset variables.
    set +u
    # shellcheck disable=SC1091
    source /opt/ros/jazzy/setup.bash
    set -u
  elif [[ "${ROS_DISTRO:-}" != "jazzy" ]]; then
    echo "ROS 2 Jazzy is not sourced and /opt/ros/jazzy/setup.bash is missing" >&2
    exit 1
  fi
  if ! command -v ros2 >/dev/null 2>&1; then
    echo "ros2 is not on PATH after sourcing Jazzy" >&2
    exit 1
  fi
  export RMW_IMPLEMENTATION=rmw_zenoh_cpp
  export ZENOH_CONFIG_OVERRIDE='mode="client";connect/endpoints=["tcp/127.0.0.1:7447"]'
  export ROS_DOMAIN_ID="${ROS_DOMAIN_ID:-0}"
}

port_open() {
  timeout 1 bash -c 'echo >/dev/tcp/127.0.0.1/7447' >/dev/null 2>&1
}

ensure_router() {
  if ! build_interop; then
    echo "cargo build of zenoh_interop failed" >&2
    exit 1
  fi
  if port_open; then
    return 0
  fi
  # The client override must not apply to the router: with
  # mode="client" rmw_zenohd dials 7447 instead of listening there.
  setsid env -u ZENOH_CONFIG_OVERRIDE ros2 run rmw_zenoh_cpp rmw_zenohd >/tmp/ros2-client-rmw-zenohd.log 2>&1 &
  PIDS+=("$!")
  local i
  for i in $(seq 1 40); do
    if port_open; then
      return 0
    fi
    sleep 0.25
  done
  echo "rmw_zenohd did not open tcp/127.0.0.1:7447" >&2
  cat /tmp/ros2-client-rmw-zenohd.log >&2 || true
  exit 1
}

# Background a command (or shell function) and kill it when this script exits.
spawn() {
  [[ "$1" == rust_interop ]] && set -- "$INTEROP_BIN" "${@:2}"
  # Own session, and no inherited stdout: a leftover child must not hold the
  # pipe that run_all.sh tees, or the next case never starts.
  setsid "$@" >/dev/null 2>&1 &
  PIDS+=("$!")
}

spawn_log() {
  local log=$1
  shift
  [[ "$1" == rust_interop ]] && set -- "$INTEROP_BIN" "${@:2}"
  setsid "$@" >"$log" 2>&1 &
  PIDS+=("$!")
}

INTEROP_BIN="$ROOT/target/debug/examples/zenoh_interop"

build_interop() {
  cargo build --quiet --no-default-features --features zenoh \
    --manifest-path "$ROOT/Cargo.toml" \
    --example zenoh_interop
}

# Run the built binary directly (a shell function cannot be exec'd by setsid).
rust_interop() {
  "$INTEROP_BIN" "$@"
}

wait_for_line() {
  local file=$1 pattern=$2 secs=$3
  local i
  for i in $(seq 1 $((secs * 5))); do
    if [[ -f "$file" ]] && grep -q -- "$pattern" "$file"; then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

pass() {
  echo "  [PASS] $*"
}

fail() {
  echo "  [FAIL] $*"
}
