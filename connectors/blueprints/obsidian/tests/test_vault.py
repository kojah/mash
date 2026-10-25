"""Vault capture and identity boundaries without a running Obsidian application."""

import os
import tempfile
import unittest
from pathlib import Path

from mash_obsidian.config import Local, Settings, load
from mash_obsidian.registry import Registry
from mash_obsidian.vault import scan


class VaultTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.vault = self.root / "vault"
        self.vault.mkdir()
        self.settings = Settings(
            vault=self.vault,
            vault_id="test",
            state_dir=self.root / "state",
            folders=(".",),
            deletion_grace_seconds=2,
            destinations=(Local(type="local", database=self.root / "silver.sqlite3"),),
        )

    def registry(self) -> Registry:
        return Registry(self.settings.state_dir, self.settings.vault_id)

    def test_metadata_is_extracted_without_rewriting_markdown(self) -> None:
        text = (
            "---\ntitle: Deployment\ncreated: 2026-10-01T12:00:00Z\n"
            "tags: [release, '#rust']\nextra: {done: true}\n---\n"
            "# Other heading\nUse #offline/search. `#not-a-tag`\n"
            "```\n#also-not-a-tag\n```\n![[diagram.png]]\n"
            "![outside](../../secret.txt)"
        )
        (self.vault / "Note.md").write_text(text)
        (self.vault / "diagram.png").write_bytes(b"opaque image")
        (self.vault / ".obsidian").mkdir()
        (self.vault / ".obsidian" / "hidden.md").write_text("not a note")
        with self.registry() as registry:
            result = scan(self.settings, registry)
        [note] = result.documents
        self.assertEqual(note.content, text)
        self.assertEqual(note.title, "Deployment")
        self.assertEqual(note.created_at, 1790856000000)
        metadata = note.metadata or {}
        self.assertEqual(metadata["tags"], ["offline/search", "release", "rust"])
        self.assertEqual(
            metadata["embeds"],
            [
                {"target": "diagram.png", "path": "diagram.png"},
                {"target": "../../secret.txt", "path": None},
            ],
        )
        self.assertEqual((self.vault / "Note.md").read_text(), text)
        self.assertEqual(result.issues, [])

    def test_rename_restart_atomic_save_and_copy_have_correct_identities(self) -> None:
        note = self.vault / "first.md"
        note.write_text("First body")
        with self.registry() as registry:
            original = scan(self.settings, registry).documents[0].id
        moved = self.vault / "renamed.md"
        note.rename(moved)
        with self.registry() as registry:
            self.assertEqual(scan(self.settings, registry).documents[0].id, original)
            temporary = self.vault / "replacement"
            temporary.write_text("Edited by atomic save")
            os.replace(temporary, moved)
            self.assertEqual(scan(self.settings, registry).documents[0].id, original)
            (self.vault / "copy.md").write_text(moved.read_text())
            notes = scan(self.settings, registry).documents
            self.assertEqual(len({n.id for n in notes}), 2)
            self.assertEqual(
                next(
                    n.id
                    for n in notes
                    if n.metadata and n.metadata["path"] == "renamed.md"
                ),
                original,
            )

    def test_temporary_absence_and_failed_scan_do_not_confirm_deletion(self) -> None:
        note = self.vault / "note.md"
        note.write_text("Initial")
        with self.registry() as registry:
            original = scan(self.settings, registry, now=100).documents[0].id
            note.unlink()
            self.assertEqual(scan(self.settings, registry, now=101).deleted, [])
            note.write_text("Restored after atomic replacement")
            self.assertEqual(
                scan(self.settings, registry, now=104).documents[0].id, original
            )
            note.unlink()
            self.assertEqual(scan(self.settings, registry, now=105).deleted, [])
            broken = self.vault / "broken.md"
            broken.write_bytes(b"\xff")
            result = scan(self.settings, registry, now=110)
            self.assertTrue(result.issues)
            self.assertEqual(result.deleted, [])
            broken.unlink()
            self.assertEqual(scan(self.settings, registry, now=111).deleted, [original])

    def test_scope_narrowing_never_deletes_excluded_notes(self) -> None:
        for name in ["a", "b"]:
            (self.vault / name).mkdir()
            (self.vault / name / "note.md").write_text(name)
        with self.registry() as registry:
            first = scan(self.settings, registry, now=100)
            ids = {n.metadata["path"]: n.id for n in first.documents if n.metadata}
            narrow = self.settings.model_copy(update={"folders": ("a",)})
            (self.vault / "b" / "note.md").unlink()
            self.assertEqual(scan(narrow, registry, now=101).deleted, [])
            self.assertEqual(scan(narrow, registry, now=120).deleted, [])
            # A copy of excluded content must not steal its identity.
            (self.vault / "b" / "note.md").write_text("b")
            (self.vault / "a" / "copy.md").write_text("b")
            notes = scan(narrow, registry, now=121).documents
            self.assertNotIn(ids["b/note.md"], {n.id for n in notes})

    def test_unknown_creation_is_not_modification_time(self) -> None:
        (self.vault / "unknown.md").write_text("No creation evidence")
        (self.vault / "old.md").write_text(
            "---\ncreated: 2020-01-01T00:00:00Z\n---\nOld"
        )
        (self.vault / "new.md").write_text(
            "---\ncreated: 2026-10-14T00:00:00Z\n---\nNew"
        )
        bounded = self.settings.model_copy(update={"history_days": 7})
        with self.registry() as registry:
            result = scan(bounded, registry, now=1791936000)
        self.assertEqual([n.title for n in result.documents], ["new"])
        self.assertEqual(result.excluded, 2)

    def test_symlinks_and_nonregular_files_are_not_read(self) -> None:
        outside = self.root / "secret.md"
        outside.write_text("Must never leave this file")
        (self.vault / "linked.md").symlink_to(outside)
        os.mkfifo(self.vault / "pipe.md")
        with self.registry() as registry:
            result = scan(self.settings, registry)
        self.assertEqual(result.documents, [])
        self.assertEqual(len(result.issues), 2)

    def test_malformed_nonfinite_and_recursive_frontmatter_are_isolated(self) -> None:
        for name, text in {
            "broken": "---\ntitle: [\n---\nBody",
            "nan": "---\nvalue: .nan\n---\nBody",
            "cycle": "---\nvalue: &x [*x]\n---\nBody",
        }.items():
            (self.vault / f"{name}.md").write_text(text)
        (self.vault / "good.md").write_text("Good")
        with self.registry() as registry:
            result = scan(self.settings, registry)
        self.assertEqual([n.title for n in result.documents], ["good"])
        self.assertEqual(len(result.issues), 3)

    def test_registry_rejects_other_vault_and_duplicate_process(self) -> None:
        with self.registry():
            with self.assertRaisesRegex(ValueError, "active"):
                self.registry()
        with self.assertRaisesRegex(ValueError, "another vault"):
            Registry(self.settings.state_dir, "different")

    def test_configuration_rejects_unknown_keys_traversal_and_vault_state(self) -> None:
        config = self.root / "connector.toml"
        base = (
            'vault="vault"\nvault_id="test"\nstate_dir="state"\nfolders=["."]\n'
            '[[destinations]]\ntype="local"\ndatabase="index.sqlite3"\n'
        )
        config.write_text(base)
        self.assertEqual(load(config).vault, self.vault)
        for invalid in [
            base.replace('folders=["."]', 'folders=["../escape"]'),
            base.replace('state_dir="state"', 'state_dir="vault/state"'),
            base.replace('database="index.sqlite3"', 'database="vault/index.sqlite3"'),
            base + "unexpected=true\n",
            base.replace('folders=["."]', "folders=[]"),
        ]:
            config.write_text(invalid)
            with self.assertRaises(ValueError):
                load(config)
