# Sandbox Notes

## Providers
- `xvfb`: implemented local/dev fallback with full action bridge and regression-baseline status.
- `qemu`: Docker-managed `qemux/qemu` VM provider with explicit bridge lifecycle plus a separate `readiness_state` ladder.

## QEMU profiles
- **`product`**: supported Ubuntu 24.04 + GNOME happy path for real desktop dogfooding.
- **`regression`**: lighter internal fixture that preserves the same guest-runtime protocol for package/file regression checks.

The control plane stays single-path:
- shell/filesystem/desktop actions go through `guest-runtime`
- the viewer/live UI is for oversight only
- `qemu` `product` uses a canonical control-plane live desktop route, while `xvfb` stays screenshot-only in this phase

## Display stack
The current environment ships with `Xvfb`, `xdotool`, `xprop`, `xrandr`, `import`, Firefox, and Docker. That is sufficient for:
- a meaningful local GUI-control loop via Xvfb,
- provisioning a real QEMU-backed Linux VM viewer via Docker, and
- validating the host-to-guest runtime bridge used for this phase.

## Shared host path
QEMU sessions can mount a host path into the container at `/shared/hostshare`, which the guest can then mount via 9p using the `shared` mount tag.

This is the intended path for dogfooding local apps such as `../taskers` without inventing a second transport.

## Regression baseline
Run the Xvfb smoke eval after QEMU changes:
```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

## QEMU image preparation
Prepare or warm image assets with:
```bash
python3 scripts/qemu_guest_assets.py ensure-image --profile regression --cache-root artifacts/qemu-image-cache --guest-runtime-binary target/debug/guest-runtime
python3 scripts/qemu_guest_assets.py ensure-image --profile product --cache-root artifacts/qemu-image-cache --guest-runtime-binary target/debug/guest-runtime
```

The product guest uses a prepared Ubuntu GNOME image. The regression fixture may be lighter, but must keep the same guest-runtime readiness and action protocol.

## Acceptance scripts
- regression bridge proof:
  ```bash
  ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-qemu-acceptance.py
  ```
- live desktop proof:
  ```bash
  ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-qemu-live-view-demo.py
  ```
- Taskers dogfood proof:
  ```bash
  ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-taskers-qemu-dogfood.py
  ```
  This is the recommended real-app qemu product proof because Taskers is a user-facing GUI app with machine-verifiable `taskersctl` state. When `ACU_TASKERS_BUNDLE` is unset, the script now auto-selects the newest local Linux bundle from `../taskers/dist`.

## Lifecycle semantics
`bridge_status` remains the coarse bridge lifecycle (`viewer_only`, `bridge_waiting`, `runtime_ready`, `failed`).

`readiness_state` is the stricter readiness ladder:
- `booting`
- `desktop_ready`
- `bridge_listening`
- `bridge_attached`
- `runtime_ready`
- `failed`

`runtime_ready` should only appear after the host can reach `/health` and attach a remote runtime session.
