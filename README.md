# Agent Computer Use Platform

A production-minded, vendor-neutral computer-use platform for AI agents. This repository exposes Linux sandbox environments as explicit, machine-readable tool surfaces so agents can observe the screen, act on GUIs, manage files and shell state, and produce auditable artifacts.

## What this MVP + QEMU bridge phase implements
- Linux sandbox sessions with a **real GUI fallback** via Xvfb.
- A Rust `guest-runtime` that exposes observations, actions, structured receipts/errors, and session lifecycle for Linux desktop sessions.
- A TypeScript `control-plane` that manages tasks, action history, browser specialization, and a minimal oversight UI.
- A standalone `sandbox-runner` wrapper that starts the runtime stack for local development.
- TypeScript and Python SDKs plus runnable examples.
- Docs, eval task scaffolds, sandbox notes, and CI basics.
- **Container-backed QEMU sessions** with explicit bridge lifecycle reporting (`viewer_only`, `bridge_waiting`, `runtime_ready`, `failed`), canonical `live_desktop_view` metadata, and retained raw `viewer_url` access for operator debugging.
- **Deterministic guest bootstrap + forwarded TCP/HTTP bridge reachability** for this phase's QEMU runtime contract.
- **Browser trust-boundary routing** that prefers the in-guest path for bridged QEMU sessions while keeping remote CDP as an explicit development fallback when `ACU_ENABLE_PLAYWRIGHT=1`.

## Architecture choices
- **Production target:** QEMU/KVM Linux VM with an in-guest runtime
- **Current verified regression baseline:** Xvfb-backed session
- **Phase bridge transport:** forwarded TCP/HTTP guest-runtime bridge (explicitly not the final transport)
- **Viewer/debug access:** QEMU product sessions expose a control-plane-owned live desktop route, while raw `viewer_url` remains available for debugging
- **Runtime core:** Rust
- **Control plane:** TypeScript
- **Browser specialization:** in-guest for bridged QEMU when available; otherwise explicit fallback with inspectable metadata

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
4. Create an Xvfb or QEMU session from the UI or via the SDKs.
5. Run the Xvfb regression baseline:
   ```bash
   ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
   ```
6. Use `evals/README.md` and `docs/qemu-guest-bridge.md` for the phase-specific QEMU parity scenarios and evidence requirements.

## Browser mode behavior
- `browser_open` is always available and falls back to launching the configured browser inside the sandbox.
- DOM-aware browser actions are capability gated behind `ACU_ENABLE_PLAYWRIGHT=1`.
- For bridged QEMU sessions, browser automation should prefer the in-guest route and keep the chosen path inspectable via session/capability metadata.
- The remote CDP Chromium sidecar remains an **explicit dev fallback / non-parity path**, not the default QEMU trust-boundary answer.
- If DOM-aware browser tooling is unavailable, the system returns structured `browser_dom_unavailable` errors instead of pretending success.

## QEMU session behavior
- `provider: "qemu"` provisions a Docker-managed `qemux/qemu` VM session.
- The session record returns `live_desktop_view`, `viewer_url`, `bridge_status`, and runtime metadata once the guest bridge is reachable.
- `viewer_only` and `bridge_waiting` are honest pre-ready states: direct observation/action APIs continue to return structured `provider_bridge_unavailable` errors until runtime health passes.
- `runtime_ready` enables observation, screenshot, shell/filesystem, and desktop actions through the guest runtime bridge.
- `failed` requires artifact-backed bootstrap or health-check diagnostics.
- `qemu` `product` sessions use `/api/sessions/:id/live-view/` as the canonical operator stream.
- `qemu` `regression` and `xvfb` sessions remain honest screenshot-fallback paths unless a real stream exists.
- `viewer_url` remains available for debugging and recovery, not as the primary control path.

## Implemented now
- Xvfb session provider and smoke baseline
- screenshot/input/shell/file/browser-open actions
- explicit QEMU bridge lifecycle + viewer fallback
- browser routing contract with remote CDP dev fallback
- basic task lifecycle
- simple oversight dashboard
- TS/Python SDKs
- schema export, docs, eval scaffolds, and regression guidance

## Planned next
- vsock / VM-native bridge transport
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
