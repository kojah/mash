import copy
import json
import unittest

from dialogue_budget import Control
from passage_scope import Relevance, Scope, capture, intervals, validate
from preceding_context import Message, Role, preceding
from reference_review import digest


class PassageScopeTests(unittest.TestCase):
    def fixture(self):
        text = 'é🙂 e\u0301\r\n\r\n- keep defaults\r\n- reject migration'
        context = preceding((Message('u', 0, Role.USER, -1, 'task', 'task'),
                             Message('a', 1, Role.ASSISTANT, 0, text, text),
                             Message('q', 2, Role.USER, 1, 'no migration', 'no migration')), 2, maximum_users=3)
        report = {'cases': [{'thread': 't', 'controls': {Control.RAW: {'contexts': [context]}}}]}
        rows = capture(report, [{'thread': 't', 'anchor': 2, 'source': 1}])
        manifest = json.dumps({'schema_version': 1, 'scope': 'development-source-passage-scope-diagnostic', 'rows': rows}).encode()
        row = rows[0]
        annotation = {'schema_version': 1, 'scope': 'development-two-axis-passage-review',
                      'annotator': 'synthetic fixture, not human evidence', 'manifest_sha256': digest(manifest),
                      'rows': [{k: row[k] for k in ('thread', 'anchor', 'source')}]}
        annotation['rows'][0].update(rationale='rejection remains topical', units=[
            {**s, 'relevance': Relevance.TOPICAL, 'requested_scope': Scope.EXCLUDED} for s in row['units']])
        return text, report, rows, manifest, annotation

    def test_dual_offsets_and_complete_source_accounting(self):
        for text in ('', '  ', 'é🙂 e\u0301\r\n\r\n- hi', '| h | z |\n|---|---|\n| é | 🙂 |\n', '```\n🙂\n```'):
            units = intervals(text)
            chunks = sorted([*units['units'], *units['excluded_gaps']], key=lambda s: s['start'])
            cursor = byte_cursor = 0
            for s in chunks:
                self.assertEqual((s['start'], s['byte_start']), (cursor, byte_cursor))
                raw = text[s['start']:s['end']].encode()
                self.assertEqual(raw, text.encode()[s['byte_start']:s['byte_end']])
                self.assertEqual(digest(raw), s['text_sha256'])
                cursor, byte_cursor = s['end'], s['byte_end']
            self.assertEqual((cursor, byte_cursor), (len(text), len(text.encode())))

    def test_topical_rejection_is_not_irrelevance(self):
        _, _, _, manifest, annotation = self.fixture()
        result = validate(annotation, manifest)
        self.assertTrue(all(j.relevance == Relevance.TOPICAL and j.scope == Scope.EXCLUDED for j in result))
        for mutate in ('missing', 'duplicate', 'hash', 'bytes', 'order', 'axis', 'schema'):
            changed = copy.deepcopy(annotation)
            row = changed['rows'][0]
            if mutate == 'missing': row['units'].pop()
            elif mutate == 'duplicate': changed['rows'].append(copy.deepcopy(row))
            elif mutate == 'hash': row['units'][0]['text_sha256'] = 'bad'
            elif mutate == 'bytes': row['units'][0]['byte_end'] -= 1
            elif mutate == 'order': row['units'].reverse()
            elif mutate == 'axis': row['units'][0]['requested_scope'] = Scope.NA
            else: row['units'][0]['label'] = 'oracle model input'
            with self.assertRaises(ValueError, msg=mutate):
                validate(changed, manifest)

    def test_only_intact_causal_displayed_sources_and_explicit_pairs(self):
        _, report, _, _, _ = self.fixture()
        pair = {'thread': 't', 'anchor': 2, 'source': 1}
        for mutate in ('edited', 'omitted', 'role', 'duplicate', 'future'):
            changed, pairs = copy.deepcopy(report), [pair]
            entry = changed['cases'][0]['controls'][Control.RAW]['contexts'][0]['display'][-1]
            if mutate == 'edited': entry['text'] += 'edit'
            elif mutate == 'omitted': entry['display_status'] = 'omitted'
            elif mutate == 'role': entry['role'] = 'user'
            elif mutate == 'duplicate': pairs = [pair, pair]
            else: pairs = [{**pair, 'source': 2}]
            with self.assertRaises(ValueError, msg=mutate):
                capture(changed, pairs)
