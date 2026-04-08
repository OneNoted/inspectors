# Sandbox Notes

## Providers
- `xvfb`: implemented local/dev fallback with full action bridge.
- `qemu`: Docker-managed `qemux/qemu` VM provider with viewer-first access and explicit `viewer_only` bridge status.

## Display stack
The current environment ships with `Xvfb`, `xdotool`, `xprop`, `xrandr`, `import`, Firefox, and Docker. That is sufficient for:
- a meaningful local GUI-control loop via Xvfb, and
- provisioning a real QEMU-backed Linux VM viewer via Docker.

## QEMU container notes
- Default container image: `qemux/qemu`
- Default boot target: `alpine`
- If `/dev/kvm` is unavailable, sessions can set `disable_kvm: true` (or `KVM=N`) and rely on slower emulation.
- The current implementation does not yet install the Rust guest runtime inside the VM, so direct action APIs stay unavailable until that bridge exists.
