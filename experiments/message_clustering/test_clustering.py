"""Synthetic contract tests, never real-data quality evidence."""

import math
import unittest

import numpy as np

from clustering import (
    CONFIGURATIONS,
    Configuration,
    Method,
    OnlineLeader,
    cluster,
    decode_stored_vector,
)


class ClusteringContracts(unittest.TestCase):
    def test_rust_signed_byte_codec_and_corrupt_metadata(self) -> None:
        decoded = decode_stored_vector(bytes([253, 4]), 2, 5.0)
        np.testing.assert_allclose(decoded, [-0.6, 0.8])
        for blob, dimensions, norm in (
            (bytes([3, 4]), 3, 5.0),
            (bytes([0, 0]), 2, 0.0),
            (bytes([3, 4]), 2, math.nan),
            (bytes([3, 4]), 2, 8.0),
        ):
            with self.subTest(blob=blob, dimensions=dimensions, norm=norm):
                with self.assertRaises(ValueError):
                    decode_stored_vector(blob, dimensions, norm)

    def test_returning_subjects_and_deterministic_representatives(self) -> None:
        vectors = np.array([[1, 0], [0, 1], [1, 0], [0, 1]], dtype=np.float64)
        for setting in CONFIGURATIONS:
            with self.subTest(setting=setting):
                result = cluster(vectors, setting)
                self.assertEqual(result.partition, (0, 1, 0, 1))
                self.assertEqual(
                    [group.representative for group in result.groups], [0, 1]
                )
                self.assertEqual(result, cluster(vectors, setting))

    def test_incremental_leader_matches_replay_and_rejects_dimension_change(
        self,
    ) -> None:
        vectors = np.array([[1, 0], [0, 1], [1, 0]], dtype=np.float64)
        leader = OnlineLeader(0.65)
        leader.append(vectors[0])
        for vector in vectors[1:]:
            leader.append(vector)
        replay = cluster(vectors, Configuration(Method.LEADER, 0.65))
        self.assertEqual(tuple(leader.labels), replay.partition)
        with self.assertRaises(ValueError):
            leader.append(np.ones(3))
        self.assertEqual(tuple(leader.labels), replay.partition)

    def test_overlap_does_not_mutate_partition(self) -> None:
        vectors = np.array([[1, 0]] * 10 + [[0, 1]] * 10 + [[1, 1]], dtype=np.float64)
        result = cluster(vectors, Configuration(Method.LEADER, 0.8))
        # The mixed singleton is not retained; threshold rejection is explicit.
        self.assertEqual(result.partition[-1], 2)
        self.assertEqual(result.memberships[-1], ())
        result = cluster(vectors, Configuration(Method.LEADER, 0.65))
        self.assertEqual(len(result.memberships[-1]), 2)
        self.assertEqual(result.partition[-1], 0)

    def test_noise_singletons_and_empty_input(self) -> None:
        for setting in CONFIGURATIONS:
            self.assertEqual(cluster(np.empty((0, 2)), setting).memberships, ())
            self.assertEqual(cluster(np.array([[1.0, 0.0]]), setting).groups, ())
            mask = np.array([True, False], dtype=np.bool_)
            self.assertEqual(
                cluster(np.array([[1.0, 0.0], [1.0, 0.0]]), setting, mask).groups, ()
            )

    def test_invalid_vectors_and_configurations(self) -> None:
        for vectors in (np.array([[0.0, 0.0]]), np.array([[math.nan, 1.0]])):
            with self.assertRaises(ValueError):
                cluster(vectors, CONFIGURATIONS[0])
        with self.assertRaises(ValueError):
            Configuration(Method.BIRCH, 0.5)
        with self.assertRaises(ValueError):
            Configuration(Method.LEADER, 0.5, 0.4)


if __name__ == "__main__":
    unittest.main()
