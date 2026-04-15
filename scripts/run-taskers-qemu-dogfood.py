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
TIMEOUT_S = int(os.environ.get("ACU_TASKERS_QEMU_TIMEOUT_S", "1800"))
SHARED_HOST_PATH = os.environ.get(
    "ACU_TASKERS_HOST_PATH",
    str((Path(__file__).resolve().parents[2] / "taskers").resolve()),
)
TASKERS_BUNDLE_ENV = os.environ.get("ACU_TASKERS_BUNDLE")

client = ComputerUseClient(base_url=BASE_URL)
started = time.time()


def fetch_screenshot(session_id: str, destination: Path) -> str:
    with urllib.request.urlopen(f"{BASE_URL}/api/sessions/{session_id}/screenshot") as response:
        destination.write_bytes(response.read())
    return str(destination)


def screenshot_sha256(path: str) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def create_qemu_session(**payload):
    last_error = None
    for _ in range(3):
        try:
            return client.create_session(**payload)["session"]
        except Exception as exc:
            last_error = exc
            time.sleep(2)
    raise last_error


def resolve_taskers_bundle() -> str:
    if TASKERS_BUNDLE_ENV:
        return TASKERS_BUNDLE_ENV

    shared_host_root = Path(SHARED_HOST_PATH)
    dist_dir = shared_host_root / "dist"
    generic_bundle = dist_dir / "taskers-linux-bundle-x86_64-unknown-linux-gnu.tar.xz"
    if generic_bundle.exists():
        return f"/mnt/shared/hostshare/dist/{generic_bundle.name}"

    versioned_bundles = sorted(
        dist_dir.glob("taskers-linux-bundle-v*-x86_64-unknown-linux-gnu.tar.xz"),
        key=lambda path: path.stat().st_mtime,
    )
    if versioned_bundles:
        return f"/mnt/shared/hostshare/dist/{versioned_bundles[-1].name}"

    raise SystemExit(
        f"could not find a Taskers Linux bundle under {dist_dir}; set ACU_TASKERS_BUNDLE explicitly"
    )


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
        "path": str(bundle_path),
        "manifest_path": str(bundle_path / "review.json"),
        "timeline_path": str(bundle_path / "timeline.jsonl"),
    }


def wait_for_taskers_visibility(session_id: str, baseline_hash: str) -> dict:
    deadline = time.time() + 45
    latest_observation = {}
    latest_screenshot_hash = baseline_hash
    while time.time() < deadline:
        latest_observation = client.get_observation(session_id)
        active_window = latest_observation.get("active_window") or {}
        screenshot_path = fetch_screenshot(session_id, Path("artifacts") / "taskers-qemu-launch.png")
        latest_screenshot_hash = screenshot_sha256(screenshot_path)
        window_text = " ".join(
            str(active_window.get(key, "")).lower()
            for key in ("title", "class_name")
        )
        if "taskers" in window_text or latest_screenshot_hash != baseline_hash:
            return {
                "observation": latest_observation,
                "screenshot_path": screenshot_path,
                "screenshot_sha256": latest_screenshot_hash,
            }
        time.sleep(2)
    raise SystemExit(
        f"Taskers did not become visibly active in time: observation={latest_observation} baseline={baseline_hash} latest={latest_screenshot_hash}"
    )


TASKERS_BUNDLE = resolve_taskers_bundle()


session = create_qemu_session(
    provider="qemu",
    qemu_profile="product",
    shared_host_path=SHARED_HOST_PATH,
    width=1440,
    height=900,
)
session_id = session["id"]

while time.time() - started < TIMEOUT_S:
    latest_session = client._request(f"/api/sessions/{session_id}")["session"]
    if (
        latest_session.get("bridge_status") == "runtime_ready"
        and latest_session.get("readiness_state") == "runtime_ready"
    ):
        break
    time.sleep(10)
else:
    raise SystemExit(
        f"timed out waiting for Taskers product guest runtime_ready: {latest_session}"
    )

live_view = latest_session.get("live_desktop_view") or {}
if live_view.get("mode") != "stream":
    raise SystemExit(f"Taskers product guest must advertise stream live_desktop_view, got: {live_view}")
canonical_live_view = live_view.get("canonical_url")
if not canonical_live_view:
    raise SystemExit(f"Taskers product guest is missing canonical live view URL: {live_view}")
with urllib.request.urlopen(f"{BASE_URL}{canonical_live_view}") as response:
    live_view_probe = {
        "status": response.status,
        "content_type": response.headers.get("content-type"),
    }

task = client.create_task(
    session_id,
    "Launch Taskers in qemu product mode, verify the visible GUI appears, exercise desktop input, create Workspace 2, and capture proof artifacts",
)["task"]
task_id = task["id"]

artifacts_dir = Path("artifacts")
artifacts_dir.mkdir(exist_ok=True)
baseline_screenshot = fetch_screenshot(session_id, artifacts_dir / "taskers-qemu-baseline.png")
baseline_hash = screenshot_sha256(baseline_screenshot)

