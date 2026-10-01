import copy
import json
import unittest

from compare_passage_ranking import METHODS
from passage_scope import intervals
from reference_review import digest
from target_coverage import CONTROLS, coverage, evaluate, validate


class TargetCoverageTests(unittest.TestCase):
    def fixture(self):
        units = intervals('Keep topic.\n\nReject migration.\n\nMetadata.')['units']
        anchor, source = {'id': 'q', 'ordinal': 3}, {'id': 'a', 'ordinal': 1}
        manifest = json.dumps({'schema_version': 1, 'scope': 'development-source-passage-scope-diagnostic',
                               'rows': [{'thread': 't', 'anchor': anchor, 'source': source, 'units': units}]}).encode()
        scope = json.dumps({'schema_version': 1, 'scope': 'development-two-axis-passage-review',
                            'annotator': 'synthetic', 'manifest_sha256': digest(manifest), 'rows': [{
                            'thread': 't', 'anchor': anchor, 'source': source, 'rationale': 'synthetic fixture',
                            'units': [{**u, 'relevance': r, 'requested_scope': s} for u, (r, s) in zip(units,
                             (('topical', 'included'), ('topical', 'mixed'), ('metadata', 'not-applicable')), strict=True)]}]}).encode()
        annotation = {'schema_version': 1, 'scope': 'development-source-backed-target-coverage',
                      'annotator': 'synthetic', 'manifest_sha256': digest(manifest), 'scope_review_sha256': digest(scope),
                      'anchors': [{'thread': 't', 'anchor': anchor, 'rationale': 'named targets in original units',
                                   'targets': [{'id': 'keep', 'description': 'retained topic', 'requested_scope': 'included'},
                                               {'id': 'migration', 'description': 'rejected but topical', 'requested_scope': 'excluded'}],
                                   'units': [{'source_id': 'a', 'index': u['index'], 'text_sha256': u['text_sha256'],
                                              'targets': targets} for u, targets in zip(units, (['keep'], ['migration'], []), strict=True)]}]}
        chosen = [{'unit': {'source_id': 'a', **u}} for u in units]
        return manifest, scope, annotation, chosen

    def test_rejected_target_is_covered_without_becoming_included(self):
        manifest, scope, annotation, chosen = self.fixture()
        review, = validate(annotation, manifest, scope)
        result = coverage(chosen[:2], review)
        self.assertEqual((result['covered_total'], result['included_covered'], result['excluded_covered']), (2, 1, 1))
        self.assertEqual(coverage([], review)['covered_total'], 0)
        report = {'reports': [{'thread': 't', 'anchor': {'id': 'q'}, 'controls': {c: {
            q: {'status': 'ranked', 'budgets': {'1': {'chosen': chosen[:1]}, '3': {'chosen': chosen}}}
            for q in METHODS} for c in CONTROLS}}]}
        score = evaluate(report, (review,))
        self.assertEqual(score['own_support']['standalone']['current-user']['budgets']['3']['covered_total'], 2)
        overflow = copy.deepcopy(report)
        overflow['reports'][0]['controls']['standalone']['current-user']['budgets']['1']['chosen'] = chosen
        with self.assertRaises(ValueError): evaluate(overflow, (review,))
        for control in report['reports'][0]['controls'].values():
            control['preceding-user'] = {'status': 'unsupported-query-or-candidate-window', 'budgets': None}
        self.assertEqual(evaluate(report, (review,))['common_query_support']['standalone']['current-user']['supported_anchors'], 0)
        report['reports'][0]['controls'].pop('standalone')
        with self.assertRaises(ValueError): evaluate(report, (review,))

    def test_complete_unit_support_and_semantic_axes_are_required(self):
        manifest, scope, annotation, _ = self.fixture()
        for mutation in ('pin', 'missing', 'duplicate', 'unknown', 'unsupported', 'hash', 'mixed-included', 'nontopical'):
            changed = copy.deepcopy(annotation)
            row = changed['anchors'][0]
            if mutation == 'pin': changed['scope_review_sha256'] = 'bad'
            elif mutation == 'missing': row['units'].pop()
            elif mutation == 'duplicate': row['units'].append(copy.deepcopy(row['units'][0]))
            elif mutation == 'unknown': row['units'][0]['targets'] = ['absent']
            elif mutation == 'unsupported': row['targets'].append({'id': 'invented', 'description': 'absent', 'requested_scope': 'included'})
            elif mutation == 'hash': row['units'][0]['text_sha256'] = 'bad'
            elif mutation == 'mixed-included': row['units'][1]['targets'] = ['keep']
            else: row['units'][2]['targets'] = ['migration']
            with self.assertRaises(ValueError, msg=mutation): validate(changed, manifest, scope)

    def test_selected_evidence_must_be_original_and_unique(self):
        manifest, scope, annotation, chosen = self.fixture()
        review, = validate(annotation, manifest, scope)
        for mutation in ('duplicate', 'unknown', 'hash'):
            changed = copy.deepcopy(chosen)
            if mutation == 'duplicate': changed.append(copy.deepcopy(changed[0]))
            elif mutation == 'unknown': changed[0]['unit']['source_id'] = 'other'
            else: changed[0]['unit']['text_sha256'] = 'bad'
            with self.assertRaises(ValueError, msg=mutation): coverage(changed, review)
