# Sandbox Notes

## Providers
- `xvfb`: implemented local/dev fallback with full action bridge and regression-baseline status.
- `qemu`: Docker-managed `qemux/qemu` VM provider with explicit bridge lifecycle (`viewer_only -> bridge_waiting -> runtime_ready | failed`) and retained viewer access for operators.

## Display stack
The current environment ships with `Xvfb`, `xdotool`, `xprop`, `xrandr`, `import`, Firefox, and Docker. That is sufficient for:
- a meaningful local GUI-control loop via Xvfb,
- provisioning a real QEMU-backed Linux VM viewer via Docker, and
- validating the host-to-guest runtime bridge used for this phase.

## Regression baseline
Run the Xvfb smoke eval after QEMU changes:
```bash
ACU_BASE_URL=http://127.0.0.1:3000 python scripts/run-smoke-eval.py
```

## QEMU container notes
- Default container image: `qemux/qemu`
- Default boot target: `alpine`
- If `/dev/kvm` is unavailable, sessions can set `disable_kvm: true` (or `KVM=N`) and rely on slower emulation.
- The phase bridge transport uses forwarded TCP/HTTP from the host/provider to the in-guest runtime.
- The guest runtime should auto-start deterministically and pass health checks before `bridge_status` becomes `runtime_ready`.
- `viewer_url` remains available for debugging even after bridge readiness.
- Remote CDP is a development fallback, not the primary QEMU trust-boundary answer.
