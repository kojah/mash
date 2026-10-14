"""Scope selection, history, comment mapping and incremental polling, offline."""

import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from fake_google import FakeGoogle, folder, item
from mash_sdk import (
    Changes,
    Connector,
    Destination,
    DocumentSnapshot,
    Mode,
    Receipt,
    Role,
    Snapshot,
)
from mash_sdk.config import Local

from mash_google_docs.config import Settings
from mash_google_docs.connector import Result, synchronize

NOW = datetime(2026, 10, 14, tzinfo=UTC)


class Recorder(Connector):
    """Records what would be delivered; the SDK's own delivery is tested upstream."""

    def __init__(self, state: Path) -> None:
        super().__init__(
            source_id="google-docs:test",
            modes=[Mode.snapshot],
            destinations=[Destination.local(database=state / "unused.sqlite3")],
            state_path=state / "unused-state.sqlite3",
        )
        self.snapshots: list[Snapshot | DocumentSnapshot] = []
        self.sent: list[Changes] = []

    async def sync(self, *snapshots: Snapshot | DocumentSnapshot) -> int:
        self.snapshots.extend(snapshots)
        return len(snapshots)

    async def send(self, changes: Changes) -> tuple[Receipt, ...]:
        self.sent.append(changes)
        return ()

    def documents(self) -> dict[str, str | None]:
        return {
            document.id: document.content
            for snapshot in self.snapshots
            if isinstance(snapshot, DocumentSnapshot)
            for document in snapshot.documents
        }


class ConnectorTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.state = Path(temporary.name)
        self.google = FakeGoogle()

    def settings(self, **selection: object) -> Settings:
        return Settings.model_validate(
            {
                "account_id": "test",
                "client_secret": self.state / "client.json",
                "state_dir": self.state,
                "destinations": [
                    Local(type="local", database=self.state / "mash.sqlite3")
                ],
                **selection,
            }
        )

    async def poll(self, settings: Settings) -> tuple[Result, Recorder]:
        recorder = Recorder(self.state)
        result = await synchronize(settings, self.google.drive(), recorder, NOW)
        return result, recorder

    async def test_empty_allowlists_select_nothing(self) -> None:
        self.google.add(item("a", parent="root"))
        result, recorder = await self.poll(self.settings())
        self.assertEqual(result.selected_documents, 0)
        self.assertEqual(recorder.snapshots, [])
        self.assertEqual(self.google.calls, [])

    async def test_folders_recurse_drives_and_documents_select_docs_only(self) -> None:
        self.google.add(
            folder("sub", parent="top"),
            folder("deeper", parent="sub"),
            item("a", parent="top"),
            item("b", parent="deeper"),
            item("sheet", parent="top", mime="application/vnd.google-apps.spreadsheet"),
            item("elsewhere", parent="other"),
            item("team", drive="drive1"),
            item("explicit", parent="other"),
            item("picked-sheet", mime="application/vnd.google-apps.spreadsheet"),
        )
        result, recorder = await self.poll(
            self.settings(
                folders=["top"],
                shared_drives=["drive1"],
                documents=["explicit", "picked-sheet"],
            )
        )
        self.assertEqual(
            set(recorder.documents()),
            {"doc:a", "doc:b", "doc:team", "doc:explicit"},
        )
        self.assertEqual(result.issues, [])

    async def test_history_window_uses_creation_time(self) -> None:
        self.google.add(
            item("old", parent="top", created="2026-01-01T00:00:00Z"),
            item("new", parent="top", created="2026-10-10T00:00:00Z"),
        )
        result, recorder = await self.poll(
            self.settings(folders=["top"], history_days=30)
        )
        self.assertEqual(set(recorder.documents()), {"doc:new"})
        self.assertEqual(result.time_excluded_documents, 1)

    async def test_document_and_comment_mapping(self) -> None:
        self.google.add(
            item("a", parent="top"),
            text="# Plan\nShip the zephyr.",
            comments=[
                {
                    "id": "c1",
                    "author": {"displayName": "Lin", "permissionId": "7"},
                    "content": "Is this final?",
                    "createdTime": "2026-10-03T00:00:00Z",
                    "resolved": True,
                    "quotedFileContent": {"value": "Ship the zephyr."},
                    "replies": [
                        {
                            "id": "r1",
                            "author": {"displayName": "Ada"},
                            "content": "Yes.",
                        },
                        {"id": "r2", "action": "resolve"},
                        {"id": "r3", "content": "gone", "deleted": True},
                    ],
                },
                {"id": "c2", "content": "deleted", "deleted": True},
            ],
        )
        await self.poll(self.settings(folders=["top"]))
        recorder = (await self.poll(self.settings(folders=["top"])))[1]
        self.assertEqual(recorder.snapshots, [], "an unchanged Doc is not re-exported")
        recorder = Recorder(self.state)
        for path in self.state.glob("documents-*.json"):
            path.unlink()
        await synchronize(
            self.settings(folders=["top"]), self.google.drive(), recorder, NOW
        )
        documents, comments = recorder.snapshots
        assert isinstance(documents, DocumentSnapshot)
        assert isinstance(comments, Snapshot)
        self.assertFalse(documents.complete)
        doc = documents.documents[0]
        self.assertEqual(doc.role, Role.document)
        self.assertEqual(doc.kind, "google-docs:document")
        self.assertEqual(doc.title, "Title a")
        self.assertEqual(doc.url, "https://docs.google.com/document/d/a/edit")
        self.assertEqual(doc.content, "# Plan\nShip the zephyr.")
        self.assertEqual(doc.created_at, 1790812800000)
        assert doc.metadata is not None
        self.assertEqual(doc.metadata["parents"], ["top"])
        self.assertEqual(doc.metadata["last_modified_by"], {"name": "Grace"})
        self.assertEqual(comments.container.id, "comments:a")
        self.assertEqual(comments.container.metadata, {"document": "doc:a"})
        self.assertTrue(comments.complete)
        comment, reply, resolve = comments.objects
        self.assertEqual(comment.kind, "google-docs:comment")
        self.assertEqual(comment.content, "> Ship the zephyr.\n\nIs this final?")
        self.assertEqual(
            comment.metadata, {"resolved": True, "quoted": "Ship the zephyr."}
        )
        assert comment.author is not None
        self.assertEqual((comment.author.id, comment.author.name), ("google:7", "Lin"))
        self.assertEqual(reply.kind, "google-docs:reply")
        self.assertEqual(reply.reply_to, comment.id)
        self.assertEqual(reply.parent, "comments:a")
        assert reply.author is not None
        self.assertTrue(reply.author.id.startswith("name:"))
        self.assertEqual(resolve.content, "(resolve)")
        self.assertEqual(resolve.metadata, {"action": "resolve"})

    async def test_only_changed_docs_are_exported_and_trash_deletes(self) -> None:
        self.google.add(
            item("a", parent="top"),
            item("b", parent="top"),
            comments=[{"id": "c", "content": "note"}],
        )
        settings = self.settings(folders=["top"])
        first, _ = await self.poll(settings)
        self.assertEqual(first.exported_documents, 2)
        self.google.files["a"]["version"] = "2"
        self.google.texts["a"] = "Edited"
        self.google.calls.clear()
        second, recorder = await self.poll(settings)
        self.assertEqual(second.exported_documents, 1)
        self.assertEqual(recorder.documents(), {"doc:a": "Edited"})
        self.assertNotIn("files/b/export", self.google.calls)
        # Unshared or moved out of scope: never a deletion.
        del self.google.files["a"]
        third, recorder = await self.poll(settings)
        self.assertEqual((third.deleted_documents, recorder.sent), (0, []))
        # Trashed in Drive: an explicit deletion of the Doc and its comments.
        self.google.files["b"]["trashed"] = True
        fourth, recorder = await self.poll(settings)
        self.assertEqual(fourth.deleted_documents, 1)
        self.assertEqual(recorder.sent, [Changes.delete("doc:b", "comments:b")])
        self.assertEqual((await self.poll(settings))[1].sent, [])

    async def test_failures_are_issues_not_deletions(self) -> None:
        self.google.add(item("a", parent="top"))
        await self.poll(self.settings(folders=["top"]))
        result, recorder = await self.poll(
            self.settings(folders=["top", "missing"], documents=["gone"])
        )
        self.assertEqual(
            sorted(result.issues),
            ["document gone: HTTP 404", "folder missing: HTTP 404"],
        )
        self.assertEqual(recorder.sent, [])


if __name__ == "__main__":
    unittest.main()
