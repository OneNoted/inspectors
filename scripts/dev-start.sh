#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  if [[ -n "${GUEST_PID:-}" ]] && kill -0 "$GUEST_PID" 2>/dev/null; then
    kill "$GUEST_PID" || true
  fi
}
trap cleanup EXIT

PORT=${PORT:-3000}
GUEST_PORT=${GUEST_PORT:-4001}

cargo run -p guest-runtime -- --port "$GUEST_PORT" > /tmp/acu-guest-runtime.log 2>&1 &
GUEST_PID=$!
sleep 2
npm run build --workspace @acu/control-plane
PORT="$PORT" GUEST_RUNTIME_URL="http://127.0.0.1:${GUEST_PORT}" npm run start --workspace @acu/control-plane
