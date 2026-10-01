"""Meaningful missing-vector and abstention controls for the boundary pilot."""
import unittest
import sqlite3

import numpy as np

from episodes import Judgment, Method, Turn, predictions, score, read_cases, read_turns, source_hash
from test_data import FrozenInputFixture


class EpisodeTests(unittest.TestCase):
    def test_missing_turn_is_not_bridged(self):
        x = np.array([1.0, 0.0])
        y = np.array([0.0, 1.0])
        turns = (Turn("a", 0, "alpha", x), Turn("b", 2, "missing", None),
                 Turn("c", 4, "beta", y), Turn("d", 7, "beta again", y))
        for method in (Method.ADJACENT, Method.CENTROID):
            self.assertEqual(predictions(turns, method), (None, None, False))

    def test_centroid_and_adjacent_have_distinct_behavior(self):
        vectors = [np.array([1.0, 0.0]), np.array([0.6, 0.8]), np.array([0.0, 1.0])]
        turns = tuple(Turn(str(i), i, str(i), vector) for i, vector in enumerate(vectors))
        self.assertEqual(predictions(turns, Method.ADJACENT), (False, False))
        self.assertEqual(predictions(turns, Method.CENTROID), (False, True))

    def test_uncertainty_excluded_abstention_not_rewarded(self):
        metrics = score([Judgment.CHANGE, Judgment.SAME, Judgment.UNCERTAIN], [None, False, True])
        self.assertEqual(metrics["known"], 2)
        self.assertEqual(metrics["abstained"], 1)
        self.assertEqual(metrics["recall_including_abstentions"], 0)
        self.assertIsNone(metrics["precision"])


class AnnotationInputTests(FrozenInputFixture):
    def annotations(self, thread):
        with sqlite3.connect(self.db) as connection:
            turns = read_turns(connection, thread, "frozen")
        path = self.root / "annotations.toml"
        path.write_text(f'''schema_version = 1
generation = "frozen"
[[cases]]
thread = "{thread}"
source_sha256 = "{source_hash(turns)}"
[[cases.boundaries]]
left = 0
right = 1
judgment = "same"
rationale = "Same visible subject."
''')
        return path

    def test_changed_source_invalidates_annotations(self):
        path = self.annotations("b")
        _, cases = read_cases(self.db, self.partition, path)
        self.assertEqual(len(cases), 1)
        with sqlite3.connect(self.db) as db:
            db.execute("UPDATE messages SET search_projection='edited subject' WHERE id='b-1'")
        with self.assertRaisesRegex(ValueError, "source changed"):
            read_cases(self.db, self.partition, path)

    def test_development_annotations_reject_reserved_group(self):
        with self.assertRaisesRegex(ValueError, "reserved groups"):
            read_cases(self.db, self.partition, self.annotations("a"))
