"""Frozen vectors stay separate; full-answer points restore omitted segments."""

import hashlib
import sqlite3
import unittest

import numpy as np

from build_input_followup import Built
from corpus import Role
from followup_inputs import AnswerScope, Variant, prepare
from study_artifacts import separate_units
from test_turns import thread, tokenizer
from turn_artifact import Artifact
from turn_study_corpus import Phase
from turns import extract
from window_points import (
    Representation,
    cached_vector,
    prepare_points,
    validate_raw_unit,
)


class WindowPoints(unittest.TestCase):
    def test_unpooling_keeps_vectors_and_full_answers_restore_source_coverage(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.ASSISTANT, Role.USER),
            ("question", "important earlier answer", "final answer", "follow up"),
        )
        turns = extract(source)
        inputs = tuple(
            item
            for index in range(len(turns))
            for item in prepare(
                source,
                turns,
                index,
                Variant.CONTEXT_MEAN,
                tokenizer(),
                AnswerScope.LAST,
            )
        )
        built = Built(
            thread_index=0,
            phase=Phase.DEVELOPMENT,
            variant=Variant.CONTEXT_MEAN,
            seconds=0,
            new_vectors=0,
            input_tokens=0,
            matrix_sha256="test",
            turn_ids=tuple(turn.id for turn in turns),
            inputs=inputs,
        )
        artifact = Artifact(
            source_sha256="test",
            protocol_sha256="test",
            encoder_generation="test",
            turns=turns,
            units=tuple(
                unit
                for turn in turns
                for unit in separate_units(source, turn, tokenizer())
            ),
        )
        with sqlite3.connect(":memory:") as cache:
            cache.execute(
                "CREATE TABLE vectors (generation TEXT, text_hash TEXT, vector BLOB)"
            )
            texts = sorted(
                {item.content.text for item in inputs}
                | {unit.text for unit in artifact.units}
            )
            for index, text in enumerate(texts):
                vector = np.eye(1, 384, index, dtype=np.float64)[0]
                cache.execute(
                    "INSERT INTO vectors VALUES (?,?,?)",
                    (
                        "test",
                        hashlib.sha256(text.encode()).hexdigest(),
                        vector.astype("<f8").tobytes(),
                    ),
                )
            before = cache.total_changes
            lean = prepare_points(
                built,
                artifact,
                source,
                turns,
                cache,
                "test",
                Representation.CONTEXT_WINDOWS,
            )
            full = prepare_points(
                built,
                artifact,
                source,
                turns,
                cache,
                "test",
                Representation.FULL_ANSWERS,
            )
            self.assertEqual(cache.total_changes, before)
            self.assertEqual(lean.owners, (turns[0].id, turns[0].id, turns[1].id))
            self.assertEqual(full.owners, lean.owners)
            np.testing.assert_array_equal(
                lean.vectors[0], cached_vector(cache, "test", inputs[0].content.text)
            )
            answer = next(
                unit for unit in artifact.units if 1 in unit.represented_sources
            )
            self.assertIn("important earlier answer", answer.text)
            np.testing.assert_array_equal(
                full.vectors[1], cached_vector(cache, "test", answer.text)
            )
            self.assertFalse(np.array_equal(lean.vectors[1], full.vectors[1]))
            with self.assertRaises(ValueError):
                cached_vector(cache, "wrong-generation", texts[0])

    def test_raw_unit_rejects_changed_text_and_cross_turn_sources(self):
        source = thread(
            (Role.USER, Role.ASSISTANT, Role.USER), ("question", "answer", "other")
        )
        turns = extract(source)
        unit = separate_units(source, turns[0], tokenizer())[-1]
        self.assertEqual(validate_raw_unit(unit, turns[0], source), Role.ASSISTANT)
        with self.assertRaises(ValueError):
            validate_raw_unit(
                unit.model_copy(update={"text": "injected " + unit.text}),
                turns[0],
                source,
            )
        with self.assertRaises(ValueError):
            validate_raw_unit(unit, turns[1], source)


if __name__ == "__main__":
    unittest.main()
