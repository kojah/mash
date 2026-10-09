"""Shared HTTP delivery against an in-process server with Mash's ingress contract.

The Rust tests in app/shared/server/tests/connectors.rs own the server side of
this contract; this server reproduces its headers, ordering and replay rules
so the client's recovery and error handling can be exercised without Qdrant.
"""

import os
import tempfile
import threading
import time
import unittest
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import orjson

from mash_sdk import Changes, Connector, DeliveryError, Destination, Document
from mash_sdk._state import StateStore

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


def note(content: str) -> Changes:
    return Changes.upsert(Document(id="note", content=content))


class SharedDeliveryTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state_path = Path(temporary.name) / "sdk.sqlite"
        self.server_state = Behaviour()
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler(self.server_state))
        server.daemon_threads = True
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        self.url = f"http://127.0.0.1:{server.server_address[1]}/"
        os.environ[TOKEN_ENV] = TOKEN
        self.addCleanup(os.environ.pop, TOKEN_ENV, None)

    def destination(self, *, timeout: float = 5) -> Destination:
        return Destination.shared(
            api_url=self.url,
            group_id="group",
            node_id="laptop",
            token_env=TOKEN_ENV,
            timeout=timeout,
        )

    def connector(self, destination: Destination | None = None) -> Connector:
        return Connector(
            source_id="external",
            destination=destination or self.destination(),
            state_path=self.state_path,
        )

    def pending(self, destination: Destination) -> bool:
        with StateStore(self.state_path) as store:
            saved = store.pending(
                source_id="external", destination_id=destination._identity()
            )
        return saved is not None

    async def test_sends_authenticated_json_and_orders_batches(self) -> None:
        async with self.connector() as connector:
            first = await connector.send(note("one"))
            second = await connector.send(note("two"))
        self.assertEqual((first.source_revision, second.source_revision), (1, 2))
        self.assertFalse(second.gold_ready)
        path, headers = self.server_state.requests[0]
        self.assertEqual(path, "/v1/connector-batches")
        self.assertEqual(headers["content-type"], "application/json")
        self.assertEqual(headers["x-mash-node"], "laptop")
        self.assertEqual(len(self.server_state.commits), 2)

    async def test_lost_response_after_commit_replays_the_original_receipt(
        self,
    ) -> None:
        self.server_state.drop_after_commit = True
        async with self.connector() as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(note("one"))
            self.assertTrue(raised.exception.retryable)
        self.assertTrue(self.pending(self.destination()))
        # Opening resumes the saved batch; the server replays its receipt.
        async with self.connector():
            pass
        self.assertEqual(len(self.server_state.commits), 1)
        self.assertEqual(len(self.server_state.requests), 2)
        self.assertFalse(self.pending(self.destination()))

    async def test_statuses_classify_retry_without_echoing_body_or_token(
        self,
    ) -> None:
        for status, retryable in ((409, False), (401, False), (503, True)):
            self.server_state.status = status
            # After the first failure, opening retries the saved batch itself.
            with self.assertRaises(DeliveryError) as raised:
                async with self.connector() as connector:
                    await connector.send(note("one"))
            message = str(raised.exception)
            self.assertIn(str(status), message)
            self.assertNotIn("PRIVATE-CANARY", message)
            self.assertNotIn(TOKEN, message)
            self.assertEqual(raised.exception.retryable, retryable)
            self.assertTrue(self.pending(self.destination()))
        self.server_state.status = None
        async with self.connector():
            pass
        self.assertEqual(len(self.server_state.commits), 1)
        self.assertFalse(self.pending(self.destination()))

    async def test_redirects_are_not_followed_with_credentials(self) -> None:
        self.server_state.redirect = True
        async with self.connector() as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(note("one"))
        self.assertFalse(raised.exception.retryable)
        self.assertEqual(
            [path for path, _ in self.server_state.requests],
            ["/v1/connector-batches"],
        )

    async def test_missing_token_fails_before_any_request(self) -> None:
        del os.environ[TOKEN_ENV]
        async with self.connector() as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(note("one"))
        self.assertIn(TOKEN_ENV, str(raised.exception))
        self.assertFalse(raised.exception.retryable)
        self.assertEqual(self.server_state.requests, [])

    async def test_stalled_server_times_out_as_retryable(self) -> None:
        self.server_state.stall_seconds = 1
        async with self.connector(self.destination(timeout=0.2)) as connector:
            with self.assertRaises(DeliveryError) as raised:
                await connector.send(note("one"))
        self.assertTrue(raised.exception.retryable)


class SharedDestinationTests(unittest.TestCase):
    def shared(self, **overrides: str) -> Destination:
        arguments = {
            "api_url": "https://mash.example/",
            "group_id": "group",
            "node_id": "laptop",
            "token_env": "MASH_TOKEN",
        } | overrides
        return Destination.shared(
            api_url=arguments["api_url"],
            group_id=arguments["group_id"],
            node_id=arguments["node_id"],
            token_env=arguments["token_env"],
        )

    def test_identity_names_the_receiving_node_not_the_token(self) -> None:
        base = self.shared()._identity()
        self.assertTrue(base.startswith("shared:"))
        self.assertEqual(base, self.shared(api_url="https://mash.example")._identity())
        self.assertEqual(base, self.shared(token_env="OTHER")._identity())
        self.assertNotEqual(base, self.shared(node_id="desktop")._identity())
        self.assertNotEqual(base, self.shared(group_id="other")._identity())

    def test_rejects_what_mash_configuration_rejects(self) -> None:
        for overrides in (
            {"api_url": "ftp://mash.example"},
            {"api_url": "https://user:secret@mash.example"},
            {"api_url": "https://mash.example/?token=x"},
            {"api_url": "https://mash.example/#x"},
            {"node_id": "bad name"},
            {"group_id": ""},
            {"token_env": "1URL"},
        ):
            with self.subTest(overrides=overrides), self.assertRaises(ValueError):
                self.shared(**overrides)
        with self.assertRaises(ValueError):
            Destination.shared(
                api_url="https://mash.example",
                group_id="group",
                node_id="laptop",
                token_env="MASH_TOKEN",
                timeout=float("nan"),
            )


if __name__ == "__main__":
    unittest.main()
