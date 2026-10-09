"""An in-process server reproducing Mash's connector ingress contract.

The Rust tests in app/shared/server/tests/connectors.rs own the server side;
this keeps its headers, ordering, replay and record semantics so client
recovery and runner behaviour can be tested without Qdrant.
"""

import threading
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import orjson

TOKEN = "test-token-first-32-bytes-long-xx"
TOKEN_ENV = "MASH_SDK_TEST_SHARED_TOKEN"


@dataclass
class Behaviour:
    """Next-request faults and what the server accepted."""

    status: int | None = None
    drop_after_commit: bool = False
    stall_seconds: float = 0
    redirect: bool = False
    revision: int = 0
    silver: int = 0
    receipts: dict[str, bytes] = field(default_factory=dict)
    commits: list[bytes] = field(default_factory=list)
    # Statuses returned (without committing) by the next requests, in order.
    failures: list[int] = field(default_factory=list)
    chats: dict[str, dict[str, object]] = field(default_factory=dict)
    messages: dict[str, dict[str, object]] = field(default_factory=dict)
    documents: dict[str, dict[str, object]] = field(default_factory=dict)

    def apply(self, changes: list[dict[str, object]]) -> None:
        for change in changes:
            id = str(change["id"])
            match change["op"]:
                case "upsert-chat":
                    self.chats[id] = change
                case "upsert-message":
                    self.messages[id] = change
                case "upsert-document":
                    self.documents[id] = change
                case "delete-chat":
                    self.chats.pop(id, None)
                    for message in [
                        key
                        for key, value in self.messages.items()
                        if value["chat_id"] == id
                    ]:
                        del self.messages[message]
                case "delete-message":
                    self.messages.pop(id, None)
                case "delete-document":
                    self.documents.pop(id, None)

    requests: list[tuple[str, dict[str, str]]] = field(default_factory=list)


def handler(state: Behaviour) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: object) -> None:
            pass

        def reply(self, status: int, body: bytes) -> None:
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self) -> None:
            body = self.rfile.read(int(self.headers["content-length"]))
            state.requests.append(
                (self.path, {k.lower(): v for k, v in self.headers.items()})
            )
            if state.stall_seconds:
                time.sleep(state.stall_seconds)
                return
            if state.redirect:
                self.send_response(307)
                self.send_header("location", "/elsewhere")
                self.end_headers()
                return
            if state.failures:
                self.reply(state.failures.pop(0), b"{}")
                return
            if state.status is not None:
                self.reply(state.status, b'{"error":"PRIVATE-CANARY"}')
                return
            if self.headers["authorization"] != f"Bearer {TOKEN}":
                self.reply(401, b"{}")
                return
            if (self.headers["x-mash-group"], self.headers["x-mash-node"]) != (
                "group",
                "laptop",
            ):
                self.reply(403, b"{}")
                return
            if self.path != "/v1/connector-batches" or not self.headers[
                "content-type"
            ].startswith("application/json"):
                self.reply(400, b"{}")
                return
            batch = orjson.loads(body)
            receipt = state.receipts.get(batch["batch_id"])
            if receipt is None:
                if batch["expected_revision"] != state.revision:
                    self.reply(409, b"{}")
                    return
                state.revision += 1
                state.silver += 1
                state.commits.append(body)
                state.apply(batch["changes"])
                receipt = orjson.dumps(
                    {
                        "schema_version": 1,
                        "source_id": batch["source_id"],
                        "batch_id": batch["batch_id"],
                        "source_revision": state.revision,
                        "silver_revision": state.silver,
                        "gold_ready": False,
                    }
                )
                state.receipts[batch["batch_id"]] = receipt
            if state.drop_after_commit:
                state.drop_after_commit = False
                self.close_connection = True
                return
            self.reply(200, receipt)

    return Handler


def serve(state: Behaviour) -> tuple[str, Callable[[], None]]:
    """Start a server; returns its URL and a function that stops it."""
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler(state))
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()

    def stop() -> None:
        server.shutdown()
        server.server_close()

    return f"http://127.0.0.1:{server.server_address[1]}/", stop
