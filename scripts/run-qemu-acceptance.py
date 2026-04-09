from __future__ import annotations

import json
import os
import sys
import time
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

install_command = "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y jq && jq --version > /tmp/acu-jq-version.txt"
install_receipt = client.perform_action(session_id, {"kind": "run_command", "command": install_command})
read_receipt = client.perform_action(session_id, {"kind": "read_file", "path": "/tmp/acu-jq-version.txt"})
observation = client.get_observation(session_id)

result = {
    "task_id": "qemu-acceptance",
    "profile": PROFILE,
    "session": latest_session,
    "install_receipt": install_receipt,
    "read_receipt": read_receipt,
    "observation": observation,
    "metrics": {
        "success": install_receipt.get("status") == "ok" and read_receipt.get("status") == "ok",
        "duration_ms": int((time.time() - started) * 1000),
        "step_count": 3,
        "human_intervention": 0,
        "artifact_path": observation.get("screenshot", {}).get("artifact_path"),
    },
}
Path("artifacts").mkdir(exist_ok=True)
Path("artifacts/qemu-acceptance.json").write_text(json.dumps(result, indent=2))
print("wrote artifacts/qemu-acceptance.json")
