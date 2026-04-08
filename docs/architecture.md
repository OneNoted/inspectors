# Architecture

## Top-level split
- `crates/desktop-core`: shared Rust types + schema export
- `crates/linux-backend`: Linux desktop operations backed by available system tools (`Xvfb`, `xdotool`, `xprop`, `import`, Firefox)
- `crates/guest-runtime`: Rust HTTP runtime that owns sandbox session lifecycle and Linux desktop access
- `apps/control-plane`: TypeScript server for task lifecycle, browser specialization, action history, and API/UI serving
- `apps/web-ui`: minimal oversight UI served by the control plane
- `apps/sandbox-runner`: local wrapper that starts the runtime stack
- `packages/ts-sdk` and `python/sdk`: client SDKs

## Sandbox strategy
- **Production target:** QEMU/KVM Linux VM with an in-guest runtime bridge.
- **Verified regression baseline:** Xvfb-backed session provider.
- **Phase transport:** Docker-managed `qemux/qemu` container + forwarded TCP/HTTP guest-runtime bridge.
- **Operator/debug path:** retain `viewer_url` even after the QEMU bridge is healthy.
- Each session gets its own artifact directory.

## Observation strategy
- Screenshot-first.
- Optional X11 metadata (active window and cursor position) when tools are available.
- Clear split between `raw` machine observations and `summary` fields intended for models/operators.
- QEMU sessions stay honest: `viewer_only` and `bridge_waiting` return structured bridge-unavailable errors instead of synthetic screenshots.
- Once `bridge_status=runtime_ready`, the QEMU path should expose the same explicit observation contract as Xvfb for this phase.

## Action strategy
- Rust guest-runtime handles desktop input, shell/filesystem, app launch, and screenshot capture.
- TypeScript control plane layers on task tracking and browser specialization.
- Every action returns a structured receipt or structured error envelope.
- QEMU bridge readiness is lifecycle-driven (`viewer_only -> bridge_waiting -> runtime_ready | failed`), not a static note.
- Capability reporting should explain exactly why a QEMU session is or is not actionable.

## Browser strategy
- `browser_open` always has a desktop fallback by launching the configured browser in the sandbox.
- DOM-aware automation is explicitly gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- For bridged QEMU sessions, browser routing should prefer the in-guest path.
- Remote CDP remains an explicit dev fallback / non-parity path and should stay inspectable in metadata.
- When DOM-aware browser tooling is unavailable, the system returns structured `browser_dom_unavailable` errors instead of pretending success.

## QEMU strategy
- The phase transport uses a forwarded TCP/HTTP bridge between the host/provider and the in-guest runtime.
- Guest bootstrap must be deterministic enough to promote sessions from `viewer_only` to `runtime_ready` automatically.
- `bridge_status` captures provider-bridge lifecycle while `state` remains the coarse session state.
- QEMU bootstrap/health failures must emit actionable artifacts/logs.
- Future work can replace the transport with vsock/VM-native plumbing without changing the external contract.

See also: `docs/qemu-guest-bridge.md`.
