"""Client-owned function execution over the non-replayable SSE/HTTP bridge."""
from __future__ import annotations

import json
import threading
from dataclasses import dataclass
from typing import Any, Callable


@dataclass
class RuntimeContext:
    cancelled: threading.Event
    scope: dict[str, Any]
    progress: Callable[[Any], None]


@dataclass
class RuntimeTool:
    name: str
    description: str
    input_schema: dict[str, Any]
    run: Callable[[Any, RuntimeContext], Any]
    output_schema: dict[str, Any] | None = None

    def descriptor(self):
        value = {"name": self.name, "description": self.description, "input_schema": self.input_schema}
        if self.output_schema is not None:
            value["output_schema"] = self.output_schema
        return value


class RuntimeExecutor:
    def __init__(self, brain, tools: list[RuntimeTool], session: str | None = None):
        from . import Brain
        registration = {"binding": "runtime", "tools": [t.descriptor() for t in tools]}
        if session:
            registration["session"] = session
        credential = brain._request("POST", "/executors", registration)
        self.id = credential["id"]
        self.client = Brain(brain.url, credential["token"], brain.timeout)
        self.tools = {t.name: t for t in tools}
        self.connection = ""
        self.calls: dict[str, threading.Event] = {}
        self.lock = threading.Lock()
        self.ready = threading.Event()
        self.stopped = threading.Event()
        self.error: Exception | None = None
        self.response = None
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()
        if not self.ready.wait(30) or self.error:
            self.close()
            raise RuntimeError("runtime executor did not become ready") from self.error

    def _reply(self, call, **payload):
        self.client._request("POST", f"/executors/{self.id}/responses", {"connection": self.connection, "call_id": call, **payload})

    def _invoke(self, command, cancelled):
        call = command["call_id"]
        try:
            def progress(value):
                if not cancelled.is_set():
                    self._reply(call, progress=value)
            try:
                value = self.tools[command["tool"]].run(command.get("arguments"), RuntimeContext(cancelled, command["scope"], progress))
                result = {"content": [{"type": "text", "text": json.dumps(value)}], "structured_content": value}
            except Exception as error:
                result = {"content": [{"type": "text", "text": str(error)}], "is_error": True}
            if not cancelled.is_set():
                self._reply(call, result=result)
        except Exception as error:
            self.error = error
            self.stopped.set()
            # A lost result is never resent. Closing the executor classifies all
            # dispatched calls as unknown outcomes on the server.
            try:
                self.client._request("POST", f"/executors/{self.id}/close")
            except Exception:
                pass
        finally:
            with self.lock:
                self.calls.pop(call, None)

    def _receive(self, command):
        kind = command["type"]
        if kind == "ready":
            self.connection = command["connection"]
            self.ready.set()
            return
        if command["connection"] != self.connection:
            raise ValueError("stale executor command")
        if kind == "ping":
            return
        with self.lock:
            call = command["call_id"]
            if kind == "cancel":
                if call in self.calls:
                    self.calls[call].set()
                return
            if kind != "call" or call in self.calls or len(self.calls) >= 32 or command["tool"] not in self.tools:
                raise ValueError("invalid runtime call")
            cancelled = threading.Event()
            self.calls[call] = cancelled
        threading.Thread(target=self._invoke, args=(command, cancelled), daemon=True).start()

    def _read(self):
        try:
            with self.client._open("GET", f"/executors/{self.id}/commands", stream=True) as response:
                self.response = response
                data, size = [], 0
                while not self.stopped.is_set():
                    line = response.readline((4 << 20) + 1)
                    if not line:
                        raise ConnectionError("runtime executor disconnected")
                    size += len(line)
                    if size > 4 << 20:
                        raise ValueError("runtime command exceeds client limit")
                    text = line.decode().rstrip("\r\n")
                    if not text:
                        if data:
                            self._receive(json.loads("\n".join(data)))
                        data, size = [], 0
                    elif text.startswith("data:"):
                        data.append(text[5:].removeprefix(" "))
        except Exception as error:
            if not self.stopped.is_set():
                self.error = error
        finally:
            self.ready.set()
            self.stopped.set()
            with self.lock:
                for event in self.calls.values():
                    event.set()

    def close(self):
        from . import BrainError
        self.stopped.set()
        with self.lock:
            for event in self.calls.values():
                event.set()
        try:
            self.client._request("POST", f"/executors/{self.id}/close")
        except BrainError as error:
            # Closing a session elsewhere already revokes its executor token.
            if error.status not in (401, 403):
                raise
        self.thread.join(timeout=30)
        if self.thread.is_alive():
            raise TimeoutError("runtime command stream did not close")
