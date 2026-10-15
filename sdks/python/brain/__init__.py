"""Small synchronous Brain client. Requests are never automatically retried."""
from __future__ import annotations

import json
from collections.abc import Iterator
from typing import Any
from urllib.error import HTTPError
from urllib.parse import quote, urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener

from .types import CreateSession, Event, Input, Session


class BrainError(Exception):
    def __init__(self, status: int, code: str, message: str, request_id: str | None = None):
        super().__init__(message)
        self.status, self.code, self.request_id = status, code, request_id


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Brain:
    def __init__(self, url: str, token: str, timeout: float = 30):
        self.url, self.token, self.timeout = url.rstrip("/"), token, timeout
        self._opener = build_opener(_NoRedirect())

    def _open(self, method: str, path: str, body: Any = None, stream: bool = False):
        headers = {"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"}
        if stream:
            headers["Accept"] = "text/event-stream"
        req = Request(self.url + path, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method)
        try:
            return self._opener.open(req, timeout=self.timeout)
        except HTTPError as error:
            try:
                payload = json.loads(error.read(1 << 20))
            except (ValueError, UnicodeError):
                payload = {}
            finally:
                error.close()
            detail = payload.get("error", {})
            raise BrainError(error.code, detail.get("code", "http_error"), detail.get("message", str(error)), payload.get("request_id")) from error

    def _request(self, method: str, path: str, body: Any = None) -> Any:
        with self._open(method, path, body) as response:
            return json.load(response)

    def create(self, config: CreateSession) -> Session:
        return self._request("POST", "/sessions", config)

    def get(self, session_id: str) -> Session:
        return self._request("GET", f"/sessions/{quote(session_id, safe='')}")

    def list(self, after: str = "", limit: int = 100) -> dict[str, Any]:
        return self._request("GET", "/sessions?" + urlencode({"after": after, "limit": limit}))

    def send(self, session_id: str, event: Input) -> Event:
        return self._request("POST", f"/sessions/{quote(session_id, safe='')}/events", event)

    def interrupt(self, session_id: str) -> None:
        self.send(session_id, {"type": "user.interrupt"})

    def resume(self, session_id: str) -> Session:
        return self._request("POST", f"/sessions/{quote(session_id, safe='')}/resume")

    def close(self, session_id: str) -> None:
        self._request("POST", f"/sessions/{quote(session_id, safe='')}/close")

    def catalog(self) -> dict[str, Any]:
        return self._request("GET", "/catalog")

    def history(self, session_id: str, after: str = "", limit: int = 100) -> dict[str, Any]:
        return self._request("GET", f"/sessions/{quote(session_id, safe='')}/events?" + urlencode({"after": after, "limit": limit}))

    def observe(self, session_id: str, after: str = "") -> Iterator[Event]:
        path = f"/sessions/{quote(session_id, safe='')}/events?" + urlencode({"after": after})
        with self._open("GET", path, stream=True) as response:
            data: list[str] = []
            size = 0
            while True:
                line = response.readline((4 << 20) + 1)
                size += len(line)
                if size > 4 << 20:
                    raise ValueError("event exceeds client limit")
                if not line:
                    if data:
                        raise ValueError("incomplete event stream")
                    return
                text = line.decode().rstrip("\r\n")
                if not text:
                    if data:
                        yield json.loads("\n".join(data))
                    data, size = [], 0
                elif text.startswith("data:"):
                    data.append(text[5:].removeprefix(" "))

