import copy
import json
import unittest

from compare_passage_ranking import METHODS
from passage_ranking import counts
from passage_scope import Judgment, Relevance, Scope, capture
from preceding_context import Message, Role, preceding
from section_experiment import run, stable, summarize


class SectionExperimentTests(unittest.TestCase):
    def fixture(self):
        text = 'Remaining:\n\n- α\n- 🙂\n\n```\ncode\n```'
        interval = preceding((Message('a', 0, Role.ASSISTANT, -1, text, text),
                              Message('q', 1, Role.USER, 0, 'do remaining', 'do remaining')), 1, maximum_users=3)
        context = {'cases': [{'thread': 't', 'controls': {'raw64': {'contexts': [interval]}}}]}
        rows = capture(context, [{'thread': 't', 'anchor': 1, 'source': 0}])
        judgments = tuple(Judgment('t', 1, 0, u['index'], Relevance.TOPICAL if u['index'] in (1, 2) else Relevance.METADATA,
                                  Scope.INCLUDED if u['index'] in (1, 2) else Scope.NA) for u in rows[0]['units'])
        ranking = [{'source_id': 'a', 'source_ordinal': 0, **u, 'cosine': 0.5} for u in rows[0]['units']]
        methods = {name: {'status': 'ranked', 'ranking': ranking,
                         'topk': {str(b): counts(range(min(b, len(ranking))), judgments) for b in (1, 3)}} for name in METHODS}
        baseline = {'reports': [{'thread': 't', 'anchor': interval['anchor'], 'candidate_units': len(ranking), 'methods': methods}]}
        return context, {'rows': rows}, judgments, baseline

    def test_layout_selection_preserves_budget_and_does_not_use_judgments(self):
        inputs = self.fixture()
        result = run(*inputs)
        row = result['reports'][0]
        self.assertEqual(row['controls']['standalone']['current-user']['budgets']['1']['counts']['included'], 0)
        expanded = row['controls']['section-child']['current-user']['budgets']['1']
        self.assertEqual(expanded['counts']['included'], 1)
        self.assertEqual(expanded['chosen'][0]['unit']['text_sha256'], inputs[1]['rows'][0]['units'][1]['text_sha256'])
        changed = list(inputs[2])
        changed[1] = Judgment('t', 1, 0, 1, Relevance.TOPICAL, Scope.EXCLUDED)
        baseline = copy.deepcopy(inputs[3])
        for m in baseline['reports'][0]['methods'].values():
            m['topk'] = {str(b): counts(range(min(b, len(changed))), changed) for b in (1, 3)}
        other = run(inputs[0], inputs[1], changed, baseline)
        self.assertEqual(expanded['chosen'], other['reports'][0]['controls']['section-child']['current-user']['budgets']['1']['chosen'])
        self.assertEqual(summarize(result)['associations'], 1)

    def test_unsupported_queries_stay_unsupported_and_counts_must_match(self):
        context, manifest, judgments, baseline = self.fixture()
        baseline['reports'][0]['methods']['preceding-user'] = {'status': 'unsupported-query-or-candidate-window', 'ranking': [], 'topk': None}
        result = run(context, manifest, judgments, baseline)
        for control in result['reports'][0]['controls'].values():
            self.assertIsNone(control['preceding-user']['budgets'])
        self.assertEqual(summarize(result)['common_query_support']['standalone']['current-user']['supported_anchors'], 0)
        baseline['reports'][0]['methods']['current-user']['topk']['1']['included'] = 1
        with self.assertRaises(ValueError): run(context, manifest, judgments, baseline)

    def test_repeat_requires_offsets_children_counts_and_full_grids(self):
        result = run(*self.fixture())
        changed = copy.deepcopy(result)
        changed['layout_ms'] += 2
        self.assertEqual(stable(result), stable(changed))
        for mutation in ('coordinates', 'association', 'counts', 'grid'):
            changed = copy.deepcopy(result)
            row = changed['reports'][0]
            if mutation == 'coordinates': row['controls']['standalone']['current-user']['budgets']['1']['chosen'][0]['unit']['byte_end'] -= 1
            elif mutation == 'association': row['associations'][0]['children'] = (2,)
            elif mutation == 'counts': row['controls']['section-child']['current-user']['budgets']['1']['counts']['included'] = 0
            else: row['controls'].pop('standalone')
            self.assertNotEqual(stable(result), stable(changed))
        self.assertEqual(stable(result), stable(json.loads(json.dumps(result))))

    def test_optional_control_preserves_original_selections(self):
        original = run(*self.fixture())
        added = run(*self.fixture(), one_child=True)
        self.assertIn('section-one-child', summarize(added)['own_support'])
        for a, b in zip(original['reports'], added['reports'], strict=True):
            self.assertEqual(a['controls'], {k: v for k, v in b['controls'].items() if k != 'section-one-child'})
