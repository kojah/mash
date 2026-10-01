import unittest

from passage_scope import intervals
from section_links import Allocation, sections, select


class SectionTests(unittest.TestCase):
    def test_generic_layout_links_stop_at_unrelated_prose_and_code(self):
        text = 'Results:\n\n- completed\n- done\n\nRemaining:\n\n- α\n- 🙂\n\nNext topic.\n\n- unrelated\n\n# Code\n\n```\n- not a child\n```\n\n- after code'
        units = intervals(text)['units']
        links = sections(text, units)
        self.assertEqual([(s.head, s.children) for s in links], [(0, (1, 2)), (3, (4, 5))])
        self.assertEqual(text[units[4]['start']:units[4]['end']], '- α')

    def test_table_framing_caps_and_emphasized_intros(self):
        text = '**Audit:**\n\n| a | b |\n|---|---|\n| α | β |\n| γ | δ |\n| ε | ζ |'
        units = intervals(text)['units']
        section, = sections(text, units, maximum_children=2)
        self.assertEqual((section.head, section.children, section.total_children), (0, (1, 2), 3))
        changed = [dict(u) for u in units]
        changed[0]['byte_end'] -= 1
        with self.assertRaises(ValueError): sections(text, changed)
        for cap in (0, True, 17):
            with self.assertRaises(ValueError): sections(text, units, cap)

    def test_fixed_budget_deduplication_and_source_boundaries(self):
        ranking = [{'source_id': s, 'index': i} for s, i in (('a', 0), ('a', 1), ('a', 2), ('b', 0))]
        links = {('a', 0): (('a', 1), ('a', 2))}
        chosen, visited = select(ranking, links, 3)
        self.assertEqual([(c['unit']['source_id'], c['unit']['index']) for c in chosen], [('a', 1), ('a', 2), ('b', 0)])
        self.assertEqual(visited, 4)
        self.assertEqual(chosen[0]['via'], {'source_id': 'a', 'index': 0})
        self.assertEqual(select(ranking, {}, 1)[0][0]['unit'], ranking[0])
        for invalid in ({('a', 0): (('b', 0),)}, {('a', 0): (('a', 0),)},
                        {('a', 0): (('a', 1), ('a', 1))}, {('missing', 0): (('a', 1),)}):
            with self.assertRaises(ValueError): select(ranking, invalid, 1)
        with self.assertRaises(ValueError): select(ranking * 2, links, 1)

    def test_one_child_keeps_another_ranked_hit_in_the_budget(self):
        ranking = [{'source_id': 'a', 'index': i} for i in (0, 4, 1, 2, 3)]
        links = {('a', 0): (('a', 1), ('a', 2), ('a', 3))}
        greedy, _ = select(ranking, links, 3)
        other, _ = select(ranking, links, 3, allocation=Allocation.ONE_CHILD)
        self.assertEqual([c['unit']['index'] for c in greedy], [1, 2, 3])
        self.assertEqual([c['unit']['index'] for c in other], [1, 4, 2])
        with self.assertRaises(ValueError): select(ranking, links, 3, allocation='unknown')
