# Plugin / Adapter Guide

Adapters extend the platform with richer app-specific behavior while preserving a fallback to generic desktop control.

## Current adapter classes
- `browser`: specialized in the control plane
- `terminal`: covered by generic shell/file actions
- future: `editor`, `file-manager`, app-specific adapters

## Capability model
Each session exposes capability flags. Clients should inspect `GET /api/sessions/:id/actions` or the SDK equivalent before assuming structured adapters exist.

## Fallback rules
- If a structured adapter is unavailable, prefer explicit fallback to generic desktop actions.
- If no safe fallback exists, return a structured error.
- Never silently pretend a higher-level adapter action succeeded.

## Recommended future shape
- Move browser specialization behind a generic adapter registry.
- Add editor/file-manager adapters that can negotiate capabilities per sandbox image.
- Keep adapter contracts vendor-neutral and machine-readable.
