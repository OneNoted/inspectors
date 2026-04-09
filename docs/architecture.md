# Architecture

## Top-level split
- `crates/desktop-core`: shared Rust types + schema export
- `crates/linux-backend`: Linux desktop operations backed by available system tools (`Xvfb`, `xdotool`, `xprop`, `import`, Firefox)
- `crates/guest-runtime`: Rust HTTP runtime that owns sandbox session lifecycle and Linux desktop access
- `apps/control-plane`: TypeScript server for task lifecycle, browser specialization, action history, and API/UI serving
- `apps/web-ui`: oversight UI served by the control plane
- `apps/sandbox-runner`: local wrapper that starts the runtime stack
- `packages/ts-sdk` and `python/sdk`: client SDKs
- `scripts/qemu_guest_assets.py`: QEMU guest asset/bootstrap helper

## Sandbox strategy
- **Production target:** QEMU/KVM Linux VM with an in-guest runtime bridge.
- **Verified regression baseline:** Xvfb-backed session provider.
- **Current supported product guest:** Ubuntu 24.04 + GNOME.
- **Current internal regression fixture:** lighter QEMU image using the same guest-runtime protocol.
- **Operator/debug path:** retain `viewer_url` and surface it in the oversight UI.
- Each session gets its own artifact directory.

## Observation strategy
- Screenshot-first.
- Optional X11 metadata (active window and cursor position) when tools are available.
- Clear split between `raw` machine observations and `summary` fields intended for models/operators.
- QEMU sessions stay honest: pre-ready sessions return structured bridge-unavailable errors instead of synthetic screenshots.
- The oversight UI prefers the live viewer when `viewer_url` is available and falls back to screenshots otherwise.

## Action strategy
- Rust guest-runtime handles desktop input, shell/filesystem, app launch, and screenshot capture.
- TypeScript control plane layers on task tracking and browser specialization.
- Every action returns a structured receipt or structured error envelope.
- For QEMU, shell/filesystem/desktop actions go through the guest runtime as the single primary plane.

## QEMU readiness strategy
- `bridge_status` remains the coarse bridge lifecycle.
- `readiness_state` adds the stricter ladder:
  - `booting`
  - `desktop_ready`
  - `bridge_listening`
  - `bridge_attached`
  - `runtime_ready`
  - `failed`
- Guest bootstrap must be deterministic enough to promote sessions to `runtime_ready` automatically.
- QEMU bootstrap/health failures must emit actionable artifacts/logs.
- Future work can replace the transport with vsock/VM-native plumbing without changing the external contract.
