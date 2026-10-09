"""Shared HTTP delivery against an in-process server with Mash's ingress contract."""

import os
import tempfile
import unittest
from pathlib import Path

from mash_sdk import Changes, Connector, DeliveryError, Destination, Document
from mash_sdk._state import StateStore
from tests.fake_mash import TOKEN, TOKEN_ENV, Behaviour, serve


def note(content: str) -> Changes:
    return Changes.upsert(Document(id="note", content=content))


class SharedDeliveryTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state_path = Path(temporary.name) / "sdk.sqlite"
        self.server_state = Behaviour()
        self.url, stop = serve(self.server_state)
        self.addCleanup(stop)
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
