#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

usage() {
  cat <<'USAGE'
Usage:
  ./scripts/release-crates-alpha.sh [--preflight]
  ./scripts/release-crates-alpha.sh --publish

Modes:
  --preflight   Run the verified local release checks only (default).
  --publish     Run preflight, publish Rust crates in dependency order,
                wait for each crate to appear on crates.io, then run
                install smoke checks from crates.io.

Optional environment:
  ACU_RELEASE_VERSION       Override the workspace version from Cargo.toml.
  ACU_CRATES_INSTALL_ROOT   Override install smoke root (default: /tmp/acu-crates-install).
  ACU_CRATES_HEALTH_PORT    Override installed runtime health port (default: 4411).
  ACU_CRATES_WAIT_TIMEOUT   Wait timeout in seconds per crate (default: 300).
  ACU_CRATES_WAIT_INTERVAL  Wait interval in seconds per crate poll (default: 5).
USAGE
}

mode="preflight"
case "${1:---preflight}" in
  --preflight) mode="preflight" ;;
  --publish) mode="publish" ;;
  --help|-h) usage; exit 0 ;;
  *)
    echo "Unknown argument: ${1}" >&2
    usage >&2
    exit 1
    ;;
esac

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

require_cmd bun
require_cmd cargo
require_cmd curl
require_cmd python

run() {
  echo "+ $*"
  "$@"
}

workspace_version() {
  python - <<'PY'
from pathlib import Path
import tomllib
cargo = tomllib.loads(Path('Cargo.toml').read_text())
print(cargo['workspace']['package']['version'])
PY
}

VERSION=${ACU_RELEASE_VERSION:-$(workspace_version)}
INSTALL_ROOT=${ACU_CRATES_INSTALL_ROOT:-/tmp/acu-crates-install}
HEALTH_PORT=${ACU_CRATES_HEALTH_PORT:-4411}
WAIT_TIMEOUT=${ACU_CRATES_WAIT_TIMEOUT:-300}
WAIT_INTERVAL=${ACU_CRATES_WAIT_INTERVAL:-5}

crate_version_from_index() {
  local crate="$1"
  python - "$crate" <<'PY'
import json
import sys
import urllib.error
import urllib.request

crate = sys.argv[1]
url = f"https://crates.io/api/v1/crates/{crate}"
try:
    with urllib.request.urlopen(url, timeout=10) as response:
        payload = json.load(response)
except urllib.error.HTTPError as exc:
    if exc.code == 404:
        print("")
        raise SystemExit(0)
    raise
print(payload["crate"]["max_version"])
PY
}

wait_for_crate_version() {
  local crate="$1"
  local deadline=$((SECONDS + WAIT_TIMEOUT))

  while (( SECONDS < deadline )); do
    local observed
    observed=$(crate_version_from_index "$crate")
    if [[ "$observed" == "$VERSION" ]]; then
      echo "Confirmed ${crate} ${VERSION} on crates.io"
      return 0
    fi

    if [[ -n "$observed" ]]; then
      echo "Waiting for ${crate} ${VERSION}; crates.io currently shows ${observed}" >&2
    else
      echo "Waiting for ${crate} ${VERSION}; crate not visible on crates.io yet" >&2
    fi
    sleep "$WAIT_INTERVAL"
  done

  echo "Timed out waiting for ${crate} ${VERSION} on crates.io" >&2
  return 1
}

run_preflight() {
  run bun ci
  run bun run lint
  run bun run build
  run bun run test
  run cargo publish -p desktop-core --dry-run
  run cargo publish --workspace --dry-run --no-verify
}

run_install_smoke() {
  local schema_dir=/tmp/acu-crates-schemas
  local runtime_log=/tmp/acu-crates-runtime.log
  local health_json=/tmp/acu-crates-runtime-health.json
  rm -rf "$INSTALL_ROOT" "$schema_dir"

  run cargo install guest-runtime --version "$VERSION" --locked --root "$INSTALL_ROOT"
  "$INSTALL_ROOT/bin/guest-runtime" --host 127.0.0.1 --port "$HEALTH_PORT" >"$runtime_log" 2>&1 &
  local runtime_pid=$!
  cleanup_runtime() {
    kill "$runtime_pid" 2>/dev/null || true
    wait "$runtime_pid" 2>/dev/null || true
  }
  trap cleanup_runtime EXIT

  for _ in $(seq 1 40); do
    if curl -fsS "http://127.0.0.1:${HEALTH_PORT}/health" >"$health_json"; then
      break
    fi
    sleep 0.5
  done

  if [[ ! -f "$health_json" ]]; then
    cat "$runtime_log" >&2 || true
    echo "Installed guest-runtime never became healthy" >&2
    exit 1
  fi

  cat "$health_json"
  cleanup_runtime
  trap - EXIT

  run cargo install desktop-core --version "$VERSION" --bin export-schemas --locked --root "$INSTALL_ROOT"
  run "$INSTALL_ROOT/bin/export-schemas" "$schema_dir"
  ls -1 "$schema_dir" | sed 's#^#schema:#'
}

run_publish_flow() {
  run_preflight

  run cargo publish -p desktop-core
  wait_for_crate_version desktop-core

  run cargo publish -p linux-backend
  wait_for_crate_version linux-backend

  run cargo publish -p guest-runtime
  wait_for_crate_version guest-runtime

  run_install_smoke
}

if [[ "$mode" == "publish" ]]; then
  run_publish_flow
else
  run_preflight
fi
