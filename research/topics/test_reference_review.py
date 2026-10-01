import copy
import json
import unittest

from preceding_context import Message, Role, preceding
from reference_review import Resolution, digest, validate


class ReferenceReviewTests(unittest.TestCase):
    def setUp(self):
        context = preceding((Message("u", 0, Role.USER, -1, "task", "task"),
                             Message("a", 1, Role.ASSISTANT, 0, "proposal", "proposal"),
                             Message("yes", 2, Role.USER, 1, "yes", "yes")), 2)
        self.context = {"cases": [{"thread": "t", "source_sha256": "source", "contexts": [context]}]}
        review = {"cases": [{"thread": "t", "source_sha256": "source", "admission_judgments": [
            {"message": "yes", "ordinal": 2, "kind": "referential", "text_sha256": digest(b"yes")}
        ]}]}
        self.review_bytes = json.dumps(review).encode()
        evidence = context["traversed"][0]
        self.annotation = {"schema_version": 1, "scope": "bounded-preceding-reference-review",
                           "annotator": "exploratory reviewer; prior prediction exposure",
                           "context_sha256": digest(json.dumps(self.context).encode()),
                           "source_review_sha256": digest(self.review_bytes),
                           "judgments": [{"thread": "t", "ordinal": 2, "anchor": context["anchor"],
                                          "context_sha256": context["context_sha256"],
                                          "resolution": "resolved-task", "evidence": [evidence],
                                          "rationale": "accepts the preceding proposal"}]}

    def check(self, context=None, annotation=None):
        return validate(json.dumps(context or self.context).encode(), self.review_bytes,
                        annotation or self.annotation)

    def test_resolved_reference_preserves_evidence_and_resolution(self):
        judgment, = self.check()
        self.assertEqual(judgment.resolution, Resolution.TASK)
        self.assertEqual(judgment.evidence_ordinals, (1,))

    def test_source_change_rejected(self):
        changed = copy.deepcopy(self.context)
        changed["cases"][0]["source_sha256"] = "edited"
        with self.assertRaisesRegex(ValueError, "source changed"):
            self.check(context=changed)

    def test_missing_duplicate_and_unexpected_anchors_rejected(self):
        for replacement in ([], self.annotation["judgments"] * 2):
            changed = copy.deepcopy(self.annotation)
            changed["judgments"] = replacement
            with self.assertRaises(ValueError):
                self.check(annotation=changed)
        changed = copy.deepcopy(self.annotation)
        changed["judgments"][0]["ordinal"] = 3
        with self.assertRaises(ValueError):
            self.check(annotation=changed)

    def test_outside_context_or_tampered_evidence_rejected(self):
        for field, value in (("ordinal", 3), ("id", "sibling"), ("searchable_sha256", "wrong")):
            changed = copy.deepcopy(self.annotation)
            changed["judgments"][0]["evidence"][0][field] = value
            with self.assertRaisesRegex(ValueError, "causal displayed"):
                self.check(annotation=changed)

    def test_omitted_or_redacted_evidence_rejected_even_with_new_packet_pin(self):
        for status in ("omitted-size", "redacted"):
            context = copy.deepcopy(self.context)
            context["cases"][0]["contexts"][0]["display"][1]["display_status"] = status
            annotation = copy.deepcopy(self.annotation)
            annotation["context_sha256"] = digest(json.dumps(context).encode())
            with self.assertRaisesRegex(ValueError, "causal displayed"):
                self.check(context, annotation)

    def test_resolved_requires_evidence_but_abstention_does_not(self):
        changed = copy.deepcopy(self.annotation)
        changed["judgments"][0]["evidence"] = []
        with self.assertRaisesRegex(ValueError, "require source evidence"):
            self.check(annotation=changed)
        changed["judgments"][0]["resolution"] = "unresolved-reference"
        self.assertEqual(self.check(annotation=changed)[0].resolution, Resolution.UNRESOLVED)

    def test_repinning_packet_does_not_accept_edited_display_or_anchor(self):
        for target in ("display", "anchor"):
            context = copy.deepcopy(self.context)
            captured = context["cases"][0]["contexts"][0]
            if target == "display":
                captured["display"][1]["text"] = "unrelated edited proposal"
            else:
                captured["anchor"]["id"] = "wrong-user"
            annotation = copy.deepcopy(self.annotation)
            annotation["context_sha256"] = digest(json.dumps(context).encode())
            with self.assertRaises(ValueError):
                self.check(context, annotation)

    def test_unsupported_resolution_and_unknown_fields_rejected(self):
        changed = copy.deepcopy(self.annotation)
        changed["judgments"][0]["resolution"] = "probably"
        with self.assertRaises(ValueError):
            self.check(annotation=changed)
        changed = copy.deepcopy(self.annotation)
        changed["model_input"] = "reviewer-generated summary"
        with self.assertRaisesRegex(ValueError, "unexpected annotation fields"):
            self.check(annotation=changed)
