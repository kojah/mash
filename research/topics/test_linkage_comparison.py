import copy
import unittest

from linkage_comparison import compare_legacy


class LegacyTests(unittest.TestCase):
    def test_only_declared_schema_and_provenance_changes_are_allowed(self):
        legacy = {'version': 1, 'policy': {'cut': False}, 'code_pins': {'source': 'old'}, 'timings': {'fit': 1},
                  'inputs': {'ids': ['a', 'b']}, 'results': {'lca': 2}}
        current = {**legacy, 'version': 2, 'policy': {'cut': False, 'linkage': 'centroid'},
                   'code_pins': {'source': 'new'}, 'timings': {'fit': 2}}
        compare_legacy(legacy, current)
        for key, value in [('policy', {'cut': True, 'linkage': 'centroid'}), ('inputs', {'ids': ['b', 'a']}),
                           ('results', {'lca': 3}), ('version', 3)]:
            changed = copy.deepcopy(current)
            changed[key] = value
            with self.assertRaises(ValueError):
                compare_legacy(legacy, changed)
        changed = {**current, 'extra': 'silent-new-decision'}
        with self.assertRaises(ValueError):
            compare_legacy(legacy, changed)


if __name__ == '__main__':
    unittest.main()
