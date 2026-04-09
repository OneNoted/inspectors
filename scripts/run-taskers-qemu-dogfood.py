from __future__ import annotations

import json
import os
import re
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from python.sdk import ComputerUseClient

BASE_URL = os.environ.get("ACU_BASE_URL", "http://127.0.0.1:3000")
TIMEOUT_S = int(os.environ.get("ACU_TASKERS_QEMU_TIMEOUT_S", "1800"))
SHARED_HOST_PATH = os.environ.get("ACU_TASKERS_HOST_PATH", str((Path(__file__).resolve().parents[2] / "taskers").resolve()))

client = ComputerUseClient(base_url=BASE_URL)
started = time.time()


def create_qemu_session(client: ComputerUseClient, **payload):
    last_error = None
    for _ in range(3):
        try:
            return client.create_session(**payload)["session"]
        except Exception as exc:
            last_error = exc
            time.sleep(2)
    raise last_error
session = create_qemu_session(client, provider="qemu", qemu_profile="product", shared_host_path=SHARED_HOST_PATH)
session_id = session["id"]

while time.time() - started < TIMEOUT_S:
    latest_session = client._request(f"/api/sessions/{session_id}")["session"]
    if latest_session.get("bridge_status") == "runtime_ready" and latest_session.get("readiness_state") == "runtime_ready":
        break
    time.sleep(10)
else:
    raise SystemExit(f"timed out waiting for Taskers product guest runtime_ready: {latest_session}")

setup_cmd = r'''
set -e
mkdir -p /mnt/shared /opt/taskers-bundle
mountpoint -q /mnt/shared || mount -t 9p -o trans=virtio shared /mnt/shared
bundle=$(ls /mnt/shared/hostshare/dist/taskers-linux-bundle-*.tar.xz | sort | tail -n1)
rm -rf /opt/taskers-bundle/*
tar -xJf "$bundle" -C /opt/taskers-bundle
/opt/taskers-bundle/bin/taskersctl query tree > /tmp/taskers-before.json || true
DISPLAY=:0 /opt/taskers-bundle/bin/taskers >/tmp/taskers.log 2>&1 &
'''
setup_receipt = client.perform_action(session_id, {"kind": "run_command", "command": setup_cmd})

wait_window_cmd = r'''
set -e
for i in $(seq 1 60); do
  if xdotool search --name Taskers >/tmp/taskers-window-id 2>/dev/null; then
    xdotool search --name Taskers getwindowgeometry --shell > /tmp/taskers-geometry.env
    cat /tmp/taskers-geometry.env
    exit 0
  fi
  sleep 2
done
exit 1
'''
geometry_receipt = client.perform_action(session_id, {"kind": "run_command", "command": wait_window_cmd})
geometry_stdout = ((geometry_receipt.get("result") or {}).get("stdout") or "")
values = dict(re.findall(r'^(X|Y|WIDTH|HEIGHT)=(\d+)$', geometry_stdout, flags=re.MULTILINE))
if not values:
    raise SystemExit(f"failed to locate Taskers window: {geometry_receipt}")
window_x = int(values["X"])
window_y = int(values["Y"])

before_tree = client.perform_action(session_id, {"kind": "read_file", "path": "/tmp/taskers-before.json"})

attempt_receipts = []
after_tree = None
screenshot_receipt = None
for dx, dy in [(186, 18), (182, 22), (190, 18)]:
    attempt_receipts.append(
        client.perform_action(session_id, {"kind": "mouse_click", "x": window_x + dx, "y": window_y + dy, "button": "left"})
    )
    time.sleep(2)
    client.perform_action(session_id, {"kind": "run_command", "command": "/opt/taskers-bundle/bin/taskersctl query tree > /tmp/taskers-after.json"})
    after_tree = client.perform_action(session_id, {"kind": "read_file", "path": "/tmp/taskers-after.json"})
    screenshot_receipt = client.perform_action(session_id, {"kind": "browser_screenshot"})
    try:
        before_payload = json.loads((before_tree.get("result") or {}).get("contents") or "{}")
        after_payload = json.loads((after_tree.get("result") or {}).get("contents") or "{}")
        before_count = len(before_payload.get("workspaces", []))
        after_count = len(after_payload.get("workspaces", []))
        if after_count > before_count:
            break
    except Exception:
        pass

result = {
    "task_id": "taskers-qemu-dogfood",
    "session": latest_session,
    "setup_receipt": setup_receipt,
    "geometry_receipt": geometry_receipt,
    "before_tree": before_tree,
    "after_tree": after_tree,
    "click_attempts": attempt_receipts,
    "screenshot_receipt": screenshot_receipt,
    "metrics": {
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 4 + len(attempt_receipts),
    },
}
Path("artifacts").mkdir(exist_ok=True)
Path("artifacts/taskers-qemu-dogfood.json").write_text(json.dumps(result, indent=2))
print("wrote artifacts/taskers-qemu-dogfood.json")
