import unittest

import numpy as np

from pair_review import Key, Pair, Relation
from preceding_context import Role
from source_spans import Kind
from span_units import Unit
from unit_grouping import Geometry, Method, Point, Representation, cluster, evaluate, geometries, keywords, points


def unit(ordinal, text, index=0, thread="a"):
    return Unit(Key(thread, ordinal, index), Role.USER, Kind.PROSE, 0, len(text), "id", "hash", text)


class UnitGroupingTests(unittest.TestCase):
    def test_geometry_uses_fixed_matched_support_and_hybrid_cosine(self):
        units = (unit(0, "database timeout"), unit(1, "database connection"),
                 unit(2, "the and"), unit(3, "unencoded source"))
        dense = {units[0].key: np.array([1., 0.]), units[1].key: np.array([0., 1.]),
                 units[2].key: np.array([1., 0.]), units[3].key: None}
        features = geometries(units, dense)
        for geometry in Geometry:
            self.assertIsNone(features[geometry][units[2].key])
            self.assertIsNone(features[geometry][units[3].key])
        lexical = features[Geometry.LEXICAL]
        hybrid = features[Geometry.HYBRID]
        self.assertEqual(len(lexical[units[0].key]), 512)
        self.assertAlmostEqual(np.linalg.norm(hybrid[units[0].key]), 1)
        self.assertAlmostEqual(hybrid[units[0].key] @ hybrid[units[1].key],
                               .5 * (lexical[units[0].key] @ lexical[units[1].key]))
        again = geometries(tuple(reversed(units)), dense)
        np.testing.assert_array_equal(lexical[units[0].key], again[Geometry.LEXICAL][units[0].key])

    def test_pooling_preserves_ownership_and_abstains_on_cancellation(self):
        units = (unit(0, "cache", 0), unit(0, "locks", 1), unit(1, "pool"), unit(0, "other", thread="b"))
        vectors = dict(zip((u.key for u in units), (np.array([1., 0.]), np.array([-1., 0.]),
                                                   np.array([0., 1.]), None)))
        self.assertEqual(len(points(units, vectors, Representation.SPAN)), 3)
        pooled = points(units, vectors, Representation.RECORD)
        self.assertEqual([p.keys for p in pooled], [(units[2].key,)])
        thread = points(units, vectors, Representation.THREAD)
        self.assertEqual(thread[0].keys, tuple(u.key for u in units[:3]))
        np.testing.assert_array_equal(thread[0].vector, [0, 1])

    def test_noise_is_not_shared_topic_and_uncertainty_not_negative(self):
        keys = [Key("a", i, 0) for i in range(3)]
        represented = tuple(Point((k,), np.array([1., 0.])) for k in keys)
        review = (Pair(keys[0], keys[1], Relation.OVERLAP),
                  Pair(keys[0], keys[2], Relation.UNCERTAIN))
        result = evaluate(review, represented, np.array([-1, -1, -1]))
        self.assertEqual(result["metrics"]["uncertain_excluded"], 1)
        self.assertEqual(result["metrics"]["native_coassigned"]["false_negative"], 1)
        self.assertFalse(result["rows"][0]["native_coassigned"])
        self.assertEqual(evaluate(review, represented[:1], np.array([0]))["metrics"]["native_coassigned"]["abstentions"], 1)

    def test_centroid_diagnostic_permits_multiple_aspects(self):
        keys = [Key("a", i, 0) for i in range(3)]
        represented = tuple(Point((k,), v) for k, v in zip(keys, (
            np.array([1., 0.]), np.array([0., 1.]), np.array([1., 1.]) / np.sqrt(2))))
        review = tuple(Pair(keys[i], keys[2], Relation.OVERLAP) for i in (0, 1))
        result = evaluate(review, represented, np.array([0, 1, -1]))
        self.assertEqual(result["metrics"]["centroid07_shared"]["true_positive"], 2)
        self.assertEqual(result["metrics"]["native_coassigned"]["false_negative"], 2)

    def test_small_and_empty_clustering_do_not_invent_density_topics(self):
        single = (Point((Key("a", 0, 0),), np.array([1., 0.])),)
        for method in Method:
            self.assertEqual(cluster((), method, False)[0].size, 0)
            self.assertEqual(cluster(single, method, True)[0].tolist(), [-1] if method == Method.HDBSCAN else [0])

    def test_label_phrases_cannot_cross_source_records(self):
        units = (unit(0, "database"), unit(1, "timeout"))
        represented = tuple(Point((u.key,), np.array([1., 0.])) for u in units)
        result = keywords(units, represented, np.array([0, 0]))
        terms = {t["term"] for t in result[0]["keywords"]}
        self.assertEqual(terms, {"database", "timeout"})
        self.assertEqual(keywords(units, represented, np.array([-1, -1])), [])
