import json
from pathlib import Path
import unittest

from native_audit import Match, Provider, decode, parent_evidence, plain_content, sha, stable_id, verify


def record(uuid, text, parent=None):
    return decode(json.dumps({"type": "user", "uuid": uuid, "parentUuid": parent,
                              "message": {"role": "user", "content": text}}).encode(),
                  1, Provider.CLAUDE, Path("/fixture"), "thread")


class NativeTests(unittest.TestCase):
    def test_same_text_different_identity_does_not_match(self):
        source = record("a", "same text")
        self.assertEqual(verify((source,), stable_id("claude-message", ("thread", "b")),
                                "user", "same text")["status"], Match.MISSING)

    def test_changed_content_role_and_duplicate_conflict_are_not_verified(self):
        source = record("a", "original")
        self.assertEqual(verify((source,), source.stored_id, "user", "changed")["status"], Match.CONTENT)
        self.assertEqual(verify((source,), source.stored_id, "assistant", "original")["status"], Match.CONTENT)
        changed = record("a", "changed")
        self.assertEqual(verify((source, changed), source.stored_id, "user", "original")["status"], Match.CONFLICT)
        self.assertEqual(verify((source, source), source.stored_id, "user", "original")["status"], Match.VERIFIED)

    def test_canonical_subset_keeps_empty_separators_and_rejects_tool_bodies(self):
        self.assertEqual(plain_content([{"type": "input_text", "text": "a"},
                                        {"type": "input_text", "text": ""},
                                        {"type": "input_image", "image_url": "opaque"},
                                        {"type": "input_text", "text": "b"}]), "a\n\nb")
        self.assertIsNone(plain_content([{"type": "tool_use", "input": {"text": "private"}}]))

    def test_parent_metadata_path_matches_snapshot_and_missing_or_cycle_is_explicit(self):
        first = record("first", "first")
        bridge = decode(json.dumps({"type": "attachment", "uuid": "bridge", "parentUuid": "first"}).encode(),
                        2, Provider.CLAUDE, Path("/fixture"), "thread")
        anchor = record("anchor", "yes", "bridge")
        ordinals = {first.stored_id: 0, anchor.stored_id: 1}
        self.assertEqual(parent_evidence((first, bridge, anchor), anchor, "thread", ordinals, 0), "verified-parent")
        self.assertEqual(parent_evidence((first, anchor), anchor, "thread", ordinals, 0), "missing-native-parent-record")
        self.assertEqual(parent_evidence((first, bridge, anchor), anchor, "thread", ordinals, 9), "parent-mismatch")
        cycle = record("anchor", "yes", "anchor")
        self.assertEqual(parent_evidence((cycle,), cycle, "thread", {}, 0), "native-parent-cycle")

    def test_codex_requires_native_id_and_never_fabricates_fallback(self):
        raw = {"type": "response_item", "payload": {"type": "message", "role": "user", "content": "yes"}}
        parsed = decode(json.dumps(raw).encode(), 1, Provider.CODEX, Path("/fixture"), "thread")
        self.assertIsNone(parsed.stored_id)
        raw["payload"]["id"] = "native"
        parsed = decode(json.dumps(raw).encode(), 1, Provider.CODEX, Path("/fixture"), "thread")
        self.assertEqual(parsed.stored_id, stable_id("message", ("codex", "/fixture", "native")))
        self.assertEqual(parsed.content_sha256, sha("yes"))

    def test_completed_attachment_uses_explicit_turn_item_identity_and_known_text(self):
        raw = {"type": "event_msg", "payload": {"type": "item_completed", "turn_id": "turn",
               "item": {"type": "UserMessage", "id": "item", "content": [
                   {"type": "text", "text": "attachment placeholder"},
                   {"type": "local_image", "path": "private-path"}]}}}
        parsed = decode(json.dumps(raw).encode(), 1, Provider.CODEX, Path("/fixture"), "thread")
        self.assertEqual(parsed.stored_id, stable_id("message", ("codex", "/fixture", "completed:turn:item")))
        self.assertEqual(parsed.content_sha256, sha("attachment placeholder"))
        self.assertNotIn("private-path", str(parsed))
