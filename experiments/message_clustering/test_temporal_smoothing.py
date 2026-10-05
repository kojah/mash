"""Temporal attraction is bounded, simultaneous and respects native branches."""

import unittest

import numpy as np

from corpus import Role
from temporal_smoothing import SETTINGS, Setting, lineages, links, smooth
from test_turns import thread
from turns import extract


class SmoothingContracts(unittest.TestCase):
    def test_control_is_bit_identical_and_two_turns_move_toward_each_other(self):
        vectors = np.array([[1.0, 0.0], [0.0, 1.0]])
        original = vectors.copy()
        owners = ("a", "b")
        ancestors = {"a": frozenset(), "b": frozenset(("a",))}
        np.testing.assert_array_equal(
            smooth(vectors, owners, ancestors, SETTINGS[0]), vectors
        )
        result = smooth(vectors, owners, ancestors, Setting(alpha=0.1, window=2))
        expected = np.array([[0.9, 0.1], [0.1, 0.9]])
        expected /= np.linalg.norm(expected, axis=1, keepdims=True)
        np.testing.assert_allclose(result, expected)
        np.testing.assert_array_equal(vectors, original)
        self.assertGreater(result[0] @ result[1], vectors[0] @ vectors[1])

    def test_window_distance_weights_and_zero_mean_fallback(self):
        owners = ("a", "b", "c")
        ancestors = {
            "a": frozenset(),
            "b": frozenset(("a",)),
            "c": frozenset(("a", "b")),
        }
        vectors = np.array([[1.0, 0.0], [0.0, 1.0], [1.0, 0.0]])
        result = smooth(vectors, owners, ancestors, Setting(alpha=0.1, window=2))
        mean = np.array([1.0, 2.0]) / np.sqrt(5)
        expected = 0.9 * vectors[0] + 0.1 * mean
        np.testing.assert_allclose(result[0], expected / np.linalg.norm(expected))
        cancelling = np.array([[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]])
        result = smooth(cancelling, owners, ancestors, Setting(alpha=0.1, window=1))
        np.testing.assert_array_equal(result[1], cancelling[1])
        isolated = {owner: frozenset() for owner in owners}
        np.testing.assert_array_equal(
            smooth(vectors, owners, isolated, SETTINGS[1]), vectors
        )

    def test_native_siblings_do_not_attract_but_parent_descendants_do(self):
        source = thread(
            (
                Role.USER,
                Role.ASSISTANT,
                Role.USER,
                Role.ASSISTANT,
                Role.USER,
                Role.ASSISTANT,
            )
        )
        messages = tuple(
            message.model_copy(
                update={"context_parent": -1 if i == 0 else 1 if i in (2, 4) else i - 1}
            )
            for i, message in enumerate(source.messages)
        )
        source = source.model_copy(update={"messages": messages})
        turns = extract(source)
        self.assertEqual(len(turns), 3)
        ancestors = lineages(source, turns)
        self.assertEqual(ancestors[turns[1].id], frozenset((turns[0].id,)))
        self.assertNotIn(turns[1].id, ancestors[turns[2].id])
        vectors = np.eye(3, dtype=np.float64)
        result = smooth(
            vectors,
            tuple(turn.id for turn in turns),
            ancestors,
            Setting(alpha=0.1, window=2),
        )
        # The last sibling has only the first turn as an eligible neighbor.
        np.testing.assert_allclose(result[2], np.array([0.1, 0.0, 0.9]) / np.sqrt(0.82))

    def test_invalid_inputs_reject_and_group_renumbering_does_not_change_links(self):
        self.assertEqual(len(SETTINGS), 5)
        self.assertEqual(len({setting.name for setting in SETTINGS}), 5)
        owners = ("a", "b")
        ancestors = {"a": frozenset(), "b": frozenset(("a",))}
        for vectors in (np.zeros((2, 2)), np.full((2, 2), np.nan)):
            with self.assertRaises(ValueError):
                smooth(vectors, owners, ancestors, SETTINGS[1])
        for setting in (Setting(alpha=-0.1, window=2), Setting(alpha=0.1, window=0)):
            with self.assertRaises(ValueError):
                smooth(np.eye(2), owners, ancestors, setting)
        np.testing.assert_array_equal(
            links(owners, {"a": frozenset((1,)), "b": frozenset((1, 2))}),
            links(owners, {"a": frozenset((9,)), "b": frozenset((9, 8))}),
        )


if __name__ == "__main__":
    unittest.main()
