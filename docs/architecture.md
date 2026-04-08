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
- **Production target:** QEMU/KVM Linux VM
- **Implemented local fallback:** Xvfb-backed session provider
- Each session gets its own display id and artifact directory.

## Observation strategy
- Screenshot-first.
- Optional X11 metadata (active window and cursor position) when tools are available.
- Clear split between `raw` machine observations and `summary` fields intended for models/operators.

## Action strategy
- Rust guest-runtime handles desktop input, shell/filesystem, app launch, and screenshot capture.
- TypeScript control plane layers on task tracking and browser specialization.
- Every action returns a structured receipt or structured error envelope.

## Browser strategy
- `browser_open` always has a desktop fallback by launching the configured browser in the sandbox.
- DOM-aware Playwright control is explicitly gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- When Playwright is unavailable, the system still supports visible browser open, fetched-HTML DOM snapshots when possible, and explicit structured errors for unsupported selector-driven actions.

## Deployment shape
- Run `guest-runtime` close to the sandbox provider.
- Run `control-plane` as the operator/API surface.
- Optionally run both via `@acu/sandbox-runner` or `scripts/dev-start.sh`.
