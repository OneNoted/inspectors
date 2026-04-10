from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from python.sdk import ComputerUseClient

client = ComputerUseClient()
session = client.create_session(provider="xvfb")["session"]
client.perform_action(session["id"], {"kind": "run_command", "command": "printf hello > /tmp/acu-demo.txt"})
result = client.perform_action(session["id"], {"kind": "read_file", "path": "/tmp/acu-demo.txt"})
print(result)
