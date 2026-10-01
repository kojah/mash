import copy
import json
from pathlib import Path
import tempfile
import unittest

from stackoverflow_source import FROM, PINS, load, parse, split


def pages():
    rows = [{'question_id': i, 'creation_date': FROM + i, 'title': f'Question {i} &lt;T&gt;',
             'body': f'<p>Original text {i}</p><pre><code>a &lt; b</code></pre>',
             'tags': [f'tag-{i % 7}', 'language'], 'link': f'https://stackoverflow.com/questions/{i}/question',
             'content_license': 'CC BY-SA 4.0'} for i in range(1, 201)]
    return [{'items': rows[:100], 'has_more': True, 'quota_max': 300, 'quota_remaining': 299},
            {'items': rows[100:], 'has_more': True, 'quota_max': 300, 'quota_remaining': 298}]


class SourceTests(unittest.TestCase):
    def test_source_text_and_tags_are_separate_and_split_is_label_independent(self):
        source = pages()
        corpus = parse(source)
        self.assertEqual(corpus.inputs[0].title, 'Question 1 <T>')
        self.assertEqual(corpus.inputs[0].body_html, source[0]['items'][0]['body'])
        self.assertFalse(hasattr(corpus.inputs[0], 'tags'))
        changed = copy.deepcopy(source)
        for page in changed:
            for row in page['items']:
                row['tags'] = ['completely-different-reference']
        other = parse(changed)
        self.assertEqual(corpus.inputs, other.inputs)
        self.assertEqual(split(corpus), split(other))
        self.assertEqual([len(p) for p in split(corpus)], [64, 64, 72])
        self.assertEqual(len({q.group for phase in split(corpus) for q in phase}), 200)

    def test_missing_license_is_explicit_not_invented(self):
        source = pages()
        del source[0]['items'][0]['content_license']
        self.assertIsNone(parse(source).references[0].content_license)

    def test_bad_known_fields_fail_without_skipping_records(self):
        for field, bad in [('question_id', True), ('creation_date', FROM - 1), ('title', ''),
                           ('body', None), ('tags', ['same', 'same']), ('tags', ['two words']),
                           ('tags', []), ('tags', ['x'] * 6), ('link', 'https://example.com/questions/1/a'),
                           ('content_license', False)]:
            with self.subTest(field=field, value=bad):
                source = pages()
                source[0]['items'][0][field] = bad
                with self.assertRaises(ValueError):
                    parse(source)

    def test_pagination_quota_errors_duplicates_and_order_are_rejected(self):
        for change in [lambda p: p[0].update(has_more=1), lambda p: p[0].update(has_more=False),
                       lambda p: p[0].update(quota_remaining=True), lambda p: p[1].update(error_id=502),
                       lambda p: p[1]['items'].__setitem__(0, p[0]['items'][0]),
                       lambda p: p[0]['items'].reverse(), lambda p: p[0]['items'].pop()]:
            source = pages()
            change(source)
            with self.assertRaises(ValueError):
                parse(source)

    def test_duplicate_content_groups_stay_together_and_insufficient_groups_fail(self):
        source = pages()
        for row in source[1]['items']:
            row['title'], row['body'] = source[0]['items'][0]['title'], source[0]['items'][0]['body']
        corpus = parse(source)
        with self.assertRaises(ValueError):
            split(corpus)

    def test_source_hash_checks_precede_decode(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            for page in PINS:
                (root / page).write_bytes(b'invalid json, invalid source hash')
            with self.assertRaisesRegex(ValueError, 'source page changed'):
                load(root)

    def test_reference_serialization_does_not_affect_inputs(self):
        original = parse(pages())
        roundtrip = parse(json.loads(json.dumps(pages())))
        self.assertEqual(original, roundtrip)


if __name__ == '__main__':
    unittest.main()
