from copy import deepcopy
import json
import unittest

from feedback_payload import Status, select
from packet_review import Origin, validate
from reference_review import digest


class PacketReviewTests(unittest.TestCase):
    def fixture(self):
        text = 'historical wrapper\n<submitted_feedback>α🙂 task</submitted_feedback>\nfooter'
        turn = {"id": "id", "ordinal": 3, "text_sha256": digest(text.encode()), "text": text, "display_redacted": False}
        case = {"thread": "t", "source_sha256": "h", "source_group": "g", "turns": [turn]}
        packets = json.dumps({"cases": [case]}).encode()
        annotation = {k: turn[k] for k in ("id", "ordinal", "text_sha256")}
        annotation.update(kind=Origin.WRAPPED, rationale="Source review", facets=["task"])
        review = {"schema_version": 1, "scope": "development-packet-origin-review", "packets_sha256": digest(packets),
                  "annotator": "exposed", "facet_granularity": "specific task", "cases": [{**case, "turns": [annotation]}]}
        return packets, review

    def test_source_review_requires_every_pinned_identity_and_exposure(self):
        packets, review = self.fixture()
        self.assertEqual(validate(packets, json.dumps(review).encode())[0].facets, frozenset({"task"}))
        for field in ("scope", "ordinal", "facets", "kind", "group", "turns"):
            changed = deepcopy(review)
            if field == "scope":
                changed["packets_sha256"] = "changed"
            elif field == "group":
                changed["cases"][0]["source_group"] = "other"
            elif field == "turns":
                changed["cases"][0]["turns"] = []
            else:
                changed["cases"][0]["turns"][0][field] = {"ordinal": 4, "facets": ["task", "task"], "kind": "protocol"}[field]
            with self.assertRaises(ValueError):
                validate(packets, json.dumps(changed).encode())

    def test_payload_boundaries_preserve_original_unicode_text_and_reject_ambiguity(self):
        text = 'α🙂wrapper\n<submitted_feedback>α🙂 do not rewrite</submitted_feedback>footer'
        selected = select(text)
        self.assertEqual(selected.status, Status.PAYLOAD)
        self.assertEqual(text[selected.start:selected.end], 'α🙂 do not rewrite')
        self.assertNotEqual(selected.start, len(text[:selected.start].encode()))
        self.assertEqual(select('ordinary question').status, Status.ABSENT)
        for text in ('<submitted_feedback>unclosed', '</submitted_feedback>early<submitted_feedback>late',
                     '<submitted_feedback> </submitted_feedback>',
                     '<submitted_feedback>first</submitted_feedback><submitted_feedback>second</submitted_feedback>',
                     '<submitted_feedback><submitted_feedback>nested</submitted_feedback></submitted_feedback>'):
            self.assertEqual(select(text).status, Status.MALFORMED)
