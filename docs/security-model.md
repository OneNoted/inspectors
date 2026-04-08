# Security Model

## Trust boundaries
- The host process is trusted but should avoid direct host-desktop control.
- The preferred trust boundary is a disposable Linux VM.
- This MVP includes an Xvfb development fallback because QEMU/KVM is not available in the current environment.

## Policy categories
- Desktop input
- Shell / command execution
- Filesystem read/write
- Browser navigation / DOM operations
- Session lifecycle and reset

## Current safety posture
- Actions are explicit and logged.
- Structured errors include retryability and categories.
- Task pause/resume/terminate is operator visible.
- Browser state is isolated per session/browser profile directory.
- Sessions are disposable and have per-session artifact directories.

## Known gaps
- QEMU/KVM isolation is planned but not implemented in this environment.
- Fine-grained permission prompts are modeled at the task layer, not enforced by a full policy engine yet.
- DOM-aware browser control is capability-gated rather than universally available.
