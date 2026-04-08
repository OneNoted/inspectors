from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from python.sdk import ComputerUseClient

# This demo assumes a GUI editor command is available inside the sandbox image.
EDITOR_COMMAND = "firefox"

client = ComputerUseClient()
session = client.create_session()["session"]
client.perform_action(session["id"], {"kind": "open_app", "name": EDITOR_COMMAND})
print(client.get_available_actions(session["id"]))
