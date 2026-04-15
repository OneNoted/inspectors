# API Reference

## Control plane endpoints

### `GET /api/health`
Returns control-plane health plus guest-runtime health.

### `POST /api/storage/reclaim`
Report or reclaim stale inspectors-managed storage.

Request body:
```json
{
  "mode": "report"
}
```

Use `"apply"` to remove reclaim candidates instead of only reporting them.

Response includes:
- `runtime_root`
- `cache_root`
- `exports_root`
- `legacy_build_root`
- `candidate_count`
- `candidates[]`
- `reclaimed[]`
- `limitations[]`

Use the reclaim endpoint for inspectors-managed stale runtime directories and old qemu prep workdirs. Detached anonymous Docker volumes from older versions may still require manual Docker review because inspectors cannot attribute them safely once their original containers are gone.

### `POST /api/sessions`
Create a sandbox session.

Default request example (provider omitted -> `qemu` + `product`):
```json
{
  "width": 1440,
  "height": 900
}
```

Explicit QEMU request example:
```json
{
  "provider": "qemu",
  "qemu_profile": "product",
  "shared_host_path": "../taskers",
  "width": 1280,
  "height": 720
}
```

Explicit Xvfb fallback example:
```json
{
  "provider": "xvfb",
  "width": 1440,
  "height": 900
}
```

Optional QEMU fields:
- `qemu_profile`: `product` or `regression`
- `shared_host_path`: host path mounted into the guest via `/shared/hostshare`
- `boot`: optional explicit boot override for low-level/debug sessions
- `container_image`: optional explicit `qemux/qemu` image override
- `desktop_user`: optional default user for GUI-sensitive display actions
- `desktop_home`: optional home directory paired with `desktop_user`
- `desktop_runtime_dir`: optional runtime dir paired with `desktop_user`

### `GET /api/sessions/:id`
Return session metadata and whether the browser adapter is attached.

QEMU sessions may include:
- `live_desktop_view`
- `review_recording`
- `viewer_url`
- `runtime_base_url`
- `bridge_status`
- `readiness_state`
- `qemu_profile`
- `desktop_user`
- `desktop_home`
- `desktop_runtime_dir`

### `DELETE /api/sessions/:id`
Stop the session and clean up child processes/containers.

Default storage semantics:
- session-owned runtime storage is deleted on normal teardown,
- reusable qemu assets stay under cache,
- retained evidence should be treated as explicit export/pin state.

### `POST /api/sessions/:id/review/export`
Export the current sparse review bundle for a qemu `product` session into durable storage.

Response shape:
```json
{
  "bundle": {
    "kind": "review_bundle",
    "path": "/absolute/path/to/artifacts/exports/<session-id>-review"
  },
  "review_recording": {
    "mode": "sparse_timeline",
    "status": "exported",
    "retention": "ephemeral_until_export",
    "event_count": 12,
    "screenshot_count": 4,
    "approx_bytes": 183421,
    "exportable": true,
    "exported_bundle": {
      "kind": "review_bundle",
      "path": "/absolute/path/to/artifacts/exports/<session-id>-review"
    }
  }
}
```

The exported directory is the durable review artifact. In v1 it contains a sparse bundle (`review.json`, `timeline.jsonl`, deduplicated screenshots) rather than a recorded video. Export before teardown if you want the review artifact to survive session deletion.

### `GET /api/sessions/:id/observation`
Return the latest desktop observation, including `summary`, `raw`, screenshot metadata, and action history.

### `GET /api/sessions/:id/screenshot`
Return the latest screenshot as `image/png` for screenshot-capable sessions, including `xvfb` and `qemu` fallback/action planes.

### `GET /api/sessions/:id/live-view/`
Return the canonical live desktop stream for `qemu` `product` sessions, including proxied noVNC assets and websocket upgrades.

Non-stream sessions return a structured `live_desktop_view_unavailable` error instead of pretending they have a live desktop stream.

### `live_desktop_view`
Session metadata now includes:
- `mode`: `stream`, `screenshot_poll`, or `unavailable`
- `status`: `ready`, `degraded`, `stale`, or `unavailable`
- `canonical_url`: control-plane path the UI should render
- `debug_url`: optional raw provider/debug URL
- `matches_action_plane`: whether the human view matches the active action plane
- `reason`: explanatory fallback/unavailability text
- `refresh_interval_ms`: screenshot polling cadence when relevant

