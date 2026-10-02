import json
import unittest

try:
    from qmsum_numeric_guidance import payload
    from numeric_guidance import retrieve
except ImportError:
    payload = None


@unittest.skipIf(payload is None, 'optional scientific NumPy/SciPy environment required')
class NumericPayload(unittest.TestCase):
    def test_exact_weights_survive_json_round_trip_without_fraction_objects(self):
        result = payload(retrieve([.9, .8], [99, 3], [20, 10], [.8, .9], [20, 10]))
        decoded = json.loads(json.dumps(result, allow_nan=False))
        self.assertEqual(decoded, result)
        rows = decoded['returned_sources']
        self.assertEqual([r['id'] for r in rows], [3, 99])
        self.assertEqual((rows[0]['score_numerator'], rows[0]['score_denominator']), (123, 3782))
        self.assertIs(type(rows[0]['source_key']), int)
        self.assertIs(type(rows[0]['parent_rank']), int)

    def test_nonpositive_parent_round_trips_as_absent_rank_and_key(self):
        decoded = json.loads(json.dumps(payload(retrieve([.8], [5], [10], [0.], [10])), allow_nan=False))
        row = decoded['returned_sources'][0]
        self.assertIsNone(row['parent_rank'])
        self.assertIsNone(row['parent_key'])
        self.assertEqual(row['parent_group'], 10)
        self.assertEqual((row['score_numerator'], row['score_denominator']), (1, 61))


if __name__ == '__main__': unittest.main()
