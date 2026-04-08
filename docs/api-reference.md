# API Reference

## Control plane endpoints

### `GET /api/health`
Returns control-plane health plus guest-runtime health.

### `POST /api/sessions`
Create a sandbox session.

Request example:
```json
{
  "provider": "xvfb",
  "width": 1440,
  "height": 900
}
```

QEMU request example:
```json
{
  "provider": "qemu",
  "boot": "alpine",
  "disable_kvm": true,
  "width": 1280,
  "height": 720
}
```

### `GET /api/sessions/:id`
Return session metadata and whether the browser adapter is attached. QEMU sessions may include `viewer_url`, `runtime_base_url`, and `bridge_status`.

### `DELETE /api/sessions/:id`
Stop the session and clean up child processes/containers.

### `GET /api/sessions/:id/observation`
Return the latest desktop observation, including `summary`, `raw`, screenshot metadata, and action history.

- QEMU sessions in `viewer_only` or `bridge_waiting` return a structured bridge-unavailable error.
- QEMU sessions in `runtime_ready` should behave like bridged Xvfb sessions for the supported observation contract.

### `GET /api/sessions/:id/screenshot`
Return the latest screenshot as `image/png` for bridged sessions.

### `GET /api/sessions/:id/actions`
Return runtime capabilities plus browser-adapter availability details.

- QEMU sessions in `viewer_only` or `bridge_waiting` return zero direct actions.
- QEMU sessions in `runtime_ready` should expose the same phase-target action families as Xvfb (observation/screenshot, shell/filesystem, desktop, plus inspectable browser routing metadata).

### `POST /api/sessions/:id/actions`
Run one action. Returns an `ActionReceipt` or a structured error.

### `POST /api/tasks`
Create a task bound to a session.

### `GET /api/tasks/:id`
Read task status and latest receipt.

### `POST /api/tasks/:id/pause|resume|complete|terminate`
Update task lifecycle state.

### `GET /api/dashboard`
Return current tasks plus per-session action history.

## Guest runtime endpoints

### `GET /health`
Return runtime health and current session count.

### `POST /api/sessions`
Create a guest session. Supported providers:
- `xvfb`: fully bridged local desktop sandbox
- `qemu`: Docker-managed VM session with `viewer_url`, explicit `bridge_status`, and `runtime_base_url` once the guest bridge is reachable

### `GET /api/sessions/:id`
Return the underlying session record.

### `DELETE /api/sessions/:id`
Stop the underlying session provider.

### `GET /api/sessions/:id/observation`
Return the raw Rust observation object for bridged sessions, or a structured bridge-unavailable error while a QEMU session is still `viewer_only` / `bridge_waiting`.

### `GET /api/sessions/:id/screenshot`
Return a PNG screenshot for bridged sessions.

### `GET /api/sessions/:id/actions`
Return guest-runtime capabilities. Pre-ready QEMU sessions return zero direct actions.

### `POST /api/sessions/:id/actions`
Run a guest-runtime action. Browser-specialized actions that require DOM tooling are usually handled by the control plane instead.

## QEMU bridge lifecycle
`bridge_status` is the provider bridge lifecycle for QEMU sessions:
- `viewer_only`: VM viewer is reachable, guest bridge readiness has not started yet
- `bridge_waiting`: bootstrap / health checks are running
- `runtime_ready`: guest bridge is healthy and direct runtime actions are allowed
- `failed`: bootstrap or health checks failed; artifact-backed diagnostics should be available

## Browser adapter metadata
`GET /api/sessions/:id/actions` may include:
- `browser_adapter_enabled`
- `browser_adapter_backend` (`desktop-fallback`, `remote-cdp`, or another inspectable backend chosen by the implementation)
- `browser_adapter` supported action names

For bridged QEMU sessions, the default happy path should not report `remote-cdp` unless the caller explicitly enabled a dev fallback.

## Action receipt shape
```json
{
  "status": "ok",
  "receipt_id": "uuid",
  "action_type": "run_command",
  "started_at": "2026-04-08T16:02:57.259975810Z",
  "completed_at": "2026-04-08T16:02:57.263049754Z",
  "result": {},
  "artifacts": [],
  "error": null
}
```

## Structured provider bridge error
```json
{
  "error": {
    "code": "provider_bridge_unavailable",
    "message": "actions require a guest runtime bridge inside the VM",
    "category": "provider",
    "details": {
      "provider": "qemu",
      "viewer_url": "http://172.17.0.4:8006",
      "bridge_status": "bridge_waiting"
    }
  }
}
```
