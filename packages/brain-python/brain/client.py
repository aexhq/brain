from __future__ import annotations

import json
from http.client import HTTPException
from collections.abc import Callable, Iterator
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import uuid4

from . import generated as wire


class SessionError(RuntimeError):
    def __init__(self, code: str, message: str, status: int = 0, request_id: str | None = None):
        super().__init__(message)
        self.code = code
        self.status = status
        self.uncertain = code == "uncertain"
        self.request_id = request_id


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def _segment(value: str) -> str:
    return quote(value, safe="")


class Brain:
    def __init__(self, url: str = "http://127.0.0.1:8080", token: str | None = None, timeout: float = 30):
        self.url = url.rstrip("/")
        self.token = token
        self.timeout = timeout
        self._opener = build_opener(_NoRedirect())

    def _open(self, method: str, path: str, body=None, *, token=None, media_type="application/json", headers=None):
        request_id = body.get("request_id") if isinstance(body, dict) else None
        request_headers = dict(headers or {})
        authority = self.token if token is None else token
        if authority is not None:
            request_headers["Authorization"] = "Bearer " + authority
        if body is not None:
            request_headers["Content-Type"] = media_type
            if not isinstance(body, bytes):
                body = json.dumps(body, allow_nan=False, separators=(",", ":")).encode()
        request = Request(self.url + path, body, request_headers, method=method)
        try:
            return self._opener.open(request, timeout=self.timeout)
        except HTTPError as error:
            try:
                raw = error.read(65537)
                if len(raw) > 65536:
                    raise ValueError("oversized error")
                detail = json.loads(raw)
            except (ValueError, UnicodeError, HTTPException, OSError):
                detail = {"code": "unavailable" if method == "GET" else "uncertain", "message": "Invalid server error"}
            finally:
                error.close()
            raise SessionError(detail["code"], detail["message"], error.code, detail.get("request_id", request_id)) from None
        except (URLError, TimeoutError, OSError):
            raise SessionError("unavailable" if method == "GET" else "uncertain",
                               "Read failed" if method == "GET" else
                               "Mutation may have committed; reconcile its original request_id", request_id=request_id) from None

    def request(self, method: str, path: str, body=None, *, binary=False, **options) -> Any:
        request_id = body.get("request_id") if isinstance(body, dict) else None
        try:
            with self._open(method, path, body, **options) as response:
                maximum = (16 if binary else 4) * 1024 * 1024
                raw = response.read(maximum + 1)
                if len(raw) > maximum:
                    raise ValueError("oversized response")
                return raw if binary else json.loads(raw) if raw else None
        except (HTTPException, OSError, ValueError, UnicodeError):
            raise SessionError("unavailable" if method == "GET" else "uncertain",
                               "Response was unavailable or invalid", request_id=request_id) from None

    def create(self, config: wire.SessionConfig, message: str | None = None, request_id: str | None = None) -> Session:
        request = {"request_id": request_id or str(uuid4()), "config": config}
        if message is not None:
            request["message"] = message
        view = self.request("POST", "/v2/sessions", request)
        return Session(self, view["id"])

    def session(self, session_id: str) -> Session:
        return Session(self, session_id)

    def identity(self) -> wire.IdentityView:
        return self.request("GET", "/v2/identity")

    def list(self, after: str = "", limit: int = 64) -> wire.SessionPage:
        return self.request("GET", f"/v2/sessions?after={_segment(after)}&limit={limit}")

    def sessions(self) -> Iterator[wire.SessionView]:
        after = ""
        while True:
            page = self.list(after)
            yield from page["sessions"]
            cursor = page.get("next")
            if not cursor:
                return
            if cursor == after:
                raise SessionError("corrupt", "Session cursor did not advance")
            after = cursor

    def catalog(self) -> list[wire.CatalogEntry]:
        return self.request("GET", "/v2/catalog")

    def nodes(self, after: str = "", limit: int = 64) -> list[wire.NodeInfo]:
        return self.request("GET", f"/operator/nodes?after={_segment(after)}&limit={limit}")

    def admit_package(self, request: wire.PackageAdmission) -> wire.Package:
        return self.request("POST", "/v2/packages", request)

    def package(self, package_id: str) -> wire.Package:
        return self.request("GET", "/v2/packages/" + _segment(package_id))


