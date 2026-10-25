"""Scope, history window, mapping, cursors and rate limits against a fake Discord."""

import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import patch

from fake_discord import (
    HELPER,
    TOKEN,
    FakeDiscord,
    attachment,
    channel,
    message,
    snowflake,
)
from mash_sdk import Connector, Destination, DocumentSnapshot, Mode, Object, Snapshot
from mash_sdk.config import Local

from mash_discord.client import connect
from mash_discord.config import Settings
from mash_discord.connector import Result, synchronize

NOW = datetime(2026, 10, 14, tzinfo=UTC)
GENERAL = "200000000000000000"
THREAD = "300000000000000000"
OTHER = "400000000000000000"


def at(day: int, sequence: int = 0) -> str:
    return snowflake(f"2026-10-{day:02d}T00:00:00+00:00", sequence)


class Recorder(Connector):
    """Records what would be delivered; the SDK's own delivery is tested upstream."""

    def __init__(self, state: Path) -> None:
        super().__init__(
            source_id="discord:test",
            modes=[Mode.snapshot],
            destinations=[Destination.local(database=state / "unused.sqlite3")],
            state_path=state / "unused-state.sqlite3",
        )
        self.snapshots: list[Snapshot] = []

    async def sync(self, *snapshots: Snapshot | DocumentSnapshot) -> int:
        for snapshot in snapshots:
            assert isinstance(snapshot, Snapshot)
            self.snapshots.append(snapshot)
        return len(snapshots)

    def objects(self) -> dict[str, Object]:
        return {
            item.id: item
            for snapshot in self.snapshots
            for item in (snapshot.container, *snapshot.objects)
        }


class ConnectorTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state = Path(temporary.name)
        self.discord = FakeDiscord()

    def settings(self, **values: object) -> Settings:
        return Settings.model_validate(
            {
                "account_id": "test",
                "state_dir": self.state,
                "channels": [GENERAL],
                "destinations": [
                    Local(type="local", database=self.state / "mash.sqlite3")
                ],
                **values,
            }
        )

    async def poll(self, settings: Settings) -> tuple[Result, Recorder]:
        recorder = Recorder(self.state)
        with self.discord.serve() as url:
            async with connect(TOKEN, url) as rest:
                result = await synchronize(settings, rest, recorder, NOW)
        return result, recorder

    async def test_messages_replies_bots_and_threads_map(self) -> None:
        first, reply, cross = at(1), at(1, 1), at(1, 2)
        self.discord.add(
            channel(GENERAL, "general"),
            message(GENERAL, first, "Ship it?", files=[attachment("7", "plan.txt")]),
            message(GENERAL, at(1, 3), "", type=7),  # A member joined.
            message(GENERAL, at(1, 4), "", type=18),  # Thread created.
            message(GENERAL, reply, "Yes.", author=HELPER, type=19, reply_to=first),
            message(
                GENERAL, cross, "See it.", type=19, reply_to="5", reply_channel=OTHER
            ),
            message(GENERAL, at(1, 5), "Typo.", edited="2026-10-02T00:00:00+00:00"),
        )
        self.discord.add(
            channel(THREAD, "plan", parent=GENERAL),
            message(THREAD, at(2), "In the thread."),
        )
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.issues, [])
        self.assertEqual((result.fetched_messages, result.synced_messages), (6, 4))
        objects = recorder.objects()
        # Selecting the channel does not select its thread.
        self.assertNotIn(f"channel:{THREAD}", objects)
        self.assertFalse(any(THREAD in call for call in self.discord.calls))
        container = objects[f"channel:{GENERAL}"]
        self.assertEqual(container.kind, "discord:channel")
        self.assertEqual(container.title, "general")
        self.assertFalse(recorder.snapshots[0].complete)
        root = objects[f"message:{first}"]
        self.assertEqual(
            root.url,
            f"https://discord.com/channels/100000000000000000/{GENERAL}/{first}",
        )
        self.assertEqual(
            root.created_at, int(datetime(2026, 10, 1, tzinfo=UTC).timestamp() * 1000)
        )
        self.assertEqual([file.name for file in root.attachments], ["plan.txt"])
        assert root.author is not None
        self.assertEqual(
            (root.author.id, root.author.kind, root.author.name),
            ("discord:1", "human", "Ada Lovelace"),
        )
        answer = objects[f"message:{reply}"]
        self.assertEqual(answer.reply_to, f"message:{first}")
        assert answer.author is not None
        self.assertEqual((answer.author.kind, answer.author.name), ("bot", "helper"))
        elsewhere = objects[f"message:{cross}"]
        self.assertIsNone(elsewhere.reply_to)
        self.assertEqual(
            elsewhere.metadata,
            {"native_reference": {"channel_id": OTHER, "message_id": "5"}},
        )
        self.assertIsNotNone(objects[f"message:{at(1, 5)}"].updated_at)

        # A thread is its own chat once explicitly selected; its parent is metadata.
        result, recorder = await self.poll(self.settings(channels=[THREAD]))
        thread = recorder.objects()[f"channel:{THREAD}"]
        self.assertEqual(thread.kind, "discord:thread")
        self.assertEqual(
            thread.metadata,
            {"guild_id": "100000000000000000", "parent_channel_id": GENERAL},
        )

    async def test_history_window_uses_snowflake_creation_time(self) -> None:
        self.discord.add(
            channel(GENERAL, "general"),
            message(GENERAL, snowflake("2026-09-01T00:00:00+00:00"), "Too old."),
            message(GENERAL, at(1), "Recent."),
        )
        result, recorder = await self.poll(self.settings(history_days=30))
        self.assertEqual(
            set(recorder.objects()), {f"channel:{GENERAL}", f"message:{at(1)}"}
        )
        floor = snowflake("2026-09-14T00:00:00+00:00")
        self.assertIn(f"after={floor}&", "\n".join(self.discord.calls))
        self.assertEqual(result.fetched_messages, 1)

    async def test_cursor_pages_and_resumes_after_the_last_message(self) -> None:
        ids = [at(1, sequence) for sequence in range(150)]
        self.discord.add(
            channel(GENERAL, "general"),
            *(message(GENERAL, id, f"Message {n}") for n, id in enumerate(ids)),
        )
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.synced_messages, 150)
        self.assertEqual(
            [len(snapshot.objects) for snapshot in recorder.snapshots], [100, 50]
        )
        self.discord.calls.clear()
        result, recorder = await self.poll(self.settings())
        self.assertEqual((result.fetched_messages, recorder.snapshots), (0, []))
        self.assertIn(f"after={ids[-1]}&", self.discord.calls[-1])
        self.discord.messages[GENERAL].append(message(GENERAL, at(3), "New."))
        result, recorder = await self.poll(self.settings())
        self.assertEqual(
            set(recorder.objects()), {f"channel:{GENERAL}", f"message:{at(3)}"}
        )

    async def test_rate_limits_are_retried_and_failures_isolated(self) -> None:
        path = f"/api/v10/channels/{GENERAL}/messages"
        limited = {
            "message": "You are being rate limited.",
            "retry_after": 0.01,
            "global": False,
        }
        self.discord.failures[path] = [(429, limited)]
        self.discord.failures[f"/api/v10/channels/{OTHER}"] = [
            (403, {"message": "Missing Access", "code": 50001})
        ]
        self.discord.add(channel(GENERAL, "general"), message(GENERAL, at(1), "Hello."))
        result, _ = await self.poll(self.settings(channels=[OTHER, GENERAL]))
        self.assertEqual(result.synced_messages, 1)
        self.assertEqual(result.issues, [f"{OTHER}: HTTP 403"])
        self.assertEqual(
            sum(call.startswith(f"{path}?after=0&") for call in self.discord.calls), 2
        )
        self.assertEqual(self.discord.authorizations, {f"Bot {TOKEN}"})

    async def test_endless_rate_limits_fail_only_that_channel(self) -> None:
        limited = {"message": "Slow down.", "retry_after": 0.01, "global": False}
        path = f"/api/v10/channels/{OTHER}/messages"
        self.discord.failures[path] = [(429, limited)] * 10000
        self.discord.add(channel(OTHER, "busy"), message(OTHER, at(1), "Stuck."))
        self.discord.add(channel(GENERAL, "general"), message(GENERAL, at(1), "Hi."))
        with (
            patch("mash_discord.connector.REQUEST_SECONDS", 0.3),
            self.assertLogs("hikari", "WARNING"),
        ):
            result, recorder = await self.poll(self.settings(channels=[OTHER, GENERAL]))
        self.assertEqual(result.issues, [f"{OTHER}: rate limited or timed out"])
        self.assertEqual(
            set(recorder.objects()), {f"channel:{GENERAL}", f"message:{at(1)}"}
        )

    async def test_disabled_message_content_intent_reads_nothing(self) -> None:
        self.discord.flags = 0
        self.discord.add(channel(GENERAL, "general"), message(GENERAL, at(1), ""))
        result, recorder = await self.poll(self.settings())
        self.assertEqual(
            result.issues, ["application: the Message Content intent is disabled"]
        )
        self.assertEqual((recorder.snapshots, len(self.discord.calls)), ([], 1))


if __name__ == "__main__":
    unittest.main()
