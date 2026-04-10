# Agent Computer Use Platform

A production-minded, vendor-neutral computer-use platform for AI agents. This repository exposes Linux sandbox environments as explicit, machine-readable tool surfaces so agents can observe the screen, act on GUIs, manage files and shell state, and produce auditable artifacts.

## Release status
- Current prerelease target: `v0.1.0-alpha.1`
- This alpha candidate is verified from source, includes crates.io dry-run packaging prep for the Rust workspace, and keeps the JavaScript workspaces private for now.
- The release remains honest about its current limits: QEMU is the product path, while Xvfb remains the explicit local/dev fallback and regression baseline.
- See [`CHANGELOG.md`](CHANGELOG.md) for the initial alpha summary and known limits.
- See [`docs/release.md`](docs/release.md) for the verified release checklist and publish order.

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
- **Default product path:** QEMU product session
- **Current verified regression baseline:** explicit Xvfb-backed session
- **Phase bridge transport:** forwarded TCP/HTTP guest-runtime bridge (explicitly not the final transport)
- **Viewer/debug access:** QEMU product sessions expose a control-plane-owned live desktop route, while raw `viewer_url` remains available for debugging
- **Runtime core:** Rust
- **Control plane:** TypeScript
- **Browser specialization:** in-guest for bridged QEMU when available; otherwise explicit fallback with inspectable metadata

## Quickstart
1. Install Bun, Node.js 22, Python 3.11+, and stable Rust.
2. Install workspace dependencies and compile everything:
   ```bash
   bun ci
   bun run build
   ```
3. Start the full stack with the sandbox runner:
   ```bash
   bun run --filter @acu/sandbox-runner start
   ```
   Or use the helper script:
   ```bash
   ./scripts/dev-start.sh
   ```
4. Open the oversight UI at `http://127.0.0.1:3000` (or the `PORT` you set).
5. Create a QEMU product session from the UI or via the SDKs (this is the default product path). Use Xvfb only when you want the lighter local/dev fallback.
6. Run the explicit Xvfb regression baseline:
   ```bash
   ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
   ```
7. Use `evals/README.md` and `docs/qemu-guest-bridge.md` for the phase-specific QEMU parity scenarios and evidence requirements.

## Rust crates and installation
The Rust workspace is prepared for crates.io publication as three packages:

- `desktop-core` — shared schemas and model types
- `linux-backend` — Linux desktop automation backend
- `guest-runtime` — installable runtime service binary

Planned crates.io publish order for this alpha: `desktop-core` → `linux-backend` → `guest-runtime`.

Install the runtime from this repository for local verification:

```bash
cargo install --path crates/guest-runtime --locked
```

Install the schema exporter binary from this repository for local verification:

```bash
cargo install --path crates/desktop-core --bin export-schemas --locked
export-schemas ./schemas
```

Once published, the equivalent install command will be:

```bash
cargo install guest-runtime --version 0.1.0-alpha.1
```

The `guest-runtime` binary is usually launched by the sandbox runner or control-plane scripts, but it can also be started directly:

```bash
guest-runtime --host 127.0.0.1 --port 4001 --artifacts-dir artifacts/runtime
```

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
- UI/API defaults now prefer `qemu` `product`; choose `xvfb` explicitly for the lighter local/dev lane.
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

## License

MIT. See [`LICENSE`](LICENSE).
