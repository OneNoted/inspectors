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
- **Operator/debug path:** expose a canonical `live_desktop_view` route for product oversight, while retaining raw `viewer_url` for debugging.
- Storage is tiered:
  - `runtime/` = session-owned and ephemeral
  - `cache/` = reusable qemu image assets
  - `exports/` = explicit retained artifacts/evidence
- Each session gets its own runtime artifact directory under the ephemeral tier.

## Review recording strategy
- QEMU `product` sessions can emit a storage-efficient `review_recording` summary and sparse review bundle for later human review.
- V1 deliberately does **not** default to continuous video capture; it keeps a sparse bundle (`review.json`, `timeline.jsonl`, deduplicated screenshots) because the goal is later review with the best byte-to-evidence ratio.
- Runtime review artifacts live under the session-owned ephemeral tier until an explicit `POST /api/sessions/:id/review/export` promotes them into `exports/`.
- The live operator surface stays `live_desktop_view`; review recording is a separate artifact for later inspection, not a replacement stream.

## Observation strategy
- Screenshot-first.
- Optional X11 metadata (active window and cursor position) when tools are available.
- Clear split between `raw` machine observations and `summary` fields intended for models/operators.
- QEMU sessions stay honest: pre-ready sessions return structured bridge-unavailable errors instead of synthetic screenshots.
- The oversight UI renders from `live_desktop_view` metadata so it can distinguish real live desktop, screenshot fallback, and unavailable states without guessing from `viewer_url`.
- QEMU product sessions can also emit a storage-efficient `review_recording` summary backed by a sparse review bundle (`review.json`, `timeline.jsonl`, deduplicated screenshots) for later human review.

## Action strategy
- Rust guest-runtime handles desktop input, shell/filesystem, app launch, and screenshot capture.
- Rust guest-runtime is also the sole writer of the canonical review timeline; other layers submit append requests instead of writing bundle files directly.
- TypeScript control plane layers on task tracking and browser specialization.
- The control plane exposes review export and forwards browser/task metadata into the canonical review ledger when those actions do not originate inside guest-runtime.
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
- Startup cleanup should only reap inspectors-owned runtime state after liveness checks; reusable cache and explicit exports are excluded.
- Failed qemu product sessions may temporarily pin runtime review data for postmortem inspection, but exported bundles remain the only durable retention tier.
- Future work can replace the transport with vsock/VM-native plumbing without changing the external contract.
