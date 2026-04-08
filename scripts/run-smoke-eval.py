from __future__ import annotations

import json
import time
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from python.sdk import ComputerUseClient

client = ComputerUseClient()
started = time.time()
session = client.create_session()["session"]
observation = client.get_observation(session["id"])
open_browser = client.perform_action(session["id"], {"kind": "browser_open", "url": "https://example.com"})
duration_ms = int((time.time() - started) * 1000)
result = {
    "task_id": "smoke-eval",
    "session": session,
    "observation": observation,
    "open_browser": open_browser,
    "metrics": {
        "success": open_browser.get("status") == "ok",
        "duration_ms": duration_ms,
        "step_count": 2,
        "human_intervention": 0,
        "artifact_path": observation.get("screenshot", {}).get("artifact_path"),
    },
}
Path('artifacts').mkdir(exist_ok=True)
Path('artifacts/smoke-eval.json').write_text(json.dumps(result, indent=2))
print('wrote artifacts/smoke-eval.json')
