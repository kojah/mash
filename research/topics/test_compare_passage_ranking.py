import copy
import json
import unittest

from compare_passage_ranking import METHODS, compare
from reference_review import digest


class ComparePassageTests(unittest.TestCase):
    def test_complete_source_units_and_scores_are_required(self):
        manifest = json.dumps({'rows': [{'thread': 't', 'anchor': {'id': 'q'}, 'source': {'id': 'a', 'ordinal': 1},
                                        'units': [{'index': 0, 'text_sha256': 'h'}]}]}).encode()
        methods = {name: {'status': 'ranked', 'ranking': [{'source_id': 'a', 'index': 0,
                   'source_ordinal': 1, 'text_sha256': 'h', 'cosine': None if name == 'recency' else 0.5}],
                   'topk': {str(k): {f: int(f in ('selected', 'non_topical')) for f in ('selected', 'topical', 'included', 'mixed', 'excluded', 'unspecified', 'non_topical')}
                            for k in (1, 3)}} for name in METHODS}
        report = {'schema_version': 1, 'scope': 'development-two-axis-passage-ranking-diagnostic',
                  'input_sha256': {'manifest': digest(manifest)}, 'reports': [{'thread': 't', 'anchor': {'id': 'q'},
                   'candidate_units': 1, 'methods': methods}], 'elapsed_ms_after_imports': 1,
                  'encoder': {'inference_ms': 1, 'calls': 1, 'cache_hits': 0}}
        changed = copy.deepcopy(report)
        changed['encoder']['inference_ms'] = 2
        self.assertTrue(compare(report, changed, manifest)['identical_nonmeasurement_fields'])
        changed['reports'][0]['methods']['current-user']['ranking'][0]['cosine'] = 0.6
        with self.assertRaises(ValueError): compare(report, changed, manifest)
        for mutation in ('missing-unit', 'missing-method', 'missing-anchor', 'duplicate', 'nan', 'abstention', 'coordinates', 'counts'):
            bad = copy.deepcopy(report)
            method = bad['reports'][0]['methods']['current-user']
            if mutation == 'missing-unit': method['ranking'] = []
            elif mutation == 'missing-method': bad['reports'][0]['methods'].pop('recency')
            elif mutation == 'missing-anchor': bad['reports'] = []
            elif mutation == 'duplicate': method['ranking'] *= 2
            elif mutation == 'nan': method['ranking'][0]['cosine'] = float('nan')
            elif mutation == 'coordinates': method['ranking'][0]['start'] = 4
            elif mutation == 'counts': method['topk']['1']['mixed'] = 1
            else: method['status'] = 'unsupported-query-or-candidate-window'
            with self.assertRaises(ValueError, msg=mutation): compare(bad, bad, manifest)
