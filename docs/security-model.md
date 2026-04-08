# Security Model

## Trust boundaries
- The host process is trusted but should avoid direct host-desktop control.
- The preferred trust boundary is a disposable Linux VM.
- This MVP includes an Xvfb development fallback because it is the most reliable local path in the current environment.
- The phase-2 QEMU path provisions a Docker-managed VM container and exposes only a viewer URL until an in-guest runtime bridge is present.

## Policy categories
- Desktop input
- Shell / command execution
- Filesystem read/write
- Browser navigation / DOM operations
- Session lifecycle and reset
- VM viewer attachment / bridge installation

## Current safety posture
- Actions are explicit and logged.
- Structured errors include retryability and categories.
- Task pause/resume/terminate is operator visible.
- Browser state is isolated per session/browser profile directory or remote sidecar.
- Sessions are disposable and have per-session artifact directories.
- Viewer-only QEMU sessions do not pretend they can execute direct desktop actions.

## Known gaps
- Full QEMU/KVM guest-runtime bridging is not implemented yet.
- Fine-grained permission prompts are modeled at the task layer, not enforced by a full policy engine yet.
- The remote CDP browser sidecar relies on Docker and host networking reachability.
