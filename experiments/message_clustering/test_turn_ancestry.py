"""Full canonical ancestry and omitted-response boundary contracts."""

import gzip
import hashlib
import tempfile
import unittest
from pathlib import Path

import orjson

from audit_turns import checked, load_capture
from corpus import Corpus, Provider, Role, SelectedThread, Split
from test_turns import thread
from turn_ancestry import (
    Canonical,
    CanonicalRole,
    Capture,
    CapturedThread,
    bridge,
    native_check,
    stable_id,
)
from turns import AssistantActivity, Boundary, extract


def row(
    identity: str,
    ordinal: int,
    role: CanonicalRole,
    parent: int | None,
    projection: str = "",
) -> Canonical:
    return Canonical(
        id=identity,
        ordinal=ordinal,
        role=role,
        context_parent=parent,
        projection=projection,
        content_sha256="not-used",
        created_at=None,
    )


class FullAncestryContracts(unittest.TestCase):
    def test_capture_rejects_source_tampering_and_wrong_or_duplicate_cohort(
        self,
    ) -> None:
        source = thread((Role.USER, Role.ASSISTANT))
        canonical = tuple(
            row(m.id, m.ordinal, CanonicalRole(m.role), m.context_parent, m.projection)
            for m in source.messages
        )
        fixed, resolutions, activities = bridge(source, canonical)
        raw = b'{"type":"session_meta"}\n'
        item = CapturedThread(
            index=0,
            thread=fixed,
            canonical=canonical,
            resolutions=resolutions,
            activities=activities,
            original_turns=extract(source),
            bridged_turns=extract(fixed),
            turns=extract(fixed, activities),
            source_sha256=hashlib.sha256(raw).hexdigest(),
            native_matched=0,
            native_parent_checked=0,
            native_parent_mismatches=0,
            native_compactions=0,
        )
        with self.assertRaisesRegex(ValueError, "provider source changed"):
            checked(item, raw + b" ")
        with self.assertRaisesRegex(ValueError, "incomplete native Claude"):
            checked(
                item.model_copy(
                    update={
                        "thread": fixed.model_copy(update={"provider": Provider.CLAUDE})
                    }
                ),
                raw,
            )
        corpus = Corpus(
            gold_manifest="fixture",
            source_threads=2,
            ancestry_files_checked=0,
            selected=(
                SelectedThread(split=Split.DEVELOPMENT, component="a", thread=source),
                SelectedThread(split=Split.HELD_OUT, component="b", thread=source),
            ),
        )
        capture = Capture(original_corpus_sha256="fixture", threads=(item,))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capture.json"
            (path.parent / "source-0.jsonl.gz").write_bytes(gzip.compress(raw))
            path.write_bytes(orjson.dumps(capture.model_dump(mode="json")))
            self.assertEqual(load_capture(path, corpus, "fixture")[0].thread, fixed)
            with self.assertRaisesRegex(ValueError, "differs"):
                load_capture(path, corpus, "other")
            for bad in ((item, item), (item.model_copy(update={"index": 1}),)):
                path.write_bytes(
                    orjson.dumps(
                        capture.model_copy(update={"threads": bad}).model_dump(
                            mode="json"
                        )
                    )
                )
                with self.assertRaisesRegex(ValueError, "exactly once"):
                    load_capture(path, corpus, "fixture")

    def test_native_presence_is_role_sensitive_and_conflicting_uuid_fails(self) -> None:
        source = thread((Role.USER,))
        canonical = (
            row("0", 0, CanonicalRole.USER, None, source.messages[0].projection),
        )
        raw = orjson.dumps(
            {
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": source.messages[0].projection,
                },
            }
        )
        self.assertEqual(native_check(source, canonical, raw)[0], 0)
        raw = b"\n".join(
            orjson.dumps(r)
            for r in (
                {"type": "user", "uuid": "same", "parentUuid": None},
                {"type": "user", "uuid": "same", "parentUuid": "different"},
            )
        )
        with self.assertRaisesRegex(ValueError, "conflicting native parents"):
            native_check(
                source.model_copy(update={"provider": Provider.CLAUDE}), canonical, raw
            )

    def test_bridge_tool_and_reasoning_preserves_real_user_reply(self) -> None:
        source = thread((Role.USER, Role.ASSISTANT, Role.USER))
        source = source.model_copy(
            update={
                "messages": (
                    source.messages[0].model_copy(update={"context_parent": -1}),
                    source.messages[1].model_copy(
                        update={"ordinal": 4, "context_parent": 3}
                    ),
                    source.messages[2].model_copy(
                        update={"ordinal": 5, "context_parent": 4}
                    ),
                )
            }
        )
        canonical = (
            row("0", 0, CanonicalRole.USER, -1, source.messages[0].projection),
            row("hidden-response", 1, CanonicalRole.ASSISTANT, 0),
            row("result", 2, CanonicalRole.USER, 1),
            row("thinking", 3, CanonicalRole.ASSISTANT, 2),
            row("1", 4, CanonicalRole.ASSISTANT, 3, source.messages[1].projection),
            row("2", 5, CanonicalRole.USER, 4, source.messages[2].projection),
        )
        fixed, resolutions, activities = bridge(source, canonical)
        self.assertEqual(fixed.messages[1].context_parent, 0)
        self.assertEqual(resolutions[1].skipped, (3, 2, 1))
        turns = extract(fixed, activities)
        self.assertEqual(
            [(t.prompts, t.answers) for t in turns], [((0,), (1,)), ((2,), ())]
        )
        self.assertEqual(turns[1].boundary, Boundary.USER_REPLY)

    def test_hidden_assistant_activity_splits_prompts_but_tool_result_does_not(
        self,
    ) -> None:
        source = thread((Role.USER, Role.USER))
        source = source.model_copy(
            update={
                "messages": (
                    source.messages[0],
                    source.messages[1].model_copy(update={"ordinal": 2}),
                )
            }
        )
        for role, expected in (
            (CanonicalRole.ASSISTANT, 2),
            (CanonicalRole.USER, 1),
            (CanonicalRole.TOOL, 1),
            (CanonicalRole.DEVELOPER, 1),
        ):
            canonical = (
                row("0", 0, CanonicalRole.USER, None, source.messages[0].projection),
                row("hidden", 1, role, None),
                row("1", 2, CanonicalRole.USER, None, source.messages[1].projection),
            )
            fixed, _, activities = bridge(source, canonical)
            self.assertEqual(len(extract(fixed, activities)), expected)

    def test_hidden_activity_from_another_branch_cannot_split_prompt_bundle(
        self,
    ) -> None:
        source = thread((Role.USER, Role.USER))
        source = source.model_copy(
            update={
                "messages": (
                    source.messages[0],
                    source.messages[1].model_copy(
                        update={"ordinal": 2, "context_parent": 0}
                    ),
                )
            }
        )
        self.assertEqual(
            len(extract(source, (AssistantActivity(ordinal=1, parent=-1),))), 1
        )

    def test_branch_continuation_keeps_branch_reason(self) -> None:
        source = thread((Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT))
        source = source.model_copy(
            update={
                "messages": source.messages[:3]
                + (source.messages[3].model_copy(update={"context_parent": 0}),)
            }
        )
        turns = extract(source)
        self.assertEqual(turns[-1].boundary, Boundary.BRANCH)
        self.assertFalse(turns[-1].prompts)

    def test_missing_future_cycle_and_changed_content_fail_closed(self) -> None:
        source = thread((Role.USER, Role.ASSISTANT))
        for parent in (7, 1):
            canonical = (
                row("0", 0, CanonicalRole.USER, -1, source.messages[0].projection),
                row(
                    "1",
                    1,
                    CanonicalRole.ASSISTANT,
                    parent,
                    source.messages[1].projection,
                ),
            )
            with self.assertRaises(ValueError):
                bridge(source, canonical)
        canonical = (
            row("0", 0, CanonicalRole.USER, None, "changed"),
            row("1", 1, CanonicalRole.ASSISTANT, None, source.messages[1].projection),
        )
        with self.assertRaises(ValueError):
            bridge(source, canonical)
        with self.assertRaises(ValueError):
            extract(source, (AssistantActivity(ordinal=1, parent=2),))

    def test_unembedded_visible_user_cannot_be_silently_skipped(self) -> None:
        source = thread((Role.USER,))
        canonical = (
            row("0", 0, CanonicalRole.USER, None, source.messages[0].projection),
            row("unembedded-user", 1, CanonicalRole.USER, None, "actual new request"),
        )
        with self.assertRaisesRegex(ValueError, "omits visible"):
            bridge(source, canonical)

    def test_native_uuid_check_resolves_nonmessage_events_and_detects_mismatch(
        self,
    ) -> None:
        source = thread((Role.USER, Role.ASSISTANT)).model_copy(
            update={"provider": Provider.CLAUDE}
        )
        ids = (
            stable_id("claude-message", ("thread", "prompt")),
            stable_id("claude-message", ("thread", "answer")),
        )
        source = source.model_copy(
            update={
                "messages": tuple(
                    m.model_copy(update={"id": ids[i]})
                    for i, m in enumerate(source.messages)
                )
            }
        )
        canonical = (
            row(ids[0], 0, CanonicalRole.USER, -1, source.messages[0].projection),
            row(ids[1], 1, CanonicalRole.ASSISTANT, 0, source.messages[1].projection),
        )
        raw = b"\n".join(
            orjson.dumps(r)
            for r in (
                {"type": "user", "uuid": "prompt", "parentUuid": None},
                {"type": "attachment", "uuid": "metadata", "parentUuid": "prompt"},
                {"type": "assistant", "uuid": "answer", "parentUuid": "metadata"},
            )
        )
        self.assertEqual(native_check(source, canonical, raw), (2, 2, 0, 0))
        bad = (canonical[0], canonical[1].model_copy(update={"context_parent": -1}))
        self.assertEqual(native_check(source, bad, raw)[2], 1)


if __name__ == "__main__":
    unittest.main()
