import copy
import unittest

from compare_grouping import compare, nonmeasurements, partition


class GroupingComparisonTests(unittest.TestCase):
    def setUp(self):
        self.report = {"schema_version": 1, "scope": "development-online-grouping-control",
                       "source": "sessions", "method": "grinch", "canonical_identities": ["a", "b", "c"],
                       "dimensions": 384, "generation": "g", "dataset_fingerprint": "d",
                       "partition_sha256": "p", "parameters": {}, "code_sha256": {"tree": "same"},
                       "versions": {}, "reference_manifest_sha256": "r", "clusters": [["a", "b"], ["c"]],
                       "hierarchy": {"triplet_signature": {"seed": 1, "triplets": [[0, 1, 2]], "closest_pairs": [[0, 1]]},
                                     "clade_sha256": ["ab"]}}

    def test_cluster_labels_and_arrival_order_do_not_change_agreement(self):
        other = copy.deepcopy(self.report)
        other["clusters"].reverse()
        self.assertEqual(compare(self.report, other), {"native_cut_ari": 1., "triplet_agreement": 1., "nontrivial_clade_jaccard": 1.})

    def test_changed_sources_code_or_sampling_cannot_be_compared(self):
        for field in ("dataset_fingerprint", "code_sha256", "canonical_identities"):
            other = copy.deepcopy(self.report)
            other[field] = "changed"
            with self.assertRaisesRegex(ValueError, "incompatible"):
                compare(self.report, other)
        other = copy.deepcopy(self.report)
        other["hierarchy"]["triplet_signature"]["seed"] = 2
        with self.assertRaisesRegex(ValueError, "sampling"):
            compare(self.report, other)

    def test_missing_and_duplicate_membership_rejected(self):
        for clusters in ([["a", "b"]], [["a", "b"], ["b", "c"]], [["a", "b"], []]):
            report = copy.deepcopy(self.report)
            report["clusters"] = clusters
            with self.assertRaises(ValueError):
                partition(report)

    def test_repeat_comparison_removes_measurements_only(self):
        a = {**self.report, "fit_wall_ms": 1., "insertion_ms": {"p95": 1.}}
        b = {**self.report, "fit_wall_ms": 2., "insertion_ms": {"p95": 2.}}
        self.assertEqual(nonmeasurements(a), nonmeasurements(b))
        b["inference_calls"] = 1
        self.assertNotEqual(nonmeasurements(a), nonmeasurements(b))
