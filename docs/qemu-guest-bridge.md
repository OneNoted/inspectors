# QEMU Guest Bridge Phase

This document captures the contract for the QEMU guest-bridge phase: a QEMU-backed session should progress from viewer-first provisioning to a usable in-guest runtime without pretending parity before the bridge is actually healthy.

## Phase outcome
- QEMU keeps a real VM + viewer path for operator debugging.
- The guest runtime is bootstrapped inside the VM and exposed to the host through a forwarded TCP/HTTP bridge for this phase.
- Session metadata reports bridge readiness honestly.
- Observation, screenshot, shell/filesystem, desktop, and browser flows only claim parity once the bridge is `runtime_ready`.
- Xvfb remains the must-stay-green regression baseline.

## Bridge lifecycle

| `bridge_status` | Meaning | API behavior | Required evidence |
| --- | --- | --- | --- |
| `viewer_only` | The VM is up enough to expose a viewer, but guest runtime reachability has not started yet. | Direct observation/action APIs return `provider_bridge_unavailable`. | Session record + `viewer_url` |
| `bridge_waiting` | Guest bootstrap and health checks are running. | Direct observation/action APIs still return `provider_bridge_unavailable`. | Bootstrap logs + bridge health timestamps |
| `runtime_ready` | The guest runtime is reachable and capability negotiation is complete. | QEMU sessions expose the same explicit action families as Xvfb for this phase target. | Session record + capability snapshot + action receipts |
| `failed` | Bootstrap or health checks exhausted the allowed recovery path. | APIs stay honest; failure artifacts/logs are mandatory. | Guest logs + health failure classification |

`state` should remain the coarse session lifecycle (`running`, `failed`, etc.). `bridge_status` is the provider-bridge lifecycle that explains why a QEMU session is or is not actionable.

## Phase transport and bootstrap rules
- Use forwarded TCP/HTTP between the host/provider and the in-guest runtime for this phase.
- Keep the transport boundary explicit so a later vsock/native VM transport can replace it cleanly.
- The guest runtime should auto-start deterministically.
- Health must be proven before the session is promoted to `runtime_ready`.
- Failure paths must emit artifact-backed diagnostics rather than silent timeouts.

## Browser trust-boundary rules
- For bridged QEMU sessions, browser automation should prefer the in-guest route.
- Remote CDP is allowed only as an explicit development fallback / non-parity path.
- Capability metadata should make the chosen browser path inspectable.
- Remote CDP success is not enough to claim QEMU trust-boundary parity.

## Evaluation matrix
The phase-specific eval catalog lives in `evals/README.md` and `evals/tasks/*.json`.

Minimum scenarios:
1. **QEMU bridge bootstrap** — prove the session reaches `runtime_ready` and keeps `viewer_url` for debugging.
2. **QEMU action bridge** — prove shell/filesystem plus one desktop action family work through the bridged runtime.
3. **QEMU browser trust boundary** — prove the default happy path stays inside the VM boundary.
4. **Xvfb regression smoke** — rerun the existing Xvfb smoke flow after QEMU changes.

## Evidence checklist
Every QEMU parity run should retain:
- session record snapshots before and after readiness
- guest bootstrap logs
- bridge health timestamps
- capability snapshots (`GET /api/sessions/:id/actions`)
- action receipts / structured errors
- screenshot artifacts when available
- failure classification when readiness is not achieved

## Xvfb regression guard
Treat Xvfb as the regression baseline for the entire phase.

Run after QEMU changes:
```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

Do not treat QEMU progress as complete if the Xvfb smoke run regresses.
