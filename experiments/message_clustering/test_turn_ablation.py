"""Source ownership, baseline parity and fixed fusion/penalty contracts."""

import unittest

import numpy as np

from ablation_evidence import (
    CELLS,
    Background,
    Evidence,
    fit,
    fuse,
    generic,
    memberships,
)
from ablation_inputs import prepare, validate
from corpus import Role
from followup_inputs import AnswerScope, source_text
from structure_chunks import Mode, prepare_structured
from study_artifacts import View
from study_scoring import Candidate, Family
from study_scoring import fit as original_fit
from test_turns import thread, tokenizer
from turns import extract


class AblationContracts(unittest.TestCase):
    def test_all_answer_segments_receive_own_prompt_without_losing_chunks(self) -> None:
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT),
            (
                "own request",
                "# First\nSubstantive explanation.",
                "# Second\nFinal details.",
            ),
        )
        turns = extract(source)
        original = prepare_structured(
            source, turns, 0, Mode.HEADING, tokenizer(), AnswerScope.ALL
        )
        conditioned = prepare(original, source, turns, tokenizer(), AnswerScope.ALL)
        answers = tuple(u for u in conditioned if u.body.role == Role.ASSISTANT)
        self.assertGreaterEqual(len(answers), 2)
        self.assertTrue(all("own request" in u.content.text for u in answers))
        self.assertTrue(all(u.selection is not None for u in answers))
        self.assertEqual(
            {
                location.source.source
                for u in answers
                for location in u.body.content.locations
            },
            {1, 2},
        )
        self.assertEqual(conditioned[0].body, original[0].input)

    def test_answer_conditioning_preserves_prompts_and_checks_ownership(self) -> None:
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.USER, Role.ASSISTANT),
            (
                "old request",
                "old answer",
                "current question",
                "# Heading\nDetailed answer.",
            ),
        )
        turns = extract(source)
        old = tuple(
            unit
            for index in range(len(turns))
            for unit in prepare_structured(
                source, turns, index, Mode.HEADING, tokenizer()
            )
        )
        new = prepare(old, source, turns, tokenizer())
        self.assertEqual(new[0].content, old[0].input.content)
        answer = new[-1]
        self.assertIn("current question", answer.content.text)
        self.assertNotIn("old request", answer.content.text)
        self.assertIsNotNone(answer.selection)
        assert answer.selection is not None
        self.assertEqual(
            answer.content.text[answer.selection.start :], answer.body.content.text
        )
        owners = tuple(t.id for t in turns)
        with self.assertRaises(ValueError):
            validate(
                (answer.model_copy(update={"prompt": source_text(source, 0)}),),
                owners,
                source,
                turns,
            )

    def test_extra_context_splits_large_body_without_losing_source_text(self) -> None:
        source = thread(
            (Role.USER, Role.ASSISTANT),
            (" ".join(["request"] * 80), " ".join(["answer"] * 490)),
        )
        turns = extract(source)
        old = prepare_structured(source, turns, 0, Mode.HEADING, tokenizer())
        new = prepare(old, source, turns, tokenizer())
        self.assertGreater(len(new), len(old))
        self.assertTrue(
            all(len(tokenizer().encode(u.content.text).ids) <= 512 for u in new)
        )

    def test_evidence_reproduces_original_memberships_and_distinct_turn_support(
        self,
    ) -> None:
        vectors = np.array([[1.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.0, 1.0]])
        owners = ("a", "b", "c", "c")
        expected = original_fit(
            vectors,
            owners,
            Candidate(
                view=View.SEPARATE, family=Family.BIRCH, cutoff=0.75, radius=0.60
            ),
        )
        actual = fit(vectors, owners)
        self.assertEqual(memberships(actual, 0.75, (0.0,) * 4), expected.overlapping)
        self.assertFalse(memberships(actual, 0.75, (0.0,) * 4)["c"])

    def test_consensus_keeps_weak_agreement_but_rejects_weak_single_view(self) -> None:
        a = Evidence(("a", "b", "c"), (0, 0, 0), ({0: 0.77}, {0: 0.77}, {0: 0.77}))
        b = Evidence(("a", "b", "c"), (0, 0, 1), ({0: 0.77}, {0: 0.77}, {}))
        union = fuse(a, b, (0.0,) * 3, False)
        strict = fuse(a, b, (0.0,) * 3, True)
        self.assertTrue(union["a"] & union["c"])
        self.assertTrue(strict["a"] & strict["b"])
        self.assertFalse(strict["a"] & strict["c"])
        penalized = fuse(a, b, (0.05,) * 3, True)
        self.assertFalse(penalized["a"] & penalized["b"])

    def test_generic_term_floor_and_frequency_boundary(self) -> None:
        background = Background(
            documents=100,
            document_frequency={"database": 21, "schema": 21, "migration": 21},
        )
        self.assertTrue(generic("thanks, done", background))
        self.assertTrue(generic("database schema migration", background))
        self.assertFalse(
            generic(
                "database schema migration",
                background.model_copy(update={"document_frequency": {"schema": 20}}),
            )
        )
        self.assertTrue(generic("rare_identifier migration", background))
        self.assertEqual(len(CELLS), 8)
        self.assertEqual(len({c.name for c in CELLS}), 8)


if __name__ == "__main__":
    unittest.main()
