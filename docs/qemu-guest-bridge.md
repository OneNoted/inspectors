# QEMU Guest Bridge

The QEMU path now distinguishes between two related but different concepts:

- **`bridge_status`** — coarse provider-bridge lifecycle used for high-level API honesty
- **`readiness_state`** — stricter readiness ladder used to explain *why* a session is or is not actionable yet

## Lifecycle tables

| Field | Values | Meaning |
| --- | --- | --- |
| `bridge_status` | `viewer_only`, `bridge_waiting`, `runtime_ready`, `failed` | High-level provider bridge state |
| `readiness_state` | `booting`, `desktop_ready`, `bridge_listening`, `bridge_attached`, `runtime_ready`, `failed` | Detailed readiness progression |

## Supported QEMU profiles

### `product`
- Ubuntu 24.04 + GNOME
- full desktop dogfood path
- intended for the Taskers proof

### `regression`
- lighter internal fixture
- same guest-runtime protocol and readiness semantics
- intended for package/file regression checks such as `jq --version`

## Asset strategy

The host prepares or reuses qcow2 assets and then boots them under `qemux/qemu`.

Practical pieces:
- product/regression image preparation: `scripts/qemu_guest_assets.py`
- guest disk booted through `BOOT=/boot.qcow2`
- optional seed ISO attached via `ARGUMENTS=-drive file=/seed.iso,format=raw,media=cdrom,readonly=on`
- optional shared host content exposed through `/shared/hostshare`

## Oversight UI contract

The canonical `live_desktop_view` contract is for:
- operator visibility
- session/task/action context
- pause/resume/stop

Provider truthfulness matters:
- `qemu` `product` exposes a canonical live stream through the control plane
- `qemu` `regression` keeps the VM viewer as debug-only because the action plane runs in guest-side `xvfb`
- `xvfb` is screenshot fallback only in this phase

The live view is **not** the primary control plane for claiming product success.

## Health contract

A QEMU session should not be treated as actionable until:
1. the guest runtime answers `/health`
2. the host can attach a remote runtime session inside the guest
3. the resulting runtime exposes actions
