from __future__ import annotations

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
TASKERS_BUNDLE = os.environ.get(
    "ACU_TASKERS_BUNDLE",
    "/mnt/shared/hostshare/dist/taskers-linux-bundle-v0.3.1-x86_64-unknown-linux-gnu.tar.xz",
)

client = ComputerUseClient(base_url=BASE_URL)
started = time.time()


def fetch_screenshot(session_id: str, destination: Path) -> str:
    with urllib.request.urlopen(f"{BASE_URL}/api/sessions/{session_id}/screenshot") as response:
        destination.write_bytes(response.read())
    return str(destination)


def create_qemu_session(**payload):
    last_error = None
    for _ in range(3):
        try:
            return client.create_session(**payload)["session"]
        except Exception as exc:
            last_error = exc
            time.sleep(2)
    raise last_error


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

task = client.create_task(
    session_id,
    "Launch Taskers, click a visible desktop button, create Workspace 2, and capture proof artifacts",
)["task"]
task_id = task["id"]

setup_cmd = f"""set -e
mkdir -p /mnt/shared "$HOME/taskers-bundle"
mountpoint -q /mnt/shared || sudo mount -t 9p -o trans=virtio shared /mnt/shared
test -f "{TASKERS_BUNDLE}"
rm -rf "$HOME/taskers-bundle"/*
tar -xJf "{TASKERS_BUNDLE}" -C "$HOME/taskers-bundle"
WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 DISPLAY=:0 "$HOME/taskers-bundle/bin/taskers" >/tmp/taskers.log 2>&1 &
sleep 5
"$HOME/taskers-bundle/bin/taskersctl" query tree > /tmp/taskers-before.json
"""
setup_receipt = client.perform_action(
    session_id,
    {"kind": "run_command", "command": setup_cmd, "taskId": task_id},
)

artifacts_dir = Path("artifacts")
artifacts_dir.mkdir(exist_ok=True)
before_screenshot = fetch_screenshot(session_id, artifacts_dir / "taskers-qemu-before.png")

# Visible GUI interaction: click the Taskers icon in the centered bottom dock.
click_receipt = client.perform_action(
    session_id,
    {"kind": "mouse_click", "x": 648, "y": 694, "button": "left", "taskId": task_id},
)
time.sleep(2)

workspace_cmd = (
    '"$HOME/taskers-bundle/bin/taskersctl" workspace new --label "Workspace 2" '
    '&& "$HOME/taskers-bundle/bin/taskersctl" query tree > /tmp/taskers-after.json'
)
workspace_receipt = client.perform_action(
    session_id,
    {"kind": "run_command", "command": workspace_cmd, "taskId": task_id},
)
after_tree = client.perform_action(
    session_id,
    {"kind": "read_file", "path": "/tmp/taskers-after.json", "taskId": task_id},
)
before_tree = client.perform_action(
    session_id,
    {"kind": "read_file", "path": "/tmp/taskers-before.json", "taskId": task_id},
)
time.sleep(2)
after_screenshot = fetch_screenshot(session_id, artifacts_dir / "taskers-qemu-after.png")
log_receipt = client.perform_action(
    session_id,
    {"kind": "read_file", "path": "/tmp/taskers.log", "taskId": task_id},
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

result = {
    "task_id": "taskers-qemu-dogfood",
    "session": latest_session,
    "task": task,
    "setup_receipt": setup_receipt,
    "click_receipt": click_receipt,
    "workspace_receipt": workspace_receipt,
    "before_tree": before_tree,
    "after_tree": after_tree,
    "taskers_log": log_receipt,
    "artifacts": {
        "before_screenshot": before_screenshot,
        "after_screenshot": after_screenshot,
    },
    "metrics": {
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 5,
        "workspace_count_before": before_count,
        "workspace_count_after": after_count,
    },
}
Path("artifacts/taskers-qemu-dogfood.json").write_text(json.dumps(result, indent=2))
try:
    client.complete(task_id)
except Exception:
    pass
try:
    client._request(f"/api/sessions/{session_id}", "DELETE", {})
except Exception:
    pass
print("wrote artifacts/taskers-qemu-dogfood.json")