setup_cmd = f"""set -e
install -d -o ubuntu -g ubuntu /home/ubuntu/taskers-bundle
mountpoint -q /mnt/shared || sudo mount -t 9p -o trans=virtio shared /mnt/shared
test -f "{TASKERS_BUNDLE}"
rm -rf /home/ubuntu/taskers-bundle/*
tar -xJf "{TASKERS_BUNDLE}" -C /home/ubuntu/taskers-bundle
chown -R ubuntu:ubuntu /home/ubuntu/taskers-bundle
"""
setup_receipt = client.perform_action(
    session_id,
    {"kind": "run_command", "command": setup_cmd, "taskId": task_id},
)
launch_receipt = client.perform_action(
    session_id,
    {
        "kind": "run_command",
        "command": (
            "LIBGL_ALWAYS_SOFTWARE=1 "
            "MESA_LOADER_DRIVER_OVERRIDE=llvmpipe "
            "GDK_BACKEND=x11 "
            "XDG_SESSION_TYPE=x11 "
            "GTK_USE_PORTAL=0 "
            "NO_AT_BRIDGE=1 "
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 "
            "nohup /home/ubuntu/taskers-bundle/bin/taskers >/tmp/taskers.log 2>&1 &"
        ),
        "run_as_user": "desktop",
        "taskId": task_id,
    },
)
launch_visibility = wait_for_taskers_visibility(session_id, baseline_hash)
visibility_observation = launch_visibility["observation"]
mouse_move_receipt = client.perform_action(
    session_id,
    {"kind": "mouse_move", "x": 720, "y": 420, "taskId": task_id},
)
mouse_click_receipt = client.perform_action(
    session_id,
    {"kind": "mouse_click", "x": 720, "y": 420, "button": "left", "taskId": task_id},
)
time.sleep(1)
before_tree_receipt = client.perform_action(
    session_id,
    {
        "kind": "run_command",
        "command": '/home/ubuntu/taskers-bundle/bin/taskersctl query tree > /tmp/taskers-before.json',
        "run_as_user": "desktop",
        "taskId": task_id,
    },
)
before_tree = client.perform_action(
    session_id,
    {"kind": "read_file", "path": "/tmp/taskers-before.json", "taskId": task_id},
)
before_screenshot = fetch_screenshot(session_id, artifacts_dir / "taskers-qemu-before.png")
before_hash = screenshot_sha256(before_screenshot)

workspace_cmd = (
    '/home/ubuntu/taskers-bundle/bin/taskersctl workspace new --label "Workspace 2" '
    '&& /home/ubuntu/taskers-bundle/bin/taskersctl query tree > /tmp/taskers-after.json'
)
workspace_receipt = client.perform_action(
    session_id,
    {
        "kind": "run_command",
        "command": workspace_cmd,
        "run_as_user": "desktop",
        "taskId": task_id,
    },
)
after_tree = client.perform_action(
    session_id,
    {"kind": "read_file", "path": "/tmp/taskers-after.json", "taskId": task_id},
)
time.sleep(2)
after_screenshot = fetch_screenshot(session_id, artifacts_dir / "taskers-qemu-after.png")
after_hash = screenshot_sha256(after_screenshot)
log_receipt = client.perform_action(
    session_id,
    {
        "kind": "run_command",
        "command": "ps -ef | grep -i taskers | grep -v grep",
        "taskId": task_id,
    },
)
runtime_review_dir = Path(latest_session["artifacts_dir"]) / "review"
review_export = export_review_bundle(session_id)
session_after_export = client.get_session(session_id)["session"]
exported_summary = session_after_export.get("review_recording") or {}
if (exported_summary.get("exported_bundle") or {}).get("path") != review_export["path"]:
    raise SystemExit(
        f"session metadata did not retain exported bundle path: summary={exported_summary} export={review_export}"
    )

before_payload = json.loads((before_tree.get("result") or {}).get("contents") or "{}")
after_payload = json.loads((after_tree.get("result") or {}).get("contents") or "{}")
before_count = len((before_payload or {}).get("workspaces", {}))
after_count = len((after_payload or {}).get("workspaces", {}))
labels = [
    workspace.get("label")
    for workspace in (after_payload or {}).get("workspaces", {}).values()
]

if after_count <= before_count or "Workspace 2" not in labels:
    raise SystemExit(
        f"Taskers proof did not create a new workspace: before={before_count} after={after_count} labels={labels}"
    )
if baseline_hash == after_hash and before_hash == after_hash:
    raise SystemExit("Taskers proof did not produce a visible desktop change across captured screenshots")

result = {
    "task_id": "taskers-qemu-dogfood",
    "selected_app": "Taskers",
    "selected_bundle": TASKERS_BUNDLE,
    "session": latest_session,
    "live_desktop_view": live_view,
    "live_view_probe": live_view_probe,
    "task": task,
    "visibility_observation": visibility_observation,
    "setup_receipt": setup_receipt,
    "launch_receipt": launch_receipt,
    "mouse_move_receipt": mouse_move_receipt,
    "mouse_click_receipt": mouse_click_receipt,
    "before_tree_receipt": before_tree_receipt,
    "before_tree": before_tree,
    "workspace_receipt": workspace_receipt,
    "after_tree": after_tree,
    "taskers_processes": log_receipt,
    "review_recording": exported_summary,
    "artifacts": {
        "baseline_screenshot": baseline_screenshot,
        "launch_screenshot": launch_visibility["screenshot_path"],
        "before_screenshot": before_screenshot,
        "after_screenshot": after_screenshot,
    },
    "metrics": {
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 7,
        "baseline_sha256": baseline_hash,
        "launch_sha256": launch_visibility["screenshot_sha256"],
        "before_sha256": before_hash,
        "after_sha256": after_hash,
        "workspace_count_before": before_count,
        "workspace_count_after": after_count,
    },
}
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
Path("artifacts/taskers-qemu-dogfood.json").write_text(json.dumps(result, indent=2))
print("wrote artifacts/taskers-qemu-dogfood.json")
