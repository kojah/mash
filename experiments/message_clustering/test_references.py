import unittest
from pathlib import Path

import numpy as np
from pydantic import ValidationError

from corpus import Corpus, Message, Provider, SelectedThread, Split
from preparation import prepare
from references import References, ThreadReference
from test_corpus import fixture_thread


class ReferenceContracts(unittest.TestCase):
    def test_context_obeys_roots_parents_and_never_uses_reference_labels(self) -> None:
        thread = fixture_thread("context", Provider.CODEX, Path("unused"), 4)
        messages = tuple(
            Message(
                id=str(i),
                ordinal=i,
                role="user",
                projection=f"real prose with five words {i}",
                context_parent=parent,
                dimensions=2,
                norm=3,
                vector_hex="0300" if i == 0 else "0003",
            )
            for i, parent in enumerate((-1, -1, 0, -1))
        )
        thread = thread.model_copy(update={"messages": messages})
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="fixture"
        )
        prepared = prepare(thread, reference, context=True)
        np.testing.assert_allclose(
            prepared.vectors[2], np.array([0.3, 0.7]) / np.linalg.norm([0.3, 0.7])
        )
        np.testing.assert_allclose(prepared.vectors[3], [0, 1])
        changed_labels = reference.model_copy(update={"boilerplate": (0,)})
        unchanged = prepare(thread, changed_labels, context=True)
        np.testing.assert_array_equal(prepared.vectors, unchanged.vectors)
        np.testing.assert_array_equal(prepared.substantive, unchanged.substantive)

    def test_exact_projection_dedup_preserves_every_source_identity(self) -> None:
        thread = fixture_thread("dedup", Provider.CODEX, Path("unused"), 3)
        thread = thread.model_copy(
            update={
                "messages": (
                    thread.messages[0],
                    thread.messages[1],
                    thread.messages[2].model_copy(
                        update={"projection": thread.messages[0].projection}
                    ),
                )
            }
        )
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="fixture"
        )
        prepared = prepare(thread, reference)
        self.assertEqual(prepared.source_to_unit, (0, 1, 0))
        self.assertEqual(prepared.unit_to_source, (0, 1))

    def test_references_reject_duplicate_pairs_and_unknown_case_tags(self) -> None:
        thread = fixture_thread("ref", Provider.CODEX, Path("unused"), 3)
        corpus = Corpus(
            gold_manifest="fixture",
            source_threads=1,
            ancestry_files_checked=1,
            selected=(
                SelectedThread(split=Split.DEVELOPMENT, component="one", thread=thread),
            ),
        )
        for reference in (
            ThreadReference(
                index=0, related=((0, 1),), unrelated=((1, 0),), notes="bad"
            ),
            ThreadReference(
                index=0,
                related=((0, 1),),
                unrelated=(),
                returning=((1, 2),),
                notes="bad",
            ),
        ):
            with self.assertRaises(ValueError):
                References(
                    schema_version=1,
                    corpus_sha256="fixture",
                    provenance="fixture",
                    threads=(reference,),
                ).validate_corpus(corpus)

    def test_deleted_parent_abstains_and_serialized_restart_preserves_ids(self) -> None:
        thread = fixture_thread("updates", Provider.CODEX, Path("unused"), 3)
        messages = (
            thread.messages[0],
            thread.messages[1],
            thread.messages[2].model_copy(update={"context_parent": 1}),
        )
        thread = thread.model_copy(update={"messages": messages})
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="fixture"
        )
        deleted = thread.model_copy(update={"messages": (messages[0], messages[2])})
        restart = type(thread).model_validate_json(deleted.model_dump_json())
        self.assertEqual(
            [m.id for m in restart.messages], [messages[0].id, messages[2].id]
        )
        prepared = prepare(restart, reference, context=True)
        self.assertEqual(prepared.contextualized, 0)
        np.testing.assert_allclose(
            prepared.vectors[1], prepare(deleted, reference).vectors[1]
        )
        for parent in (-2, 2, 3):
            with self.assertRaises(ValidationError):
                Message.model_validate(
                    messages[2].model_dump() | {"context_parent": parent}
                )

    def test_edit_updates_child_context_and_append_preserves_existing_ids(self) -> None:
        thread = fixture_thread("edit", Provider.CODEX, Path("unused"), 3)
        thread = thread.model_copy(
            update={
                "messages": (
                    thread.messages[0],
                    thread.messages[1],
                    thread.messages[2].model_copy(update={"context_parent": 1}),
                )
            }
        )
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="fixture"
        )
        before = prepare(thread, reference, context=True)
        edited = thread.messages[1].model_copy(
            update={
                "projection": "edited substantive message with different vector",
                "vector_hex": "fd04",
                "norm": 5.0,
            }
        )
        changed = thread.model_copy(
            update={"messages": (thread.messages[0], edited, thread.messages[2])}
        )
        after = prepare(changed, reference, context=True)
        self.assertFalse(np.allclose(before.vectors[2], after.vectors[2]))
        self.assertEqual(
            [m.id for m in thread.messages], [m.id for m in changed.messages]
        )
        appended = changed.messages[-1].model_copy(
            update={
                "id": "new-source",
                "ordinal": 3,
                "context_parent": 2,
                "projection": "new substantive append with its own identity",
            }
        )
        appended_thread = changed.model_copy(
            update={"messages": changed.messages + (appended,)}
        )
        restarted = type(thread).model_validate_json(appended_thread.model_dump_json())
        self.assertEqual(restarted.messages[:-1], changed.messages)
        self.assertEqual(
            prepare(restarted, reference, True).source_to_unit, (0, 1, 2, 3)
        )


if __name__ == "__main__":
    unittest.main()