class Session:
    def __init__(self, client: Brain, session_id: str):
        self.client = client
        self.id = session_id
        self.path = "/v2/sessions/" + _segment(session_id)

    def _request(self, method, suffix="", body=None, **options):
        return self.client.request(method, self.path + suffix, body, **options)

    def read(self) -> wire.SessionView:
        return self._request("GET")

    def input(self, data: wire.InputData, request_id: str | None = None) -> wire.CommandReceipt:
        return self._request("POST", "/inputs", {"request_id": request_id or str(uuid4()), "data": data})

    def send(self, text: str, request_id: str | None = None) -> wire.CommandReceipt:
        return self.input({"kind": "message", "text": text}, request_id)

    def event(self, name: str, version: str, data: Any, *, request_id=None, correlation=None) -> wire.CommandReceipt:
        event = {"kind": "event", "name": name, "version": version, "data": data}
        if correlation is not None:
            event["correlation"] = correlation
        return self.input(event, request_id)

    def invoke(self, name: str, arguments: Any, request_id=None) -> wire.CommandReceipt:
        return self.input({"kind": "action", "name": name, "arguments": arguments}, request_id)

    def control(self, control: wire.ControlKind, request_id=None) -> wire.CommandReceipt:
        return self.input({"kind": "control", "control": control}, request_id)

    def command(self, command_id: str) -> wire.CommandReceipt:
        return self._request("GET", "/commands/" + _segment(command_id))

    def records(self, after: str = "0", limit: int = 64) -> wire.RecordPage:
        return self._request("GET", f"/records?after={_segment(after)}&limit={limit}")

    def runs(self, limit: int = 64, after: str = "") -> list[wire.Run]:
        return self._request("GET", f"/runs?limit={limit}&after={_segment(after)}")

    def children(self, run_id: str, step: int = 0) -> wire.ChildResults:
        return self._request("GET", f"/runs/{_segment(run_id)}/children?step={step}")

    def child_result(self, run_id: str, child: int, step: int = 0) -> wire.Record:
        return self._request("GET", f"/runs/{_segment(run_id)}/children/{child}/result?step={step}")

    def operations(self, limit: int = 64, after: str = "") -> list[wire.Operation]:
        return self._request("GET", f"/operations?limit={limit}&after={_segment(after)}")

    def operation(self, operation_id: str) -> wire.Operation:
        return self._request("GET", "/operations/" + _segment(operation_id))

    def claim(self, operation_id: str, request: wire.Claim) -> wire.Execution:
        return self._request("POST", "/operations/" + _segment(operation_id) + "/claim", request)

    def receipt(self, operation_id: str, receipt: wire.Receipt, receipt_token: str) -> wire.Operation:
        return self._request("POST", "/operations/" + _segment(operation_id) + "/receipt", receipt, token=receipt_token)

    def report_usage(self, operation_id: str, request: wire.UsageSupplement, receipt_token: str | None = None) -> wire.Operation:
        return self._request("POST", "/operations/" + _segment(operation_id) + "/usage", request, token=receipt_token)

    def usage_evidence(self, operation_id: str, limit: int = 64, after: str = "") -> list[wire.UsageEvidence]:
        return self._request("GET", f"/operations/{_segment(operation_id)}/usage?limit={limit}&after={_segment(after)}")

    def retry(self, operation_id: str, request: wire.Retry) -> wire.CommandReceipt:
        return self._request("POST", "/operations/" + _segment(operation_id) + "/retry", request)

    def interactions(self, limit: int = 64, after: str = "") -> list[wire.Interaction]:
        return self._request("GET", f"/interactions?limit={limit}&after={_segment(after)}")

    def interact(self, request: wire.InteractionCreate) -> wire.Interaction:
        return self._request("POST", "/interactions", request)

    def answer(self, interaction_id: str, request: wire.InteractionAnswer) -> wire.Interaction:
        return self._request("POST", "/interactions/" + _segment(interaction_id), request)

    def connections(self, limit: int = 64, after: str = "") -> list[wire.Connection]:
        return self._request("GET", f"/connections?limit={limit}&after={_segment(after)}")

    def attach(self, template: str, *, request_id=None, resource_id=None) -> wire.Connection:
        request = {"request_id": request_id or str(uuid4()), "template": template}
        if resource_id is not None:
            request["resource_id"] = resource_id
        return self._request("POST", "/connections", request)

    def revoke_connection(self, connection_id: str) -> wire.Connection:
        return self._request("DELETE", "/connections/" + _segment(connection_id))

    def documents(self, scope: str, limit: int = 64, after: str = "") -> list[wire.Document]:
        if scope not in ("state", "memory"):
            raise ValueError("scope must be state or memory")
        return self._request("GET", f"/{scope}?limit={limit}&after={_segment(after)}")

    def put_document(self, scope: str, key: str, request: wire.DocumentUpdate) -> wire.Document:
        if scope not in ("state", "memory"):
            raise ValueError("scope must be state or memory")
        return self._request("PUT", f"/{scope}/{_segment(key)}", request)

    def delete_document(self, scope: str, key: str) -> wire.Document:
        if scope not in ("state", "memory"):
            raise ValueError("scope must be state or memory")
        return self._request("DELETE", f"/{scope}/{_segment(key)}")

    def files(self, limit: int = 64, after: str = "") -> list[wire.FileInfo]:
        return self._request("GET", f"/files?limit={limit}&after={_segment(after)}")

    def put_file(self, name: str, body: bytes, media_type="application/octet-stream") -> wire.FileInfo:
        return self._request("PUT", "/files/" + _segment(name), body, media_type=media_type)

    def file(self, name: str) -> bytes:
        return self._request("GET", "/files/" + _segment(name), binary=True)

    def delete_file(self, name: str) -> wire.FileInfo:
        return self._request("DELETE", "/files/" + _segment(name))

    def schedules(self, limit: int = 64, after: str = "") -> list[wire.Schedule]:
        return self._request("GET", f"/schedules?limit={limit}&after={_segment(after)}")

    def schedule(self, request: wire.ScheduleCreate) -> wire.Schedule:
        return self._request("POST", "/schedules", request)

    def cancel_schedule(self, schedule_id: str) -> wire.Schedule:
        return self._request("DELETE", "/schedules/" + _segment(schedule_id))

    def webhook(self, request: wire.Input) -> wire.CommandReceipt:
        return self._request("POST", "/webhooks", request)

    def delegate(self, request: wire.Delegation) -> wire.SessionView:
        return self._request("POST", "/delegations", request)

    def budget(self) -> wire.SessionBudget:
        return self._request("GET", "/budget")

    def handoff(self, request: wire.Handoff) -> wire.SessionView:
        return self._request("POST", "/handoff", request)

    def transfer_ownership(self, request: wire.TransferCreate) -> wire.Transfer:
        return self.client.request("POST", f"/operator/sessions/{_segment(self.id)}/transfers", request)

    def ownership_transfer(self, transfer_id: str) -> wire.Transfer:
        return self.client.request("GET", f"/operator/sessions/{_segment(self.id)}/transfers/{_segment(transfer_id)}")

    def cancel_transfer(self, transfer_id: str) -> wire.Transfer:
        return self.client.request("DELETE", f"/operator/sessions/{_segment(self.id)}/transfers/{_segment(transfer_id)}")

    def grant(self, request: wire.ParticipantGrant) -> wire.ParticipantToken:
        return self._request("POST", "/participants", request)

    def revoke_participant(self, participant_id: str) -> str:
        return self._request("DELETE", "/participants/" + _segment(participant_id))

    def export(self, after: str = "0") -> wire.ExportManifest:
        return self._request("GET", "/export?after=" + _segment(after))

    def delete(self) -> wire.SessionView:
        return self._request("DELETE")

    def _items(self, read, key):
        after = ""
        while True:
            page = read(after)
            if not page:
                return
            cursor = page[-1][key]
            if cursor == after:
                raise SessionError("corrupt", "Collection cursor did not advance")
            yield from page
            after = cursor

    def all_runs(self) -> Iterator[wire.Run]:
        return self._items(lambda after: self.runs(64, after), "id")

    def all_operations(self) -> Iterator[wire.Operation]:
        return self._items(lambda after: self.operations(64, after), "id")

    def all_interactions(self) -> Iterator[wire.Interaction]:
        return self._items(lambda after: self.interactions(64, after), "id")

    def all_connections(self) -> Iterator[wire.Connection]:
        return self._items(lambda after: self.connections(64, after), "id")

    def all_documents(self, scope: str) -> Iterator[wire.Document]:
        return self._items(lambda after: self.documents(scope, 64, after), "key")

    def all_files(self) -> Iterator[wire.FileInfo]:
        return self._items(lambda after: self.files(64, after), "name")

    def all_schedules(self) -> Iterator[wire.Schedule]:
        return self._items(lambda after: self.schedules(64, after), "id")

    def export_pages(self) -> Iterator[wire.ExportManifest]:
        after = "0"
        while True:
            page = self.export(after)
            yield page
            cursor = page["continuation"]
            if not cursor:
                return
            if cursor == after:
                raise SessionError("corrupt", "Export cursor did not advance")
            after = cursor

    def events(self, after: str = "0") -> Iterator[dict[str, Any]]:
        with self.client._open("GET", self.path + "/events",
                               headers={"Accept": "text/event-stream", "Last-Event-ID": after}) as response:
            event = cursor = ""
            data = []
            size = 0
            while True:
                line = response.readline(1048577)
                if not line:
                    return
                size += len(line)
                if size > 1048576:
                    raise SessionError("invalid", "SSE frame exceeds one MiB")
                text = line.decode("utf-8").rstrip("\r\n")
                if not text:
                    if event in ("record", "gap"):
                        if not cursor.isascii() or not cursor.isdecimal() or str(int(cursor)) != cursor:
                            raise SessionError("invalid", "Invalid durable cursor")
                        yield {"kind": event, "id": cursor, event: json.loads("\n".join(data))}
                    event = cursor = ""
                    data = []
                    size = 0
                elif text.startswith("event:"):
                    event = text[6:].lstrip()
                elif text.startswith("id:"):
                    cursor = text[3:].lstrip()
                elif text.startswith("data:"):
                    data.append(text[5:].lstrip())

    def work(self, connection: wire.Connection, handler: Callable[[wire.Execution], wire.Receipt], receipts, after="0"):
        for execution, receipt in receipts.pending():
            if execution["operation"]["session_id"] != self.id:
                continue
            receipt = receipt or {"request_id": "uncertain:" + execution["operation"]["id"], "outcome": "unknown",
                                  "error": "Application restarted after claim; handler is not automatically repeated"}
            self.receipt(execution["operation"]["id"], receipt, execution["receipt_token"])
            receipts.acknowledged(execution["operation"]["id"])

        def execute(operation):
            if operation["recipient"] != connection["id"] or operation["status"] != "prepared":
                return
            try:
                execution = self.claim(operation["id"], {"request_id": str(uuid4()),
                    "connection_id": connection["id"], "generation": connection["generation"]})
            except SessionError as error:
                if error.status in (403, 409):
                    return
                raise
            receipts.begin(execution)
            try:
                receipt = handler(execution)
            except Exception:
                receipt = {"request_id": str(uuid4()), "outcome": "unknown", "error": "Handler returned no authoritative outcome"}
            receipts.complete(execution, receipt)
            self.receipt(operation["id"], receipt, execution["receipt_token"])
            receipts.acknowledged(operation["id"])

        for operation in self.all_operations():
            execute(operation)
        for event in self.events(after):
            if event["kind"] == "record" and event["record"]["kind"] == "operation.prepared":
                execute(self.operation(event["record"]["data"]["id"]))
            elif event["kind"] == "record" and event["record"]["kind"] == "interaction.answered":
                for operation in self.all_operations():
                    execute(operation)
