# API Reference

## Control plane endpoints

### `GET /api/adapters`
Return the adapter registry and fallback strategy for browser, terminal, and generic desktop modes.


### `GET /api/health`
Returns control-plane health plus guest-runtime health.

### `POST /api/sessions`
Create a sandbox session.

Request:
```json
{
  "provider": "xvfb",
  "width": 1440,
  "height": 900
}
```

### `GET /api/sessions/:id`
Return session metadata and whether the browser adapter is attached.

### `DELETE /api/sessions/:id`
Stop the session and clean up child processes.

### `GET /api/sessions/:id/observation`
Return the latest desktop observation, including `summary`, `raw`, screenshot metadata, and action history.

### `GET /api/sessions/:id/screenshot`
Return the latest screenshot as `image/png`.

### `GET /api/sessions/:id/actions`
Return runtime capabilities plus browser-adapter availability details.

### `POST /api/sessions/:id/actions`
Run one action. Returns an `ActionReceipt`.

### `POST /api/tasks`
Create a task bound to a session.

### `GET /api/tasks/:id`
Read task status and latest receipt.

### `POST /api/tasks/:id/pause|resume|complete|terminate|reset`
Update task lifecycle state.

### `GET /api/dashboard`
Return current tasks plus per-session action history.

## Guest runtime endpoints

### `GET /health`
Return runtime health and current session count.

### `POST /api/sessions`
Create a guest session. `xvfb` works today; `qemu` is reserved for the production target and currently returns a structured not-implemented error.

### `GET /api/sessions/:id`
Return the underlying session record.

### `DELETE /api/sessions/:id`
Stop the underlying session provider.

### `GET /api/sessions/:id/observation`
Return the raw Rust observation object.

### `GET /api/sessions/:id/screenshot`
Return a PNG screenshot for the session.

### `GET /api/sessions/:id/actions`
Return guest-runtime capabilities.

### `POST /api/sessions/:id/actions`
Run a guest-runtime action. Browser-specialized actions that require DOM tooling return structured unsupported errors unless handled by the control plane.

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
