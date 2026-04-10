from __future__ import annotations

import json
import os
from dataclasses import dataclass
from typing import Any, Dict, Optional
from urllib import request


@dataclass
class ComputerUseClient:
    base_url: str = os.environ.get("ACU_BASE_URL", "http://127.0.0.1:3000")

    def _request(self, path: str, method: str = "GET", payload: Optional[Dict[str, Any]] = None) -> Any:
        data = None if payload is None else json.dumps(payload).encode("utf-8")
        req = request.Request(
            f"{self.base_url}{path}",
            method=method,
            data=data,
            headers={"Content-Type": "application/json"},
        )
        with request.urlopen(req) as response:
            body = response.read().decode("utf-8")
            return json.loads(body) if body else None

    def list_adapters(self) -> Any:
        return self._request("/api/adapters")

    def create_session(self, provider: str = "qemu", width: int = 1440, height: int = 900, **options: Any) -> Any:
        payload = {"provider": provider, "width": width, "height": height, **options}
        return self._request("/api/sessions", "POST", payload)

    def get_session(self, session_id: str) -> Any:
        return self._request(f"/api/sessions/{session_id}")

    def get_observation(self, session_id: str) -> Any:
        return self._request(f"/api/sessions/{session_id}/observation")

    def get_available_actions(self, session_id: str) -> Any:
        return self._request(f"/api/sessions/{session_id}/actions")

    def perform_action(self, session_id: str, action: Dict[str, Any]) -> Any:
        return self._request(f"/api/sessions/{session_id}/actions", "POST", action)

    def create_task(self, session_id: str, description: str) -> Any:
        return self._request("/api/tasks", "POST", {"session_id": session_id, "description": description})

    def get_task_status(self, task_id: str) -> Any:
        return self._request(f"/api/tasks/{task_id}")

    def pause(self, task_id: str) -> Any:
        return self._request(f"/api/tasks/{task_id}/pause", "POST", {})

    def resume(self, task_id: str) -> Any:
        return self._request(f"/api/tasks/{task_id}/resume", "POST", {})

    def reset(self, task_id: str) -> Any:
        return self._request(f"/api/tasks/{task_id}/reset", "POST", {})

    def terminate(self, task_id: str) -> Any:
        return self._request(f"/api/tasks/{task_id}/terminate", "POST", {})
