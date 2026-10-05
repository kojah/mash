"""Selective attraction cannot move long targets or cross semantic/branch gates."""

import unittest

import numpy as np

from build_input_followup import Built
from corpus import Role
from followup_inputs import Input, Text, Variant
from selective_smoothing import eligibility, refine
from turn_study_corpus import Phase


class SelectiveContracts(unittest.TestCase):
    def test_both_length_caps_and_own_inventory(self) -> None:
        built = Built(
            thread_index=40,
            phase=Phase.DEVELOPMENT,
            variant=Variant.LEAN,
            seconds=0,
            new_vectors=0,
            input_tokens=128,
            matrix_sha256="fixture",
            turn_ids=("a",),
            inputs=(
                Input(
                    turn_id="a", role=Role.USER, content=Text(text="prompt"), tokens=32
                ),
                Input(
                    turn_id="a",
                    role=Role.ASSISTANT,
                    content=Text(text="answer"),
                    tokens=96,
                ),
            ),
        )
        self.assertTrue(eligibility(built)[0].eligible)
        for index in (0, 1):
            changed = list(built.inputs)
            changed[index] = changed[index].model_copy(
                update={"tokens": changed[index].tokens + 1}
            )
            self.assertFalse(
                eligibility(built.model_copy(update={"inputs": tuple(changed)}))[
                    0
                ].eligible
            )
        with self.assertRaises(ValueError):
            eligibility(built.model_copy(update={"variant": Variant.CONTEXT_MEAN}))

    def test_semantic_gate_and_target_scope(self) -> None:
        vectors = np.array([[1.0, 0.0], [0.8, 0.6], [0.0, 1.0]])
        owners = ("a", "b", "c")
        ancestors = {
            "a": frozenset(),
            "b": frozenset({"a"}),
            "c": frozenset({"a", "b"}),
        }
        result = refine(vectors, owners, ancestors, frozenset({"a"}))
        expected = 0.9 * vectors[0] + 0.1 * vectors[1]
        np.testing.assert_allclose(result[0], expected / np.linalg.norm(expected))
        np.testing.assert_array_equal(result[1:], vectors[1:])
        np.testing.assert_array_equal(vectors[0], [1.0, 0.0])

    def test_siblings_and_below_floor_leave_targets_unchanged(self) -> None:
        owners = ("a", "b")
        vectors = np.array([[1.0, 0.0], [0.8, 0.6]])
        no_relation = {"a": frozenset(), "b": frozenset()}
        np.testing.assert_array_equal(
            refine(vectors, owners, no_relation, frozenset(owners)), vectors
        )
        relation = {"a": frozenset(), "b": frozenset({"a"})}
        np.testing.assert_array_equal(
            refine(np.eye(2), owners, relation, frozenset(owners)), np.eye(2)
        )
        with self.assertRaises(ValueError):
            refine(vectors, owners, relation, frozenset({"missing"}))


if __name__ == "__main__":
    unittest.main()
