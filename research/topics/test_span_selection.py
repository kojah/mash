import copy
from dataclasses import asdict
import json
from types import SimpleNamespace
import unittest

import numpy as np

from reference_review import digest
from source_spans import structural_spans
from span_selection import Query, Relation, query_inputs, rank_spans, review_pilot, selection_counts


class SpanSelectionTests(unittest.TestCase):
    def setUp(self):
        self.text = "A useful subject\n\nA rejected proposal"
        self.identity = {"id": "a", "ordinal": 1, "role": "assistant",
                         "searchable_sha256": digest(self.text.encode())}
        anchor = {"id": "u", "ordinal": 2, "role": "user", "searchable_sha256": digest(b"no")}
        self.context = {"dataset_fingerprint": "d", "cases": [{"thread": "t", "contexts": [
            {"anchor": anchor, "traversed": [self.identity],
             "display": [{**self.identity, "text": self.text, "display_status": "included"}]}]}]}
        spans = [{**asdict(s), "text_sha256": digest(self.text[s.start:s.end].encode()),
                  "source_bytes": len(self.text[s.start:s.end].encode())} for s in structural_spans(self.text)]
        self.spans = json.dumps({"dataset_fingerprint": "d", "rows": [
            {"thread": "t", "identity": self.identity, "spans": spans}]}).encode()
        self.annotation = {"schema_version": 1, "scope": "development-span-selection-pilot",
                           "annotator": "source reviewer, exposed development cases", "spans_sha256": digest(self.spans),
                           "contexts_sha256": digest(json.dumps(self.context).encode()),
                           "cases": [{"thread": "t", "ordinal": 2, "anchor": anchor, "assistant": self.identity,
                                      "judgments": [{"index": i, "text_sha256": s["text_sha256"], "relation": relation}
                                                    for i, (s, relation) in enumerate(zip(spans, ["topic-support", "rejected-proposal"]))]}]}

    def check(self, annotation=None, context=None):
        return review_pilot(annotation or self.annotation, self.spans, json.dumps(context or self.context).encode())

    def test_complete_pinned_scalar_judgments(self):
        pilot, = self.check()
        self.assertEqual(pilot.relations, (Relation.SUPPORT, Relation.REJECTED))

    def test_incomplete_reordered_unknown_or_added_model_text_rejected(self):
        for mode in ("missing", "reordered", "unknown", "text", "duplicate"):
            annotation = copy.deepcopy(self.annotation)
            case = annotation["cases"][0]
            if mode == "missing":
                case["judgments"].pop()
            elif mode == "reordered":
                case["judgments"].reverse()
            elif mode == "unknown":
                case["judgments"][0]["relation"] = "probably"
            elif mode == "text":
                case["judgments"][0]["summary"] = "reviewer-generated input"
            else:
                annotation["cases"].append(copy.deepcopy(case))
            with self.assertRaises(ValueError):
                self.check(annotation)

    def test_repinning_does_not_accept_modified_source_or_noncausal_identity(self):
        for mode in ("source", "causal"):
            context = copy.deepcopy(self.context)
            entry = context["cases"][0]["contexts"][0]
            if mode == "source":
                entry["display"][0]["text"] = "different source"
            else:
                entry["traversed"] = []
            annotation = copy.deepcopy(self.annotation)
            annotation["contexts_sha256"] = digest(json.dumps(context).encode())
            with self.assertRaisesRegex(ValueError, "intact causal"):
                self.check(annotation, context)

    def test_cosine_ties_preserve_source_order_and_overflow_does_not_partially_encode(self):
        calls = []
        def encode(text):
            calls.append(text)
            return np.asarray([1., 0.])
        encoder = SimpleNamespace(tokenizer=SimpleNamespace(encode=lambda t: SimpleNamespace(ids=[0] * len(t))), encode=encode)
        self.assertEqual(rank_spans(("ok",), ("aa", "bb"), encoder), ((0, 1.), (1, 1.)))
        calls.clear()
        with self.assertRaisesRegex(ValueError, "no truncation"):
            rank_spans(("ok",), ("aa", "x" * 513), encoder)
        self.assertEqual(calls, [])

    def test_previous_user_queries_do_not_leap_over_missing_or_edited_source(self):
        context = copy.deepcopy(self.context["cases"][0]["contexts"][0])
        identity = {"id": "old", "ordinal": 0, "role": "user", "searchable_sha256": digest(b"subject")}
        entry = {**identity, "text": "subject", "display_status": "included"}
        context["display"].insert(0, entry)
        context["traversed"].append(identity)
        self.assertEqual(query_inputs("no", context, Query.PREVIOUS), ("subject",))
        self.assertEqual(query_inputs("no", context, Query.CHAIN), ("subject", "no"))
        entry["display_status"] = "omitted-size"
        self.assertEqual(query_inputs("no", context, Query.PREVIOUS), ())
        self.assertEqual(query_inputs("no", context, Query.CHAIN), ())
        self.assertEqual(query_inputs("no", context, Query.CURRENT), ("no",))
        entry["display_status"] = "included"
        entry["text"] = "edited"
        self.assertEqual(query_inputs("no", context, Query.PREVIOUS), ())

    def test_mixed_rejection_is_not_counted_as_safe_topic_support(self):
        counts = selection_counts((0, 1, 2), (Relation.SUPPORT, Relation.MIXED, Relation.METADATA))
        self.assertEqual(counts["topic_support_hits"], 1)
        self.assertEqual(counts["mixed_or_rejected"], 1)
        self.assertEqual(counts["metadata"], 1)
