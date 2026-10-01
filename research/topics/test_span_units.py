from copy import deepcopy
from dataclasses import asdict
import json
import unittest

from pair_review import Relation, validate_pairs
from reference_review import digest
from source_spans import structural_spans
from span_units import reconstruct


def fixture():
    text = "α🙂 database\n\ncache locks"
    identity = {"id": "id", "ordinal": 1, "role": "user", "searchable_sha256": digest(text.encode())}
    spans = [{**asdict(s), "text_sha256": digest(text[s.start:s.end].encode()),
              "source_bytes": len(text[s.start:s.end].encode())} for s in structural_spans(text)]
    report = {"schema_version": 1, "scope": "development-admitted-anchors-and-oracle-reference-evidence-spans",
              "offset_unit": "unicode-code-points", "rows": [{"thread": "t", "identity": identity,
              "source_chars": len(text), "source_bytes": len(text.encode()), "spans": spans}]}
    return {("t", 1): (identity, text)}, report


class SpanUnitTests(unittest.TestCase):
    def test_exact_unicode_reconstruction_has_no_body_in_metadata(self):
        documents, report = fixture()
        units = reconstruct(documents, report)
        self.assertEqual([u.text for u in units], ["α🙂 database", "cache locks"])
        self.assertNotIn("text", units[0].metadata())
        self.assertEqual(units[0].metadata()["text_sha256"], digest(units[0].text.encode()))

    def test_changed_body_offsets_or_scope_fail_closed(self):
        documents, report = fixture()
        for mutation in ("body", "offset", "duplicate", "scope"):
            data, capture = deepcopy(documents), deepcopy(report)
            if mutation == "body":
                identity, text = data[("t", 1)]
                data[("t", 1)] = identity, text + "x"
            elif mutation == "offset":
                capture["rows"][0]["spans"][0]["end"] -= 1
            elif mutation == "duplicate":
                capture["rows"].append(deepcopy(capture["rows"][0]))
            else:
                capture["rows"] = []
            with self.assertRaises(ValueError):
                reconstruct(data, capture)

    def test_pair_review_pins_endpoints_and_rejects_invalid_or_reversed_pairs(self):
        _, report = fixture()
        raw = json.dumps(report).encode()
        row = report["rows"][0]
        def endpoint(index):
            return {"thread": "t", "identity": row["identity"], "index": index,
                    "text_sha256": row["spans"][index]["text_sha256"]}
        pair = {"left": endpoint(0), "right": endpoint(1), "relation": Relation.DISTINCT, "rationale": "source review"}
        annotation = {"schema_version": 1, "scope": "development-source-span-topic-overlap",
                      "annotator": "exposed", "granularity": "task aspects", "spans_sha256": digest(raw), "pairs": [pair]}
        self.assertEqual(validate_pairs(annotation, raw)[0].relation, Relation.DISTINCT)
        for mutation in ("hash", "unknown", "bool", "malformed", "reverse", "self"):
            value = deepcopy(annotation)
            left = value["pairs"][0]["left"]
            if mutation == "hash":
                left["text_sha256"] = "changed"
            elif mutation == "unknown":
                left["thread"] = "unknown"
            elif mutation == "bool":
                left["index"] = True
            elif mutation == "malformed":
                left["identity"] = None
            elif mutation == "self":
                value["pairs"][0]["right"] = deepcopy(left)
            else:
                value["pairs"].append({**deepcopy(pair), "left": endpoint(1), "right": endpoint(0)})
            with self.assertRaises(ValueError):
                validate_pairs(value, raw)
