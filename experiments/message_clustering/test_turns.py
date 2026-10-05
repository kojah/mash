"""Synthetic turn boundaries, source mappings, retention and publication contracts."""

import tempfile
import unittest
from pathlib import Path

import numpy as np
from tokenizers import Tokenizer, models, pre_tokenizers, processors

from clustering import Configuration, Method
from corpus import Message, Provider, Role, Thread
from local_encoder import Encoder, EncoderManifest
from references import ThreadReference
from turn_artifact import Artifact, Store
from turn_evaluation import Inputs, fit
from turns import Boundary, Recipe, Unit, extract, units, windows


def thread(roles: tuple[Role, ...], texts: tuple[str, ...] | None = None) -> Thread:
    return Thread(
        id="thread",
        provider=Provider.CODEX,
        session=None,
        source_path=Path("/not-read"),
        title=None,
        canonical_messages=len(roles),
        searchable_messages=len(roles),
        generation="message-generation",
        messages=tuple(
            Message(
                id=str(i),
                ordinal=i,
                role=role,
                projection=texts[i]
                if texts
                else "useful source text with enough words",
                context_parent=None,
                dimensions=2,
                norm=1,
                vector_hex="0100",
            )
            for i, role in enumerate(roles)
        ),
    )


def tokenizer() -> Tokenizer:
    result = Tokenizer(
        models.WordLevel({"[UNK]": 0, "[CLS]": 1, "[SEP]": 2}, unk_token="[UNK]")
    )
    result.pre_tokenizer = pre_tokenizers.Whitespace()
    result.post_processor = processors.TemplateProcessing(
        single="[CLS] $A [SEP]", special_tokens=[("[CLS]", 1), ("[SEP]", 2)]
    )
    return result


class FakeEncoder(Encoder):
    def __init__(self) -> None:
        self.manifest = EncoderManifest(
            model_sha256="model",
            tokenizer_sha256="tokens",
            runtime="test",
            tokenizer_runtime="test",
        )
        self.calls = 0
        self.fail = False

    def encode(self, texts: tuple[str, ...]):
        self.calls += len(texts)
        if self.fail:
            raise ValueError("simulated inference failure")
        return np.tile(np.array([[1.0, 0.0]]), (len(texts), 1))


