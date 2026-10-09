import copy
import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from search_evaluation import Split, evaluate


def fixture():
    query = {'id': 'q1', 'text': 'needle', 'category': 'semantic',
             'split': 'held-out', 'intent': 'related-discussion', 'scope': 'local',
             'judgments': [{'provider': 'codex', 'chat_id': identity, 'grade': grade}
                           for identity, grade in [('a', 2), ('b', 1), ('c', 0)]]}
    response = {'query': 'needle', 'intent': 'related-discussion', 'unit': 'chats',
                'diagnostics': {'requested_scope': 'local', 'limit': 10,
                                'completed_sources': ['local'], 'shared_fallback': False,
                                'shared_context_incomplete': False},
                'results': [{'provider': 'codex', 'chat_id': identity}
                            for identity in ['c', 'a', 'b']]}
    return {'version': 1, 'queries': [query]}, {'version': 1, 'queries': [{'id': 'q1', 'response': response}]}


class EvaluationTests(unittest.TestCase):
    def test_known_ranking_and_short_list_denominators(self):
        judgments, captures = fixture()
        report = evaluate(judgments, captures, Split.HELD_OUT)
        metrics = report['queries'][0]['metrics']
        self.assertEqual(metrics['mrr_at_10'], .5)
        self.assertEqual(metrics['precision_at_5'], .4)
        self.assertEqual(metrics['recall_at_10'], 1)
        expected = (3 / math.log2(3) + 1 / math.log2(4)) / (3 + 1 / math.log2(3))
        self.assertAlmostEqual(metrics['ndcg_at_5'], expected)
        self.assertEqual(report['categories']['semantic']['positive_queries'], 1)

    def test_unjudged_threads_prevent_aggregate_scores(self):
        judgments, captures = fixture()
        captures['queries'][0]['response']['results'][0]['chat_id'] = 'unknown'
        report = evaluate(judgments, captures, Split.HELD_OUT)
        self.assertFalse(report['complete'])
        self.assertIsNone(report['categories'])
        self.assertIsNone(report['queries'][0]['metrics'])
        self.assertEqual(report['queries'][0]['unjudged'][0]['chat_id'], 'unknown')

    def test_empty_positive_and_negative_queries_are_distinct(self):
        judgments, captures = fixture()
        captures['queries'][0]['response']['results'] = []
        positive = evaluate(judgments, captures, Split.HELD_OUT)
        self.assertEqual(positive['categories']['all']['recall_at_10'], 0)
        self.assertEqual(positive['categories']['all']['ndcg_at_10'], 0)
        judgments['queries'][0]['category'] = 'negative'
        for judgment in judgments['queries'][0]['judgments']:
            judgment['grade'] = 0
        negative = evaluate(judgments, captures, Split.HELD_OUT)
        self.assertEqual(negative['categories']['all']['abstention_rate'], 1)
        self.assertNotIn('recall_at_10', negative['categories']['all'])
        captures['queries'][0]['response']['results'] = [{'provider': 'codex', 'chat_id': 'c'}]
        self.assertEqual(evaluate(judgments, captures, Split.HELD_OUT)
                         ['categories']['all']['abstention_rate'], 0)

    def test_query_transport_and_degradation_mismatches_reject(self):
        judgments, original = fixture()
        changes = [('query', 'other'), ('intent', 'exact-mention'), ('unit', 'messages'),
                   ('shared_error', 'unavailable'), ('shared_context_error', 'timeout')]
        for field, value in changes:
            with self.subTest(field=field):
                captures = copy.deepcopy(original)
                captures['queries'][0]['response'][field] = value
                with self.assertRaises(ValueError):
                    evaluate(judgments, captures, Split.HELD_OUT)
        changes = [('requested_scope', 'combined'), ('limit', 5), ('completed_sources', []),
                   ('shared_fallback', True), ('shared_context_incomplete', True)]
        for field, value in changes:
            with self.subTest(field=field):
                captures = copy.deepcopy(original)
                captures['queries'][0]['response']['diagnostics'][field] = value
                with self.assertRaises(ValueError):
                    evaluate(judgments, captures, Split.HELD_OUT)

    def test_duplicate_ranked_threads_cannot_inflate_recall(self):
        judgments, captures = fixture()
        captures['queries'][0]['response']['results'].append(
            {'provider': 'codex', 'chat_id': 'a', 'node_id': 'another-copy'})
        with self.assertRaises(ValueError):
            evaluate(judgments, captures, Split.HELD_OUT)

    def test_identity_is_provider_and_thread(self):
        judgments, captures = fixture()
        captures['queries'][0]['response']['results'][0]['provider'] = 'claude-code'
        self.assertFalse(evaluate(judgments, captures, Split.HELD_OUT)['complete'])

    def test_malformed_judgments_and_query_sets_reject(self):
        original, captures = fixture()
        for grade in (True, -1, 3, '2'):
            judgments = copy.deepcopy(original)
            judgments['queries'][0]['judgments'][0]['grade'] = grade
            with self.assertRaises(ValueError):
                evaluate(judgments, captures, Split.HELD_OUT)
        for field in ('queries', 'judgments'):
            judgments = copy.deepcopy(original)
            values = judgments['queries'] if field == 'queries' else judgments['queries'][0]['judgments']
            values.append(copy.deepcopy(values[0]))
            with self.assertRaises(ValueError):
                evaluate(judgments, captures, Split.HELD_OUT)
        for saved in ([], captures['queries'] * 2, [{'id': 'wrong', 'response': captures['queries'][0]['response']}]):
            with self.assertRaises(ValueError):
                evaluate(original, {'version': 1, 'queries': saved}, Split.HELD_OUT)

    def test_split_must_be_requested_and_nonempty(self):
        judgments, captures = fixture()
        with self.assertRaises(ValueError):
            evaluate(judgments, captures, Split.DEVELOPMENT)
        judgments['queries'][0]['split'] = 'development'
        self.assertTrue(evaluate(judgments, captures, Split.DEVELOPMENT)['complete'])

    def test_cli_reports_hashes_and_incomplete_exit_without_scoring(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            judgments = root / 'judgments.toml'
            judgments.write_text('''version=1
[[queries]]
id="q1"
text="needle"
category="semantic"
split="held-out"
intent="related-discussion"
scope="local"
[[queries.judgments]]
provider="codex"
chat_id="a"
grade=2
''')
            _, captures = fixture()
            response = captures['queries'][0]['response']
            capture = root / 'captures.json'
            output = root / 'report.json'
            command = [sys.executable, str(Path(__file__).with_name('search_evaluation.py')),
                       '--judgments', str(judgments), '--captures', str(capture),
                       '--output', str(output)]
            capture.write_text(json.dumps(captures))
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2, result.stderr)
            report = json.loads(output.read_text())
            self.assertIsNone(report['categories'])
            self.assertEqual(set(report['input_sha256']), {'judgments', 'captures', 'scorer'})
            self.assertTrue(all(len(value) == 64 for value in report['input_sha256'].values()))
            response['results'] = [{'provider': 'codex', 'chat_id': 'a'}]
            capture.write_text(json.dumps(captures))
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(output.read_text())['categories']['all']['ndcg_at_10'], 1)


if __name__ == '__main__':
    unittest.main()
