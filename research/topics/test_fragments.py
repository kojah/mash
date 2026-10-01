import unittest

import numpy as np

from fragment_metrics import evaluate
from fragments import CAP, Mode, bounds, split
from pair_review import Key, Pair, Relation
from source_spans import Kind
from test_unit_grouping import unit
from unit_grouping import Point


class FragmentTests(unittest.TestCase):
    def test_sentence_splits_preserve_quotes_code_technical_dots_and_negation(self):
        text = 'Do not change v1.2 or `x; y. Z`; Keep "no; yes. Fine" intact. Next task.'
        pieces = [text[a:b] for a, b in bounds(text, Kind.PROSE, Mode.SENTENCES)]
        self.assertEqual(''.join(pieces), text)
        self.assertEqual(len(pieces), 3)
        self.assertTrue(pieces[0].startswith('Do not'))
        self.assertIn('"no; yes. Fine"', pieces[1])
        self.assertEqual(bounds('Keep `unclosed. Code; next', Kind.PROSE, Mode.SENTENCES), ((0, 26),))

    def test_window_cap_and_complete_unicode_source_coverage(self):
        for size in (0, 1, 16, 17, 27, 100, 4096):
            text = ' '.join(f'α🙂{i}' for i in range(size))
            pieces = bounds(text, Kind.PROSE, Mode.WINDOWS)
            self.assertLessEqual(len(pieces), CAP)
            covered = set(i for a, b in pieces for i in range(a, b))
            self.assertEqual(covered, set(range(len(text))))
            self.assertTrue(all(a[1] >= b[0] for a, b in zip(pieces, pieces[1:])))

    def test_sentence_cap_merges_without_discarding_tail(self):
        text = ' '.join(f'Task {i}.' for i in range(40))
        pieces = bounds(text, Kind.PROSE, Mode.SENTENCES)
        self.assertEqual(len(pieces), CAP)
        self.assertEqual(''.join(text[a:b] for a, b in pieces), text)

    def test_code_and_headings_stay_intact_and_child_keys_do_not_collide(self):
        units = (unit(0, 'First task. Next task.', 0), unit(0, 'Third task. Last task.', 1))
        children = split(units, Mode.SENTENCES)
        self.assertEqual(len({f.unit.key for f in children}), 4)
        for child in children:
            parent = next(u for u in units if u.key == child.parent)
            self.assertEqual(child.unit.text, parent.text[child.unit.start:child.unit.end])
            self.assertNotIn('text', child.metadata())
        for kind in (Kind.CODE, Kind.HEADING):
            self.assertEqual(bounds('No split; Next. Last.', kind, Mode.SENTENCES), ((0, 21),))

    def test_multivector_recovers_distinct_aspects_and_never_groups_noise(self):
        units = (unit(0, 'cache'), unit(1, 'locks'), unit(2, 'cache. Locks.'))
        children = split(units, Mode.SENTENCES)
        vectors = (np.array([1., 0.]), np.array([0., 1.]), np.array([1., 0.]), np.array([0., 1.]))
        points = tuple(Point((c.unit.key,), v) for c, v in zip(children, vectors))
        pairs = (Pair(units[0].key, units[2].key, Relation.OVERLAP),
                 Pair(units[1].key, units[2].key, Relation.OVERLAP),
                 Pair(units[0].key, units[1].key, Relation.DISTINCT))
        result = evaluate(pairs, children, points, np.array([0, 1, 0, 1]))
        self.assertEqual(result['metrics']['native_coassigned']['true_positive'], 2)
        self.assertEqual(result['metrics']['native_coassigned']['false_positive'], 0)
        noise = evaluate(pairs, children, points, np.full(4, -1))
        self.assertEqual(noise['metrics']['native_coassigned']['true_positive'], 0)
        self.assertEqual(evaluate(pairs, children, points[:1], np.array([0]))['metrics']['native_coassigned']['abstentions'], 3)
