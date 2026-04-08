from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from python.sdk import ComputerUseClient

client = ComputerUseClient()
session = client.create_session()["session"]
client.perform_action(session["id"], {"kind": "browser_open", "url": "https://example.com"})
observation = client.get_observation(session["id"])
print(observation.get("summary"))
