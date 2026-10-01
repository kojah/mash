from types import SimpleNamespace
import unittest

from packet_review import Judgment, Origin
from payload_experiment import matched_sources


class PayloadWindowTests(unittest.TestCase):
    def test_short_payload_does_not_gain_unmatched_scope_from_long_wrapper(self):
        sources = {('t', 0): {'text': 'long wrapper<submitted_feedback>fits</submitted_feedback>'},
                   ('t', 1): {'text': '<submitted_feedback>also fits</submitted_feedback>'}}
        selected = [Judgment('t', i, Origin.WRAPPED, frozenset({'task'})) for i in (0, 1)]
        class Tokenizer:
            def encode(self, text):
                return SimpleNamespace(ids=range(513 if text.startswith('long') else 10))
        self.assertEqual(matched_sources(sources, selected, Tokenizer()), {('t', 1)})
        sources[('t', 0)]['text'] = '<submitted_feedback>unclosed'
        with self.assertRaises(ValueError):
            matched_sources(sources, selected, Tokenizer())
