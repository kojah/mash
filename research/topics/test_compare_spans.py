from copy import deepcopy
import unittest

from compare_spans import compare
from unit_grouping import Geometry, Method, Representation


class SpanRepeatTests(unittest.TestCase):
    def fixture(self):
        return {"schema_version": 1, "scope": "development-matched-source-unit-grouping",
                "dataset_fingerprint": "pinned", "elapsed_ms_after_imports": 1, "peak_rss_mib": 2,
                "encoder": {"calls": 10, "inference_ms": 3},
                "reports": [{"geometry": g, "representation": r, "method": m, "reverse": reverse,
                             "grouping_ms": 1, "clusters": [], "metrics": {"average_precision": .5}}
                            for g in Geometry for r in Representation for m in Method for reverse in (False, True)]}

    def test_measurements_may_change_but_predictions_sources_and_calls_cannot(self):
        first = self.fixture()
        second = deepcopy(first)
        second["elapsed_ms_after_imports"] = 7
        second["encoder"]["inference_ms"] = 9
        second["reports"][0]["grouping_ms"] = 4
        self.assertEqual(compare(first, second)["controls"], 72)
        for target in ("source", "calls", "prediction", "grid"):
            changed = deepcopy(second)
            if target == "source":
                changed["dataset_fingerprint"] = "other"
            elif target == "calls":
                changed["encoder"]["calls"] += 1
            elif target == "prediction":
                changed["reports"][0]["metrics"]["average_precision"] = .6
            else:
                changed["reports"][0] = deepcopy(changed["reports"][1])
            with self.assertRaises(ValueError):
                compare(first, changed)
