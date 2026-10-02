import unittest

from token_boundaries import audit, summarize


class TokenBoundaryTests(unittest.TestCase):
    def test_attached_punctuation_loss_and_hash_split_remain_visible(self):
        rows = audit('C++ C# .NET /usr/lib --flag foo#bar')
        self.assertEqual([r['surface'] for r in rows], ['C++', 'C#', '.NET', '/usr/lib', '--flag', 'foo#bar'])
        self.assertEqual([[m['surface'] for m in r['matches']] for r in rows],
                         [['C'], ['C'], ['NET'], ['usr/lib'], ['flag'], ['foo', 'bar']])
        self.assertFalse(any(r['exact_single_match'] for r in rows))

    def test_source_offsets_unicode_marks_and_prefixes(self):
        text = '🙂 é e\u0301 C++\r\nnext'
        rows = audit(text, 7, 11)
        for row in rows:
            self.assertEqual(text[row['start']-7:row['end']-7], row['surface'])
            self.assertEqual(text.encode()[row['byte_start']-11:row['byte_end']-11].decode(), row['surface'])
            for match in row['matches']:
                self.assertEqual(text[match['start']-7:match['end']-7], match['surface'])
                self.assertEqual(text.encode()[match['byte_start']-11:match['byte_end']-11].decode(), match['surface'])
        decomposed = next(r for r in rows if r['surface'] == 'e\u0301')
        self.assertEqual(decomposed['matches'][0]['surface'], 'e')
        self.assertFalse(decomposed['exact_single_match'])

    def test_eligibility_stages_do_not_repair_stopwords(self):
        rows = audit('system alpha beta gamma', vocabulary=['system', 'alpha', 'beta'], graph_words=['system', 'alpha'])
        self.assertEqual([r['matches'][0]['state'] for r in rows],
                         ['stopword', 'graph-retained', 'outside-graph-vocabulary', 'outside-original-vocabulary'])

    def test_collision_summary_preserves_exact_forms_and_punctuation_noise(self):
        records = [{'spans': audit('c C++ C# word word. foo#bar ... ---')}]
        summary = summarize(records)
        self.assertEqual(summary['normalized_terms_with_multiple_maximal_forms']['c'], ['c', 'c#', 'c++'])
        self.assertEqual(summary['normalized_terms_with_multiple_maximal_forms']['word'], ['word', 'word.'])
        self.assertEqual(summary['counts']['multi_match_runs'], 1)
        self.assertEqual(summary['counts']['plus_hash_runs'], 3)
        self.assertEqual(summary['counts']['original_matches'], 7)

    def test_normalization_is_legacy_lower_not_casefold(self):
        self.assertEqual(audit('Straße')[0]['matches'][0]['term'], 'straße')
        self.assertEqual(audit('İ')[0]['matches'][0]['term'], 'i\u0307')

    def test_invalid_source_offsets_and_bounds(self):
        for text, offset, byte_offset in [(None, 0, 0), ('x', -1, 0), ('x', 0, True), ('x'*65537, 0, 0)]:
            with self.assertRaises(ValueError):
                audit(text, offset, byte_offset)


if __name__ == '__main__':
    unittest.main()
