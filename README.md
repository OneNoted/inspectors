# Agent Computer Use Platform

A production-minded, vendor-neutral computer-use platform for AI agents. This repository exposes Linux sandbox environments as explicit, machine-readable tool surfaces so agents can observe the screen, act on GUIs, manage files and shell state, and produce auditable artifacts.

## What this MVP + phase-2 follow-up implements
- Linux sandbox sessions with a **real GUI fallback** via Xvfb.
- A Rust `guest-runtime` that exposes observations, actions, structured receipts/errors, and session lifecycle for Linux desktop sessions.
- A TypeScript `control-plane` that manages tasks, action history, browser specialization, and a minimal oversight UI.
- A standalone `sandbox-runner` wrapper that starts the runtime stack for local development.
- TypeScript and Python SDKs plus runnable examples.
- Docs, eval task scaffolds, sandbox notes, and CI basics.
- **Container-backed QEMU session provisioning** via `qemux/qemu`, with viewer URLs and explicit bridge-status reporting.
- **Stronger browser DOM automation** via a remote CDP Chromium sidecar when `ACU_ENABLE_PLAYWRIGHT=1`.

## Architecture choices
- **Production target:** QEMU/KVM Linux VM
- **Current verified local sandbox:** Xvfb-backed session
- **Current VM provisioning path:** Docker-managed `qemux/qemu` container with web viewer access
- **Runtime core:** Rust
- **Control plane:** TypeScript
- **Browser specialization:** remote CDP Chromium when enabled; otherwise visible browser open + coordinate fallback + structured DOM-unavailable errors

## Quickstart
1. Install dependencies:
   ```bash
   npm install
   cargo build --workspace
   ```
2. Start the full stack with the sandbox runner:
   ```bash
   npm run start --workspace @acu/sandbox-runner
   ```
   Or use the helper script:
   ```bash
   ./scripts/dev-start.sh
   ```
3. Open the oversight UI at `http://127.0.0.1:3000` (or the `PORT` you set).
4. Create a local Xvfb session from the UI or via the SDKs.
5. Run smoke evaluation:
   ```bash
   ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
   ```

## Browser mode behavior
- `browser_open` is always available and falls back to launching the configured browser inside the sandbox.
- DOM-aware browser actions are capability gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- When enabled, the control plane prefers a **remote CDP Chromium sidecar** (Docker + `chromedp/headless-shell`) and exposes `browser_adapter_backend: remote-cdp`.
- If DOM-aware browser tooling is unavailable, the system returns structured `browser_dom_unavailable` errors instead of pretending success.

## QEMU session behavior
- `provider: "qemu"` now provisions a Docker-managed `qemux/qemu` VM session.
- The session record returns a `viewer_url` and `bridge_status`.
- In this environment the QEMU path is **viewer-only**: direct desktop actions stay unavailable until a guest runtime bridge is present inside the VM.
- This is still useful for proving VM provisioning, viewer reachability, and future bridge integration seams.

## Implemented now
- Xvfb session provider
- screenshot/input/shell/file/browser-open actions
- remote CDP browser DOM path when enabled
- viewer-only QEMU container provisioning
- basic task lifecycle
- simple oversight dashboard
- TS/Python SDKs
- browser specialization surface with explicit capability gating
- schema export and docs

## Planned next
- full guest-runtime bridge inside QEMU-managed VMs
- richer window metadata and accessibility tree
- replay video pipeline
- adapter/plugin loader runtime
- broader eval automation and more robust browser/download support

## Example demos
- `python/examples/browser_research.py`
- `python/examples/file_terminal.py`
- `python/examples/code_editor.py`

## References
- Cursor agent computer use: https://cursor.com/blog/agent-computer-use
- Cursor changelog: https://cursor.com/changelog/1-7/
- Anthropic computer-use demo: https://github.com/anthropics/claude-quickstarts/blob/main/computer-use-demo/README.md
- browser-use: https://github.com/browser-use/browser-use
- OpenAdapt: https://github.com/OpenAdaptAI/OpenAdapt
- OmniMCP: https://github.com/OpenAdaptAI/OmniMCP
- OmniParser: https://github.com/microsoft/OmniParser
- OSWorld: https://github.com/xlang-ai/OSWorld
- VisualWebArena: https://github.com/web-arena-x/visualwebarena
