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
- **Implemented local fallback:** Xvfb-backed session provider.
- **Implemented VM provisioning path:** Docker-managed `qemux/qemu` container for viewer-first VM access.
- Each session gets its own artifact directory.

## Observation strategy
- Screenshot-first.
- Optional X11 metadata (active window and cursor position) when tools are available.
- Clear split between `raw` machine observations and `summary` fields intended for models/operators.
- Viewer-only QEMU sessions intentionally return bridge-unavailable errors instead of synthetic screenshots.

## Action strategy
- Rust guest-runtime handles desktop input, shell/filesystem, app launch, and screenshot capture for Xvfb sessions.
- TypeScript control plane layers on task tracking and browser specialization.
- Every action returns a structured receipt or structured error envelope.
- QEMU sessions advertise `bridge_status: viewer_only` until a guest runtime bridge is available.

## Browser strategy
- `browser_open` always has a desktop fallback by launching the configured browser in the sandbox.
- DOM-aware automation is explicitly gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- When enabled, the control plane can spawn a Docker-managed `chromedp/headless-shell` container and connect through CDP (`browser_adapter_backend: remote-cdp`).
- When DOM-aware browser tooling is unavailable, the system returns structured `browser_dom_unavailable` errors instead of pretending success.

## QEMU strategy
- The current QEMU path provisions a real Linux VM container via `qemux/qemu`, using `BOOT=<image>` and `KVM=N` when `/dev/kvm` is unavailable.
- The control plane records `viewer_url` so operators or future browser-driven agents can attach to the VM web viewer.
- The next architectural step is installing and exposing the Rust guest runtime inside the VM so the same explicit action contract works across both Xvfb and VM providers.
