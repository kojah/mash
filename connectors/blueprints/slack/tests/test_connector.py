"""Scope, history window, threads, edits, mapping, cursors and retries, offline."""

import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from fake_slack import TEAM, FakeSlack, limited, post, ts
from mash_sdk import Connector, Destination, DocumentSnapshot, Mode, Object, Snapshot
from mash_sdk.config import Local

from mash_slack.config import Settings
from mash_slack.connector import Result, synchronize

NOW = datetime(2026, 10, 14, tzinfo=UTC)
DAY = 86400
TODAY = int(NOW.timestamp())


def days_ago(days: float, micros: int = 0) -> str:
    return ts(int(TODAY - days * DAY), micros)


class Recorder(Connector):
    """Records what would be delivered; the SDK's own delivery is tested upstream."""

    def __init__(self, state: Path) -> None:
        super().__init__(
            source_id=f"slack:{TEAM}:U0ALICE",
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
        self.slack = FakeSlack()
        self.slack.__enter__()
        self.addCleanup(self.slack.__exit__)
        self.slack.channel("C01", "general")
        self.slack.channel("C02", "random")
        self.slack.user("U1", "Ada")
        self.slack.user("U2", "Grace")

    def settings(self, **values: object) -> Settings:
        return Settings.model_validate(
            {
                "team_id": TEAM,
                "principal": "U0ALICE",
                "state_dir": self.state,
                "channels": ["C01"],
                "destinations": [
                    Local(type="local", database=self.state / "mash.sqlite3")
                ],
                **values,
            }
        )

    def test_source_id_is_scoped_by_the_principal(self) -> None:
        alice = self.settings(principal="U0ALICE")
        bob = self.settings(principal="U0BOB")
        assert alice.source_id == f"slack:{TEAM}:U0ALICE"
        # Two credentials in one workspace are two sources, never one.
        assert alice.source_id != bob.source_id

    async def poll(self, settings: Settings) -> tuple[Result, Recorder]:
        recorder = Recorder(self.state)
        result = await synchronize(settings, self.slack.client(), recorder, NOW)
        return result, recorder

    def called(self, method: str) -> list[dict[str, str]]:
        return [query for name, query in self.slack.calls if name == method]

    async def test_messages_threads_and_subtypes_map(self) -> None:
        parent, reply = days_ago(2), days_ago(1)
        self.slack.messages["C01"] = [
            post(
                parent,
                "Deploy <@U2>? See <https://x.example|the plan> &amp; <#C02|random>",
            ),
            post(reply, "Done.", user="U2", thread_ts=parent),
            post(days_ago(1, 5), "joined", subtype="channel_join"),
            post(
                days_ago(1, 7),
                "",
                subtype="bot_message",
                user=None,
                bot_id="B1",
                bot_profile={"name": "CI"},
                files=[
                    {
                        "id": "F1",
                        "title": "log.txt",
                        "mimetype": "text/plain",
                        "size": 3,
                        "permalink": "https://acme.slack.com/files/F1",
                    }
                ],
            ),
            post(
                days_ago(0.5), "edited text", edited={"user": "U1", "ts": days_ago(0.1)}
            ),
        ]
        self.slack.messages["C02"] = [post(days_ago(1), "not selected")]
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.issues, [])
        self.assertEqual((result.synced_channels, result.fetched_threads), (1, 1))
        objects = recorder.objects()
        self.assertEqual(
            set(objects),
            {
                "C01",
                f"C01:{parent}",
                f"C01:{reply}",
                f"C01:{days_ago(1, 7)}",
                f"C01:{days_ago(0.5)}",
            },
        )
        self.assertFalse(any(q.get("channel") == "C02" for _, q in self.slack.calls))
        channel = objects["C01"]
        self.assertEqual((channel.kind, channel.title), ("slack:channel", "#general"))
        self.assertEqual(channel.url, "https://acme.slack.com/archives/C01")
        first = objects[f"C01:{parent}"]
        self.assertEqual(
            first.content,
            "Deploy @Grace? See the plan (https://x.example) & #random",
        )
        assert first.author is not None
        self.assertEqual((first.author.id, first.author.name), ("slack:U1", "Ada"))
        self.assertEqual(first.created_at, int(parent.split(".")[0]) * 1000)
        self.assertEqual(
            first.url, f"https://acme.slack.com/archives/C01/p{parent.replace('.', '')}"
        )
        second = objects[f"C01:{reply}"]
        self.assertEqual((second.parent, second.reply_to), ("C01", f"C01:{parent}"))
        assert second.url is not None
        self.assertIn(f"?thread_ts={parent}&cid=C01", second.url)
        bot = objects[f"C01:{days_ago(1, 7)}"]
        assert bot.author is not None
        self.assertEqual((bot.author.kind, bot.author.name), ("bot", "CI"))
        self.assertEqual(bot.content, "(file: log.txt)")
        self.assertEqual([a.name for a in bot.attachments], ["log.txt"])
        edited = objects[f"C01:{days_ago(0.5)}"]
        self.assertEqual(edited.updated_at, int(days_ago(0.1).split(".")[0]) * 1000)
        self.assertFalse(recorder.snapshots[0].complete)
        # Each user is looked up once per poll.
        self.assertEqual(
            sorted(q["user"] for q in self.called("users.info")), ["U1", "U2"]
        )

    async def test_window_excludes_old_messages_and_empty_allowlist_reads_nothing(
        self,
    ) -> None:
        self.slack.messages["C01"] = [
            post(days_ago(40), "ancient"),
            post(days_ago(3), "recent"),
        ]
        result, recorder = await self.poll(self.settings(channels=[]))
        self.assertEqual(result.synced_channels, 0)
        self.assertEqual([m for m, _ in self.slack.calls], ["auth.test"])
        result, recorder = await self.poll(self.settings(history_days=30))
        self.assertEqual(set(recorder.objects()), {"C01", f"C01:{days_ago(3)}"})
        self.assertEqual(
            recorder.snapshots[0].created_since, datetime(2026, 9, 14, tzinfo=UTC)
        )
        # The window starts the listing; thread_days does not reach further back.
        self.assertEqual(
            self.called("conversations.history")[0]["oldest"],
            f"{TODAY - 30 * DAY}.000000",
        )

    async def test_new_reply_on_an_older_thread_and_edits_are_found(self) -> None:
        parent = days_ago(5)
        self.slack.messages["C01"] = [
            post(parent, "Question?"),
            post(days_ago(4), "First answer.", user="U2", thread_ts=parent),
            post(days_ago(2), "Unrelated."),
        ]
        await self.poll(self.settings())
        self.slack.calls.clear()
        # Nothing changed: one history listing over the lookback, no replies.
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.fetched_threads, 0)
        self.assertEqual(self.called("conversations.replies"), [])
        self.assertEqual(
            self.called("conversations.history")[0]["oldest"],
            f"{TODAY - 7 * DAY}.000000",
        )
        # A late reply to the 5-day-old thread and an edit to an older message.
        late = days_ago(0.2)
        self.slack.messages["C01"] += [
            post(late, "Late answer.", user="U2", thread_ts=parent)
        ]
        self.slack.messages["C01"][2]["text"] = "Unrelated, edited."
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.fetched_threads, 1)
        objects = recorder.objects()
        self.assertEqual(objects[f"C01:{late}"].reply_to, f"C01:{parent}")
        self.assertEqual(objects[f"C01:{days_ago(2)}"].content, "Unrelated, edited.")

    async def test_history_pages_and_cursor_survive_downtime(self) -> None:
        self.slack.messages["C01"] = [post(days_ago(30 - n), f"m{n}") for n in range(5)]
        result, recorder = await self.poll(self.settings())
        self.assertEqual(len(recorder.snapshots[0].objects), 5)
        self.assertEqual(len(self.called("conversations.history")), 3)  # Pages of 2.
        self.slack.calls.clear()
        # The cursor (26 days ago) is older than the lookback, so it wins.
        await self.poll(self.settings())
        self.assertEqual(
            self.called("conversations.history")[0]["oldest"], days_ago(26)
        )

    async def test_rate_limits_retry_and_channel_failures_are_isolated(self) -> None:
        self.slack.messages["C01"] = [post(days_ago(1), "hello")]
        self.slack.failures["conversations.history"] = [limited()]
        result, recorder = await self.poll(self.settings(channels=["C09", "C01"]))
        self.assertEqual(result.issues, ["C09: channel_not_found"])
        self.assertEqual(result.synced_channels, 1)
        self.assertEqual(len(self.called("conversations.history")), 2)


if __name__ == "__main__":
    unittest.main()
