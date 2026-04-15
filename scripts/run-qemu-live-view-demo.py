from __future__ import annotations

import hashlib
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from python.sdk import ComputerUseClient

BASE_URL = os.environ.get("ACU_BASE_URL", "http://127.0.0.1:3000")
TIMEOUT_S = int(os.environ.get("ACU_QEMU_LIVE_VIEW_TIMEOUT_S", "1800"))

client = ComputerUseClient(base_url=BASE_URL)
started = time.time()


def create_qemu_session(**payload):
    last_error = None
    for _ in range(3):
        try:
            return client.create_session(**payload)["session"]
        except Exception as exc:
            last_error = exc
            time.sleep(2)
    raise last_error


def fetch_bytes(path: str) -> bytes:
    with urllib.request.urlopen(f"{BASE_URL}{path}") as response:
        return response.read()


def fetch_screenshot(session_id: str, destination: Path) -> str:
    destination.write_bytes(fetch_bytes(f"/api/sessions/{session_id}/screenshot"))
    return str(destination)


def export_review_bundle(session_id: str) -> dict:
    payload = client.export_review_bundle(session_id)
    bundle = payload.get("bundle") or {}
    review_recording = payload.get("review_recording") or {}
    bundle_path = Path(bundle.get("path") or "")
    if bundle.get("kind") != "review_bundle":
        raise SystemExit(f"expected review_bundle export kind, got: {payload}")
    if review_recording.get("mode") != "sparse_timeline":
        raise SystemExit(f"expected sparse_timeline review recording, got: {payload}")
    if review_recording.get("status") != "exported":
        raise SystemExit(f"expected exported review recording status, got: {payload}")
    if not bundle_path.exists():
        raise SystemExit(f"expected exported review bundle path to exist, got: {bundle_path}")
    for required_name in ("review.json", "timeline.jsonl"):
        if not (bundle_path / required_name).exists():
            raise SystemExit(f"expected exported review bundle to contain {required_name}: {bundle_path}")
    return {
        "payload": payload,
        "path": str(bundle_path),
        "manifest_path": str(bundle_path / "review.json"),
        "timeline_path": str(bundle_path / "timeline.jsonl"),
    }


session = create_qemu_session(provider="qemu", qemu_profile="product", width=1440, height=900)
session_id = session["id"]

latest_session = session
while time.time() - started < TIMEOUT_S:
    latest_session = client.get_session(session_id)["session"]
    if (
        latest_session.get("bridge_status") == "runtime_ready"
        and latest_session.get("readiness_state") == "runtime_ready"
    ):
        break
    time.sleep(10)
else:
    raise SystemExit(f"timed out waiting for qemu product runtime_ready: {latest_session}")

live_view = latest_session.get("live_desktop_view") or {}
if live_view.get("mode") != "stream":
    raise SystemExit(f"expected qemu product stream live_desktop_view, got: {live_view}")
canonical_live_view = live_view.get("canonical_url")
if not canonical_live_view:
    raise SystemExit(f"missing canonical live view URL: {live_view}")
with urllib.request.urlopen(f"{BASE_URL}{canonical_live_view}") as response:
    live_view_probe = {
        "status": response.status,
        "content_type": response.headers.get("content-type"),
    }

task = client.create_task(
    session_id,
    "Launch xmessage, move the mouse, click visibly, type into GNOME search, and capture live desktop proof",
)["task"]
task_id = task["id"]

artifacts_dir = Path("artifacts")
artifacts_dir.mkdir(exist_ok=True)
before_screenshot = fetch_screenshot(session_id, artifacts_dir / "qemu-live-view-before.png")

xmessage_receipt = client.perform_action(
    session_id,
    {
        "kind": "run_command",
        "command": 'nohup xmessage -center "Live desktop demo" >/tmp/qemu-live-view-demo.out 2>/tmp/qemu-live-view-demo.err </dev/null &',
        "run_as_user": "desktop",
        "taskId": task_id,
    },
)
time.sleep(2)
mouse_move_receipt = client.perform_action(
    session_id,
    {"kind": "mouse_move", "x": 720, "y": 450, "taskId": task_id},
)
mouse_click_receipt = client.perform_action(
    session_id,
    {"kind": "mouse_click", "x": 720, "y": 450, "button": "left", "taskId": task_id},
)
time.sleep(1)
type_receipt = client.perform_action(
    session_id,
    {"kind": "type_text", "text": "hello from live desktop demo", "taskId": task_id},
)
time.sleep(2)

observation = client.get_observation(session_id)
after_screenshot = fetch_screenshot(session_id, artifacts_dir / "qemu-live-view-after.png")
runtime_review_dir = Path(latest_session["artifacts_dir"]) / "review"
review_export = export_review_bundle(session_id)
session_after_export = client.get_session(session_id)["session"]
exported_summary = session_after_export.get("review_recording") or {}
if (exported_summary.get("exported_bundle") or {}).get("path") != review_export["path"]:
    raise SystemExit(
        f"session metadata did not retain exported bundle path: summary={exported_summary} export={review_export}"
    )

before_bytes = Path(before_screenshot).read_bytes()
after_bytes = Path(after_screenshot).read_bytes()
before_hash = hashlib.sha256(before_bytes).hexdigest()
after_hash = hashlib.sha256(after_bytes).hexdigest()
if before_hash == after_hash:
    raise SystemExit("live desktop demo screenshots did not change")

result = {
    "task_id": "qemu-live-view-demo",
    "session": latest_session,
    "live_desktop_view": live_view,
    "live_view_probe": live_view_probe,
    "task": task,
    "xmessage_receipt": xmessage_receipt,
    "mouse_move_receipt": mouse_move_receipt,
    "mouse_click_receipt": mouse_click_receipt,
    "type_receipt": type_receipt,
    "observation": observation,
    "review_recording": exported_summary,
    "artifacts": {
        "before_screenshot": before_screenshot,
        "after_screenshot": after_screenshot,
    },
    "metrics": {
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 5,
        "before_sha256": before_hash,
        "after_sha256": after_hash,
    },
}
Path("artifacts/qemu-live-view-demo.json").write_text(json.dumps(result, indent=2))

try:
    client._request(f"/api/tasks/{task_id}/complete", "POST", {})
except Exception:
    pass
try:
    client._request(f"/api/sessions/{session_id}", "DELETE", {})
except Exception:
    pass
if runtime_review_dir.exists():
    raise SystemExit(f"expected runtime review dir to be removed after session deletion: {runtime_review_dir}")
if not Path(review_export["path"]).exists():
    raise SystemExit(f"expected exported review bundle to survive session deletion: {review_export['path']}")

result["review_export"] = {
    **review_export,
    "runtime_review_dir": str(runtime_review_dir),
    "runtime_review_dir_removed_after_delete": not runtime_review_dir.exists(),
    "exported_bundle_survived_session_delete": Path(review_export["path"]).exists(),
}
Path("artifacts/qemu-live-view-demo.json").write_text(json.dumps(result, indent=2))

print("wrote artifacts/qemu-live-view-demo.json")