class TurnContracts(unittest.TestCase):
    def test_multiple_prompts_segments_and_user_interruptions(self) -> None:
        source = thread(
            (
                Role.USER,
                Role.USER,
                Role.ASSISTANT,
                Role.ASSISTANT,
                Role.USER,
                Role.ASSISTANT,
            )
        )
        turns = extract(source)
        self.assertEqual(
            [(t.prompts, t.answers) for t in turns], [((0, 1), (2, 3)), ((4,), (5,))]
        )
        self.assertTrue(turns[0].closed)
        self.assertFalse(turns[1].closed)
        self.assertEqual(turns[1].boundary, Boundary.USER_REPLY)

    def test_native_branches_orphans_and_skipped_instruction(self) -> None:
        source = thread(
            (Role.ASSISTANT, Role.USER, Role.ASSISTANT, Role.USER, Role.USER)
        )
        changed = list(source.messages)
        changed[3] = changed[3].model_copy(update={"context_parent": -1})
        changed[4] = changed[4].model_copy(
            update={"projection": "# AGENTS.md injected"}
        )
        turns = extract(source.model_copy(update={"messages": tuple(changed)}))
        self.assertEqual(turns[0].boundary, Boundary.ORPHAN)
        self.assertEqual(turns[-1].boundary, Boundary.BRANCH)
        self.assertNotIn(4, tuple(i for t in turns for i in t.sources))

    def test_edit_changes_revision_but_not_identity_and_delete_removes_source(
        self,
    ) -> None:
        source = thread((Role.USER, Role.ASSISTANT))
        old = extract(source)[0]
        edited = source.model_copy(
            update={
                "messages": (
                    source.messages[0],
                    source.messages[1].model_copy(
                        update={"projection": "edited answer"}
                    ),
                )
            }
        )
        new = extract(edited)[0]
        self.assertEqual(old.id, new.id)
        self.assertNotEqual(old.revision, new.revision)
        reduced = extract(source.model_copy(update={"messages": source.messages[:1]}))[
            0
        ]
        self.assertEqual(reduced.sources, (0,))

    def test_unicode_spans_full_passage_coverage_and_budget(self) -> None:
        source = thread(
            (Role.USER, Role.ASSISTANT),
            ("café naïve 日本語", " ".join(["résumé"] * 1200)),
        )
        turn = extract(source)[0]
        tok = tokenizer()
        passages = units(source, turn, Recipe.PASSAGES, tok)
        self.assertTrue(all(u.tokens <= 512 for u in passages))
        covered = set()
        for unit in passages:
            for span in unit.spans:
                self.assertLessEqual(
                    span.end, len(source.messages[span.source].projection)
                )
                if span.source == 1:
                    covered.update(range(span.start, span.end))
        # Whitespace isn't a token; every character of every word is covered.
        self.assertTrue(
            all(
                i in covered
                for i, char in enumerate(source.messages[1].projection)
                if not char.isspace()
            )
        )
        joint = units(source, turn, Recipe.JOINT, tok)
        self.assertEqual(len(joint), 1)
        self.assertGreater(joint[0].discarded_tokens, 0)
        with self.assertRaises(ValueError):
            windows(tok, "text", (), 32, 32)

    def test_same_turn_passages_cannot_create_retained_cluster(self) -> None:
        source = thread((Role.USER, Role.ASSISTANT))
        turns = extract(source)
        prepared = units(source, turns[0], Recipe.PASSAGES, tokenizer())
        reference = ThreadReference(
            index=0, related=((0, 1),), unrelated=(), notes="synthetic"
        )
        outcome = fit(
            Inputs(
                source,
                turns,
                prepared,
                np.tile([[1.0, 0.0]], (len(prepared), 1)),
                reference,
            ),
            Configuration(Method.AVERAGE_LINK, 0.35),
        )
        self.assertEqual(outcome.groups, 0)
        overlay = fit(
            Inputs(
                source,
                turns,
                prepared,
                np.tile([[1.0, 0.0]], (len(prepared), 1)),
                reference,
            ),
            Configuration(Method.AVERAGE_LINK, 0.35),
            bundle_overlay=True,
        )
        self.assertEqual(overlay.cross_turn, outcome.cross_turn)
        self.assertEqual(overlay.all_pairs.tp if overlay.all_pairs else 0, 1)

    def test_passage_memberships_map_to_exact_sources_across_turns(self) -> None:
        source = thread((Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT))
        turns = extract(source)
        prepared = tuple(
            Unit(
                id=str(i),
                turn_id=turns[i // 2].id,
                text="source",
                spans=(),
                represented_sources=(i,),
                tokens=1,
            )
            for i in range(4)
        )
        reference = ThreadReference(
            index=0,
            related=((0, 2), (1, 3)),
            unrelated=((0, 1), (0, 3)),
            notes="synthetic",
        )
        outcome = fit(
            Inputs(
                source,
                turns,
                prepared,
                np.array([[1.0, 0.0], [0.0, 1.0], [1.0, 0.0], [0.0, 1.0]]),
                reference,
            ),
            Configuration(Method.AVERAGE_LINK, 0.5),
        )
        self.assertEqual(outcome.groups, 2)
        self.assertIsNotNone(outcome.all_pairs)
        self.assertEqual(outcome.all_pairs.f1 if outcome.all_pairs else 0, 1)

    def test_restart_reuse_failure_preserves_publication_and_deleted_units(
        self,
    ) -> None:
        encoder = FakeEncoder()
        source = thread((Role.USER, Role.ASSISTANT))
        turns = extract(source)
        prepared = units(source, turns[0], Recipe.JOINT, tokenizer())
        artifact = Artifact(
            source_sha256="source",
            protocol_sha256="protocol",
            encoder_generation="test",
            turns=turns,
            units=prepared,
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "gold.sqlite"
            store = Store(path)
            vectors, count = store.vectors(
                (prepared[0].text, prepared[0].text), encoder, lambda done, total: None
            )
            self.assertEqual(count, 1)
            store.publish("joint", artifact)
            store.close()
            store = Store(path)
            cached, count = store.vectors(
                (prepared[0].text,), encoder, lambda done, total: None
            )
            np.testing.assert_array_equal(cached[0], vectors[0])
            self.assertEqual(count, 0)
            encoder.fail = True
            with self.assertRaises(ValueError):
                store.vectors(("new text",), encoder, lambda done, total: None)
            self.assertEqual(store.read("joint"), artifact)
            deleted = artifact.model_copy(update={"units": (), "turns": ()})
            store.publish("joint", deleted)
            self.assertEqual(store.read("joint"), deleted)
            store.close()


if __name__ == "__main__":
    unittest.main()
