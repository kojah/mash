import copy
import json
import unittest

from blind_topic_packet import Pair, Stratum, blind, template
from reference_review import digest
from stackoverflow_source import encoded
from topic_review import agreement, decode, validate


class TopicReviewTests(unittest.TestCase):
    def fixture(self):
        sources = {i: {'group': i, 'title': i + ' example', 'body_displayed': 'é🙂 exact code',
                       'body_html': '<p>é🙂 exact code</p>'} for i in ('a', 'b', 'c')}
        packet, selection = blind([(Pair('a', 'b', False, True), Stratum.SOURCE),
                                   (Pair('a', 'c', False, True), Stratum.POSITIVE)], sources)
        raw = encoded(packet)
        answer = template(packet, digest(raw))
        answer['reviewer'] = {'identity': 'synthetic-reviewer-A', 'human_attestation': True,
                             'case_reference_exposure': False, 'case_prediction_exposure': False}
        for row in answer['documents']:
            row.update(primary_subject='synthetic subject', secondary_subjects=[], sufficient_context=True,
                       source_evidence=[{'source_id': row['id'], 'field': 'body_displayed', 'quote': 'é🙂 exact code'}])
        cases = {c['id']: c for c in packet['cases']}
        for row in answer['cases']:
            row.update(shared_specific_subject='yes', useful_broader_subject='uncertain',
                       specific_subject='synthetic overlap', rationale='synthetic rationale',
                       source_evidence=[{'source_id': cases[row['id']][key], 'field': 'body_displayed',
                                         'quote': 'é🙂 exact code'} for key in ('left', 'right')])
        manifest = {'packet_sha256': digest(raw), 'selection': selection}
        return raw, answer, manifest

    def test_complete_unicode_evidence_and_reordered_rows(self):
        raw, answer, _ = self.fixture()
        answer['documents'].reverse()
        answer['cases'].reverse()
        self.assertEqual(validate(raw, answer), answer)

    def test_blank_template_is_not_completed_uncertainty(self):
        raw, _, _ = self.fixture()
        with self.assertRaises(ValueError):
            validate(raw, template(json.loads(raw), digest(raw)))

    def test_schema_identity_pin_and_declaration_rejection(self):
        raw, answer, _ = self.fixture()
        mutations = [lambda a: a.update(packet_sha256='changed'), lambda a: a.update(version=True),
                     lambda a: a.update(extra='ignored'), lambda a: a['documents'].pop(),
                     lambda a: a['cases'].append(a['cases'][0]),
                     lambda a: a['cases'][0].update(id='foreign'),
                     lambda a: a['reviewer'].update(human_attestation=False),
                     lambda a: a['reviewer'].update(case_prediction_exposure=0),
                     lambda a: a['reviewer'].update(identity=' ')]
        for mutate in mutations:
            candidate = copy.deepcopy(answer)
            mutate(candidate)
            with self.subTest(candidate=candidate['reviewer']):
                with self.assertRaises(ValueError):
                    validate(raw, candidate)

    def test_exact_evidence_scope_and_coverage(self):
        raw, answer, _ = self.fixture()
        mutations = [lambda a: a['documents'][0]['source_evidence'][0].update(quote='invented'),
                     lambda a: a['documents'][0]['source_evidence'][0].update(field='tags'),
                     lambda a: a['documents'][0]['source_evidence'][0].update(source_id=a['documents'][1]['id']),
                     lambda a: a['cases'][0]['source_evidence'].pop(),
                     lambda a: a['documents'][0].update(source_evidence=[])]
        for mutate in mutations:
            candidate = copy.deepcopy(answer)
            mutate(candidate)
            with self.assertRaises(ValueError):
                validate(raw, candidate)

    def test_subject_decisions_and_insufficient_context(self):
        raw, answer, _ = self.fixture()
        mutations = [lambda a: a['cases'][0].update(shared_specific_subject=None),
                     lambda a: a['cases'][0].update(specific_subject=''),
                     lambda a: a['cases'][0].update(broader_subject='unsupported'),
                     lambda a: a['documents'][0].update(primary_subject=None),
                     lambda a: a['documents'][0].update(secondary_subjects=None),
                     lambda a: a['documents'][0].update(secondary_subjects=['x', 'x']),
                     lambda a: a['documents'][0].update(sufficient_context=1)]
        for mutate in mutations:
            candidate = copy.deepcopy(answer)
            mutate(candidate)
            with self.assertRaises(ValueError):
                validate(raw, candidate)
        for row in answer['documents']:
            row.update(primary_subject=None, sufficient_context=False)
        with self.assertRaises(ValueError):
            validate(raw, answer)
        for row in answer['cases']:
            row.update(shared_specific_subject='uncertain', specific_subject=None)
        validate(raw, answer)

    def test_agreement_separates_strata_and_degenerate_kappa(self):
        raw, first, manifest = self.fixture()
        second = copy.deepcopy(first)
        second['reviewer']['identity'] = 'synthetic-reviewer-B'
        second['reviewer']['case_prediction_exposure'] = True
        primary = next(r['case'] for r in manifest['selection'] if r['stratum'] == Stratum.SOURCE)
        next(r for r in second['cases'] if r['id'] == primary).update(shared_specific_subject='no', specific_subject=None)
        report = agreement(raw, first, second, manifest)
        specific = report['strata'][Stratum.SOURCE]['shared_specific_subject']
        self.assertEqual(specific['count'], 1)
        self.assertEqual(specific['agreement'], 0)
        self.assertEqual(specific['kappa'], 0)
        self.assertEqual(specific['matrix'], [[0, 1, 0], [0, 0, 0], [0, 0, 0]])
        self.assertEqual(report['strata'][Stratum.SOURCE]['useful_broader_subject']['uncertain_either'], 1)
        self.assertIsNone(report['strata'][Stratum.SOURCE]['useful_broader_subject']['kappa'])
        self.assertIsNone(report['strata'][Stratum.NEGATIVE]['shared_specific_subject']['agreement'])
        self.assertTrue(report['reviewer_declarations'][1]['case_prediction_exposure'])

    def test_manifest_and_reviewer_aliases_rejected(self):
        raw, first, manifest = self.fixture()
        second = copy.deepcopy(first)
        with self.assertRaises(ValueError):
            agreement(raw, first, second, manifest)
        second['reviewer']['identity'] = 'synthetic-reviewer-B'
        for mutate in (lambda m: m.update(packet_sha256='changed'), lambda m: m['selection'].pop(),
                       lambda m: m['selection'][0].update(stratum='unknown'),
                       lambda m: m['selection'].append(m['selection'][0])):
            candidate = copy.deepcopy(manifest)
            mutate(candidate)
            with self.assertRaises(ValueError):
                agreement(raw, first, second, candidate)

    def test_nonconstant_perfect_agreement_has_kappa_one(self):
        raw, first, manifest = self.fixture()
        first['cases'][0].update(shared_specific_subject='no', specific_subject=None)
        second = copy.deepcopy(first)
        second['reviewer']['identity'] = 'synthetic-reviewer-B'
        for row in manifest['selection']:
            row['stratum'] = Stratum.SOURCE
        result = agreement(raw, first, second, manifest)['strata'][Stratum.SOURCE]['shared_specific_subject']
        self.assertEqual(result['count'], 2)
        self.assertEqual(result['agreement'], 1)
        self.assertEqual(result['kappa'], 1)

    def test_duplicate_json_and_nonfinite_rejected(self):
        for raw in ('{"id":1,"id":2}', '{"nested":{"id":1,"id":2}}', '{"v":NaN}', '{"v":Infinity}'):
            with self.assertRaises(ValueError):
                decode(raw)


if __name__ == '__main__':
    unittest.main()
