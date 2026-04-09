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
PROFILE = os.environ.get("ACU_QEMU_PROFILE", "regression")
TIMEOUT_S = int(os.environ.get("ACU_QEMU_ACCEPTANCE_TIMEOUT_S", "900"))
SHARED_HOST_PATH = os.environ.get("ACU_QEMU_SHARED_HOST_PATH")

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
create_options = {"qemu_profile": PROFILE}
if SHARED_HOST_PATH:
    create_options["shared_host_path"] = SHARED_HOST_PATH
session = create_qemu_session(client, provider="qemu", width=1440, height=900, **create_options)
session_id = session["id"]

latest_session = session
while time.time() - started < TIMEOUT_S:
    latest_session = client._request(f"/api/sessions/{session_id}")["session"]
    if latest_session.get("bridge_status") == "runtime_ready" and latest_session.get("readiness_state") == "runtime_ready":
        break
    time.sleep(5)
else:
    raise SystemExit(f"timed out waiting for runtime_ready: {latest_session}")

live_view = latest_session.get("live_desktop_view") or {}
expected_mode = "stream" if PROFILE == "product" else "screenshot_poll"
if live_view.get("mode") != expected_mode:
    raise SystemExit(f"unexpected live_desktop_view for profile={PROFILE}: {live_view}")
canonical_url = live_view.get("canonical_url")
if canonical_url:
    with urllib.request.urlopen(f"{BASE_URL}{canonical_url}") as response:
        live_view_probe = {
            "status": response.status,
            "content_type": response.headers.get("content-type"),
        }
else:
    live_view_probe = {"status": None, "content_type": None}

install_command = "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y jq && jq --version > /tmp/acu-jq-version.txt"
install_receipt = client.perform_action(session_id, {"kind": "run_command", "command": install_command})
read_receipt = client.perform_action(session_id, {"kind": "read_file", "path": "/tmp/acu-jq-version.txt"})
observation = client.get_observation(session_id)
artifacts_dir = Path("artifacts")
artifacts_dir.mkdir(exist_ok=True)
with urllib.request.urlopen(f"{BASE_URL}/api/sessions/{session_id}/screenshot") as response:
    (artifacts_dir / "qemu-acceptance.png").write_bytes(response.read())

result = {
    "task_id": "qemu-acceptance",
    "profile": PROFILE,
    "session": latest_session,
    "live_desktop_view": live_view,
    "live_view_probe": live_view_probe,
    "install_receipt": install_receipt,
    "read_receipt": read_receipt,
    "observation": observation,
    "metrics": {
        "success": install_receipt.get("status") == "ok" and read_receipt.get("status") == "ok",
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 3,
        "human_intervention": 0,
        "artifact_path": str(artifacts_dir / "qemu-acceptance.png"),
    },
}
Path("artifacts/qemu-acceptance.json").write_text(json.dumps(result, indent=2))
try:
    client._request(f"/api/sessions/{session_id}", "DELETE", {})
except Exception:
    pass
print("wrote artifacts/qemu-acceptance.json")
