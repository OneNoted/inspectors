# Plugin / Adapter Guide

Adapters extend the platform with richer app-specific behavior while preserving a fallback to generic desktop control.

## Current adapter classes
- `browser`: specialized in the control plane
- `terminal`: covered by generic shell/file actions
- future: `editor`, `file-manager`, app-specific adapters

## Capability model
Each session exposes capability flags. Clients should inspect `GET /api/sessions/:id/actions` or the SDK equivalent before assuming structured adapters exist.

## Browser adapter modes
- `desktop-fallback`: browser open + coordinate/generic desktop actions
- `remote-cdp`: DOM-aware browser automation via a Docker-managed Chromium sidecar; explicit dev fallback outside the preferred QEMU trust boundary
- in-guest bridge route: DOM-aware automation routed through the bridged QEMU guest runtime once `bridge_status=runtime_ready`

## Fallback rules
- If a structured adapter is unavailable, prefer explicit fallback to generic desktop actions.
- If no safe fallback exists, return a structured error.
- Never silently pretend a higher-level adapter action succeeded.
- For bridged QEMU sessions, keep the chosen browser route inspectable and do not silently fall back from in-guest routing to remote CDP.

## Recommended future shape
- Move browser specialization behind a generic adapter registry.
- Add editor/file-manager adapters that can negotiate capabilities per sandbox image.
- Represent QEMU bridge/browser readiness as adapter capability metadata rather than a viewer-only escape hatch.
- Keep adapter contracts vendor-neutral and machine-readable.
