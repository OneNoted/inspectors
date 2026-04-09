from __future__ import annotations

import json
import time
import urllib.request
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from python.sdk import ComputerUseClient

client = ComputerUseClient()
started = time.time()
session = client.create_session()["session"]
session_id = session["id"]
latest_session = client.get_session(session_id)["session"]
live_view = latest_session.get("live_desktop_view") or {}
if live_view.get("mode") != "screenshot_poll":
    raise SystemExit(f"xvfb should advertise screenshot_poll live_desktop_view, got: {live_view}")
observation = client.get_observation(session_id)
open_browser = client.perform_action(session_id, {"kind": "browser_open", "url": "https://example.com"})
artifacts_dir = Path("artifacts")
artifacts_dir.mkdir(exist_ok=True)
with urllib.request.urlopen(f"{client.base_url}/api/sessions/{session_id}/screenshot") as response:
    (artifacts_dir / "smoke-eval.png").write_bytes(response.read())
duration_ms = int((time.time() - started) * 1000)
result = {
    "task_id": "smoke-eval",
    "session": latest_session,
    "live_desktop_view": live_view,
    "observation": observation,
    "open_browser": open_browser,
    "metrics": {
        "success": open_browser.get("status") == "ok",
        "duration_ms": duration_ms,
        "step_count": 3,
        "human_intervention": 0,
        "artifact_path": str(artifacts_dir / "smoke-eval.png"),
    },
}
Path('artifacts/smoke-eval.json').write_text(json.dumps(result, indent=2))
print('wrote artifacts/smoke-eval.json')
