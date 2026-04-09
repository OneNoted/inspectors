# Eval Matrix

This directory captures the acceptance scenarios and evidence requirements for the QEMU guest-bridge phase.

## Required baseline
Run the existing Xvfb smoke flow after QEMU work:
```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

Treat this as the regression guard. QEMU progress is not complete if the Xvfb baseline regresses.

## Phase-specific scenarios
- `tasks/qemu-bridge-bootstrap.json` — session lifecycle + readiness proof
- `tasks/qemu-action-bridge.json` — shell/filesystem + desktop parity proof
- `tasks/qemu-browser-trust-boundary.json` — in-guest browser routing proof
- `tasks/qemu-acceptance-regression.json` — lighter QEMU guardrail (`jq --version` + file readback)
- `tasks/taskers-qemu-dogfood.json` — Ubuntu GNOME + Taskers product proof
- `tasks/xvfb-regression-smoke.json` — explicit regression guard record

## Evidence to retain
- session metadata snapshots
- bridge health timestamps
- action receipts / structured errors
- screenshot artifacts when available
- guest bootstrap logs
- viewer/debug references
- failure classification if the bridge does not become ready
