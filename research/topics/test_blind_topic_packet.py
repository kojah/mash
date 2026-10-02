import itertools
import json
from html.parser import HTMLParser
import unittest

from blind_topic_packet import Pair, Stratum, blind, page, select, template


class BlindPacketTests(unittest.TestCase):
    def candidates(self):
        groups = {str(i): 'source-group-' + str(i) for i in range(12)}
        pairs = [Pair(a, b, n % 3 == 0, n % 3 == 1) for n, (a, b) in enumerate(itertools.combinations(groups, 2))]
        return pairs, groups

    def test_primary_sample_independent_of_outcomes_and_input_order(self):
        pairs, groups = self.candidates()
        first = select(pairs, groups, primary=10, diagnostic=0)
        changed = [Pair(p.left, p.right, not p.tag_overlap, not p.prediction) for p in reversed(pairs)]
        second = select(changed, groups, primary=10, diagnostic=0)
        self.assertEqual([(p.left, p.right) for p, _ in first], [(p.left, p.right) for p, _ in second])
        self.assertTrue(all(s == Stratum.SOURCE for _, s in first))

    def test_strata_disjoint_and_diagnostic_shortfalls_not_refilled(self):
        pairs, groups = self.candidates()
        chosen = select(pairs, groups, primary=10, diagnostic=4)
        self.assertEqual(len(chosen), 18)
        self.assertEqual(len({tuple(sorted((p.left, p.right))) for p, _ in chosen}), 18)
        for pair, stratum in chosen:
            if stratum == Stratum.POSITIVE:
                self.assertTrue(pair.prediction and not pair.tag_overlap)
            elif stratum == Stratum.NEGATIVE:
                self.assertTrue(pair.tag_overlap and not pair.prediction)
        short = select([Pair('a', 'b', False, False)], {'a': 'a', 'b': 'b'}, primary=1, diagnostic=8)
        self.assertEqual(len(short), 1)

    def test_opaque_packet_preserves_source_and_hides_selection_metadata(self):
        sources = {i: {'group': i, 'title': i + ' title', 'body_displayed': 'é\r\n🙂 data', 'body_html': '<p>é data</p>',
                       'tags': ['hidden'], 'prediction': True} for i in ('canonical-a', 'canonical-b')}
        chosen = [(Pair('canonical-a', 'canonical-b', True, False), Stratum.NEGATIVE)]
        packet, manifest = blind(chosen, sources)
        self.assertEqual(set(packet), {'version', 'scope', 'rubric_version', 'documents', 'cases'})
        for document in packet['documents']:
            self.assertEqual(set(document), {'id', 'title', 'body_displayed', 'body_html'})
            self.assertEqual(document['body_displayed'], 'é\r\n🙂 data')
            self.assertTrue(document['id'].startswith('source-'))
            self.assertNotIn('canonical', document['id'])
        for case in packet['cases']:
            self.assertEqual(set(case), {'id', 'left', 'right'})
        self.assertTrue(manifest[0]['tag_overlap'])
        self.assertEqual(packet, json.loads(json.dumps(packet)))

    def test_template_nulls_are_unanswered_not_uncertain_annotations(self):
        sources = {i: {'group': i, 'title': i, 'body_displayed': i, 'body_html': i} for i in ('a', 'b')}
        packet, _ = blind([(Pair('a', 'b', False, True), Stratum.POSITIVE)], sources)
        answers = template(packet, 'packet-pin')
        self.assertIsNone(answers['reviewer']['human_attestation'])
        self.assertIsNone(answers['cases'][0]['shared_specific_subject'])
        self.assertIsNone(answers['cases'][0]['useful_broader_subject'])
        self.assertIsNone(answers['documents'][0]['secondary_subjects'])

    def test_source_html_is_inert_and_rubric_is_escaped(self):
        malicious = '<script>alert(1)</script><img src="https://example.org/x" onerror="bad()">'
        sources = {i: {'group': i, 'title': malicious, 'body_displayed': malicious, 'body_html': malicious} for i in ('a', 'b')}
        packet, _ = blind([(Pair('a', 'b', False, False), Stratum.SOURCE)], sources)
        output = page(packet, malicious)
        class Tags(HTMLParser):
            def __init__(self):
                super().__init__()
                self.tags = []
            def handle_starttag(self, tag, attrs):
                self.tags.append((tag, attrs))
        parser = Tags()
        parser.feed(output)
        self.assertFalse(any(tag in {'script', 'img', 'iframe', 'object'} for tag, _ in parser.tags))
        self.assertFalse(any(key in {'src', 'href', 'onerror'} for _, attrs in parser.tags for key, _ in attrs))
        self.assertIn('&lt;script&gt;', output)
        self.assertIn('Content-Security-Policy', output)

    def test_bad_pairs_seed_counts_aliases_and_collisions_rejected(self):
        for pairs, groups in [([Pair('a', 'a', False, False)], {'a': 'a'}),
                              ([Pair('a', 'missing', False, False)], {'a': 'a'}),
                              ([Pair('a', 'b', 1, False)], {'a': 'a', 'b': 'b'}),
                              ([Pair('a', 'b', False, False), Pair('b', 'a', False, False)], {'a': 'a', 'b': 'b'}),
                              ([Pair('a', 'b', False, False)], {'a': 'same', 'b': 'same'})]:
            with self.assertRaises(ValueError):
                select(pairs, groups)
        pairs, groups = self.candidates()
        for values in ({'seed': ''}, {'primary': True}, {'diagnostic': -1}, {'primary': 65}):
            with self.assertRaises(ValueError):
                select(pairs, groups, **values)


if __name__ == '__main__':
    unittest.main()
