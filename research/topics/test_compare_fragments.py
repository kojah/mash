from copy import deepcopy
import unittest

from compare_fragments import compare
from fragments import Mode
from unit_grouping import Method


class FragmentRepeatTests(unittest.TestCase):
    def fixture(self):
        return {"schema_version": 1, "scope": "development-bounded-source-fragment-aspects",
                "elapsed_ms_after_imports": 1, "cpu_ms": 2, "peak_rss_mib": 3,
                "captures": [{"mode": mode, "encoding_p95_ms": 2, "coverage": [{"parent": "source"}],
                              "reports": [{"method": m, "reverse": r, "metrics": {"AP": .8}}
                                          for m in Method for r in (False, True)]} for mode in Mode]}

    def test_only_measurements_may_change_and_complete_grid_required(self):
        first = self.fixture()
        changed = deepcopy(first)
        changed["cpu_ms"] = 10
        changed["captures"][0]["encoding_p95_ms"] = 8
        self.assertEqual(compare(first, changed)["controls"], 24)
        for kind in ("coverage", "prediction", "mode", "method"):
            bad = deepcopy(changed)
            if kind == "coverage":
                bad["captures"][0]["coverage"][0]["parent"] = "other"
            elif kind == "prediction":
                bad["captures"][0]["reports"][0]["metrics"]["AP"] = .9
            elif kind == "mode":
                bad["captures"][0] = deepcopy(bad["captures"][1])
            else:
                bad["captures"][0]["reports"][0] = deepcopy(bad["captures"][0]["reports"][1])
            with self.assertRaises(ValueError):
                compare(first, bad)
