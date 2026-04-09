# Security Model

## Trust boundaries
- The host process is trusted but should avoid direct host-desktop control.
- The preferred trust boundary is a disposable Linux VM.
- Xvfb remains the most reliable local regression baseline in the current environment.
- Bridged QEMU sessions should keep desktop, shell/filesystem, and browser work inside the VM boundary whenever the guest bridge is `runtime_ready`.
- `live_desktop_view` is the canonical operator-facing oversight contract; raw `viewer_url` remains a debug path, not the primary control path.

## Policy categories
- Desktop input
- Shell / command execution
- Filesystem read/write
- Browser navigation / DOM operations
- Session lifecycle and reset
- VM bridge lifecycle / guest bootstrap
- Viewer attachment for debugging and recovery

## Current safety posture
- Actions are explicit and logged.
- Structured errors include retryability and categories.
- Task pause/resume/terminate is operator visible.
- Browser state is isolated per session/browser profile directory or remote sidecar.
- Sessions are disposable and have per-session artifact directories.
- QEMU bridge readiness is explicit: pre-ready sessions do not pretend they can execute direct desktop actions.
- Remote CDP remains visible as a fallback path rather than an implicit trust-boundary shortcut.

## Known gaps
- The forwarded TCP/HTTP bridge is the pragmatic phase transport, not the final VM-native transport.
- Fine-grained permission prompts are modeled at the task layer, not enforced by a full policy engine yet.
- The remote CDP browser sidecar still depends on Docker and host networking reachability when explicitly enabled.
- KVM acceleration is optional in local/dev environments, so performance characteristics can differ from production intent.
