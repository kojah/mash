"""Scope, history window, mapping, incremental cursors and retries, offline."""

import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import patch

import httpx
from fake_github import (
    FakeGitHub,
    comment,
    issue,
    review,
    review_comment,
)
from mash_sdk import Connector, Destination, DocumentSnapshot, Mode, Object, Snapshot

from mash_github.config import Local, Settings
from mash_github.connector import Result, synchronize

NOW = datetime(2026, 10, 14, tzinfo=UTC)


class Recorder(Connector):
    """Records what would be delivered; the SDK's own delivery is tested upstream."""

    def __init__(self, state: Path) -> None:
        super().__init__(
            source_id="github:test",
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
        self.github = FakeGitHub()

    def settings(self, **values: object) -> Settings:
        return Settings.model_validate(
            {
                "account_id": "test",
                "state_dir": self.state,
                "repositories": ["acme/app"],
                "destinations": [
                    Local(type="local", database=self.state / "mash.sqlite3")
                ],
                **values,
            }
        )

    async def poll(self, settings: Settings) -> tuple[Result, Recorder]:
        recorder = Recorder(self.state)
        result = await synchronize(settings, self.github.github(), recorder, NOW)
        return result, recorder

    async def test_issues_prs_comments_reviews_and_review_comments_map(self) -> None:
        self.github.add(
            "acme/app",
            issue("acme/app", 1),
            issue("acme/app", 2, pull=True, merged=True),
        )
        self.github.add("acme/other", issue("acme/other", 3))
        self.github.comments["acme/app#1"] = [comment(11, "Seen on Linux.")]
        self.github.comments["acme/app#2"] = [comment(12, "Thanks!")]
        self.github.reviews["acme/app#2"] = [
            review(21, ""),
            review(22, "draft", state="PENDING"),
        ]
        self.github.review_comments["acme/app#2"] = [
            review_comment(31, "Rename this."),
            review_comment(32, "Done.", reply_to=31),
        ]
        result, recorder = await self.poll(self.settings())
        self.assertEqual((result.listed_items, result.synced_items), (2, 2))
        self.assertEqual(result.issues, [])
        objects = recorder.objects()
        self.assertEqual(
            {id: item.kind for id, item in objects.items()},
            {
                "issue:1001": "github:issue",
                "issue:1001:body": "github:description",
                "comment:11": "github:comment",
                "issue:1002": "github:pull-request",
                "issue:1002:body": "github:description",
                "comment:12": "github:comment",
                "review:21": "github:review",
                "review-comment:31": "github:review-comment",
                "review-comment:32": "github:review-comment",
            },
        )
        self.assertFalse(any("acme/other" in call for call in self.github.calls))
        pull = objects["issue:1002"]
        assert pull.metadata is not None
        self.assertEqual(pull.metadata["merged"], True)
        self.assertEqual(pull.metadata["labels"], ["bug"])
        self.assertEqual(pull.url, "https://github.com/acme/app/pull/2")
        self.assertEqual(objects["review:21"].content, "(approved)")
        reply = objects["review-comment:32"]
        self.assertEqual(reply.reply_to, "review-comment:31")
        self.assertEqual(reply.parent, "issue:1002")
        assert reply.metadata is not None
        self.assertEqual(reply.metadata["path"], "src/main.rs")
        author = objects["comment:11"].author
        assert author is not None
        self.assertEqual((author.id, author.kind), ("github:2", "bot"))

        # Selecting only issues (a separate cursor) skips PRs but moves past them.
        result, recorder = await self.poll(self.settings(item_kinds=["issue"]))
        self.assertEqual((result.listed_items, result.synced_items), (2, 1))
        self.assertNotIn("issue:1002", recorder.objects())

    async def test_old_issue_with_recent_comment_stays_discoverable(self) -> None:
        old = "2025-01-01T00:00:00Z"
        self.github.add(
            "acme/app",
            issue("acme/app", 1, created=old, updated="2026-10-10T00:00:00Z"),
            issue("acme/app", 2, created=old, updated="2026-10-10T00:00:00Z"),
        )
        self.github.comments["acme/app#1"] = [
            comment(11, "Ancient.", created=old),
            comment(12, "Still broken.", created="2026-10-09T00:00:00Z"),
        ]
        result, recorder = await self.poll(self.settings(history_days=30))
        self.assertEqual(result.time_excluded_items, 1)
        self.assertEqual(set(recorder.objects()), {"issue:1001", "comment:12"})
        snapshot = recorder.snapshots[0]
        self.assertTrue(snapshot.complete)
        self.assertEqual(snapshot.created_since, datetime(2026, 9, 14, tzinfo=UTC))
        self.assertIn("since=2026-09-14", self.github.calls[0])

    async def test_cursor_skips_unchanged_items_and_pages_by_update_time(self) -> None:
        same = "2026-10-03T00:00:00Z"
        self.github.add(
            "acme/app",
            *(issue("acme/app", n, updated=same) for n in range(1, 4)),
            issue("acme/app", 4, updated="2026-10-04T00:00:00Z"),
            issue("acme/app", 5, updated="2026-10-05T00:00:00Z"),
        )
        with patch("mash_github.connector.PAGE", 2):
            result, _ = await self.poll(self.settings())
            self.assertEqual(result.synced_items, 5)
            self.github.calls.clear()
            result, recorder = await self.poll(self.settings())
            self.assertEqual(result.listed_items, 0)
            self.assertEqual(recorder.snapshots, [])
            self.assertEqual(len(self.github.calls), 1)
            self.assertIn("since=2026-10-05", self.github.calls[0])
            # A same-second update to an unseen issue is not lost at the boundary.
            self.github.add(
                "acme/app", issue("acme/app", 6, updated="2026-10-05T00:00:00Z")
            )
            self.github.issues["acme/app"][1]["updated_at"] = "2026-10-06T00:00:00Z"
            result, recorder = await self.poll(self.settings())
        self.assertEqual(
            [snapshot.container.id for snapshot in recorder.snapshots],
            ["issue:1006", "issue:1002"],
        )

    async def test_rate_limits_are_retried_and_failures_reported(self) -> None:
        limited = httpx.Response(
            403,
            json={"message": "You have exceeded a secondary rate limit."},
            headers={"retry-after": "1"},
        )
        self.github.failures["/repos/acme/app/issues"] = [limited]
        self.github.failures["/repos/acme/gone/issues"] = [httpx.Response(404, json={})]
        self.github.add("acme/app", issue("acme/app", 1))
        result, _ = await self.poll(
            self.settings(repositories=["acme/gone", "acme/app"])
        )
        self.assertEqual(result.synced_items, 1)
        self.assertEqual(result.issues, ["acme/gone: HTTP 404"])
        self.assertEqual(
            sum("/repos/acme/app/issues?" in call for call in self.github.calls), 2
        )


if __name__ == "__main__":
    unittest.main()