### `review_recording`
Session metadata may also include:
- `mode`: `sparse_timeline` or `unavailable`
- `status`: `active`, `idle`, `exported`, or `unavailable`
- `retention`: `ephemeral_until_export` or `temporary_postmortem_pin`
- `event_count`: number of canonical review timeline entries
- `screenshot_count`: number of deduplicated screenshot assets retained in the runtime/export bundle
- `approx_bytes`: approximate review bundle size
- `last_captured_at`: latest review capture timestamp
- `exportable`: whether the session can currently export a durable review bundle
- `exported_bundle`: optional `ArtifactRef` pointing at the latest exported bundle
- `postmortem_retained_until`: optional bounded retention timestamp for failed/postmortem sessions
- `reason`: explanatory text when review recording is unavailable

V1 review recording is intentionally **not** default video capture. Inspectors records a sparse review bundle for qemu `product` sessions: `review.json`, `timeline.jsonl`, and deduplicated screenshots stored only at meaningful action/state boundaries.

### `POST /api/sessions/:id/review/export`
Export the current sparse review bundle into the durable `artifacts/exports/` tier.

Response example:
```json
{
  "bundle": {
    "kind": "review_bundle",
    "path": "artifacts/exports/<session-id>-review",
    "mime_type": null
  },
  "review_recording": {
    "mode": "sparse_timeline",
    "status": "exported",
    "retention": "ephemeral_until_export",
    "event_count": 18,
    "screenshot_count": 7,
    "approx_bytes": 16384,
    "last_captured_at": "2026-04-15T11:12:13Z",
    "exportable": true,
    "exported_bundle": {
      "kind": "review_bundle",
      "path": "artifacts/exports/<session-id>-review",
      "mime_type": null
    },
    "postmortem_retained_until": null,
    "reason": null
  }
}
```

### `GET /api/sessions/:id/actions`
Return runtime capabilities plus browser-adapter availability details.

### `POST /api/sessions/:id/actions`
Run one action. Returns an `ActionReceipt` or a structured error.

Desktop-sensitive action notes:
- `open_app` accepts optional `run_as_user`
- `run_command` accepts optional `run_as_user`
- `run_as_user: "desktop"` resolves to the session's configured desktop user when available

Example GUI-safe Taskers launch in a qemu `product` guest:

```json
{
  "kind": "run_command",
  "command": "LIBGL_ALWAYS_SOFTWARE=1 GDK_BACKEND=x11 nohup /home/ubuntu/taskers-bundle/bin/taskers >/tmp/taskers.log 2>&1 &",
  "run_as_user": "desktop"
}
```

### `POST /api/tasks`
Create a task bound to a session.

## Canonical minimal agent workflow

The simplest supported workflow is:

1. `POST /api/sessions` with provider omitted
2. `GET /api/sessions/:id` until the session is actionable
3. `POST /api/tasks` or `POST /api/sessions/:id/actions`
4. Observe `live_desktop_view` / `GET /api/sessions/:id/observation`
5. `POST /api/sessions/:id/review/export` when you need durable later-review evidence
6. `DELETE /api/sessions/:id` unless you explicitly want to retain exported evidence

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
- `qemu` (default product path when `provider` is omitted)
- `xvfb` (explicit local/dev fallback)
- `display`

For QEMU, the host-side provider eventually attaches a remote runtime session using either:
- `display` for the full-desktop product guest
- `xvfb` for the lighter regression fixture

## QEMU lifecycle fields

### `bridge_status`
- `viewer_only`: VM viewer is reachable but there is no actionable bridge yet
- `bridge_waiting`: bootstrap / health checks are running
- `runtime_ready`: guest bridge is healthy and direct runtime actions are allowed
- `failed`: bootstrap or health checks failed

### `readiness_state`
- `booting`
- `desktop_ready`
- `bridge_listening`
- `bridge_attached`
- `runtime_ready`
- `failed`

`runtime_ready` should only appear after `/health` succeeds and the host has attached a usable remote runtime session.

## Structured provider bridge error
```json
{
  "error": {
    "code": "provider_bridge_unavailable",
    "message": "actions require a guest runtime bridge inside the VM",
    "category": "provider",
    "details": {
      "provider": "qemu",
      "qemu_profile": "product",
      "viewer_url": "http://172.17.0.4:8006",
      "bridge_status": "bridge_waiting",
      "readiness_state": "bridge_listening"
    }
  }
}
```
