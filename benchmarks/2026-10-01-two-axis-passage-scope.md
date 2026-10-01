# Topic relevance and requested scope in passage retrieval

2026-10-01; `mash-c0d.19.2.29`, following
[stored-record retrieval](2026-10-01-stored-reference-record-selection.md).
No production method is promoted.

## Procedure and annotation exposure

Before passage scoring, freeze six anchor/source pairs from four already observed
failures:525/519,525/521,601/600,2285/2236,2285/2284,2302/2293. Sources are cited
antecedents and selected nearest alternatives. This is a **post-record-ranking,
oracle-source-shortlist diagnostic**, not representative or held-out evaluation.
Only one previously exposed coding-assistant source group is involved. Read each
source completely; annotate every structural unit in those pairs, preserving
current request, causal record identity, exact content digests and context bounds.
Names/rationales/labels never enter segmentation or scoring. No human ground truth
or automatic scope classifier is claimed.

Use independent axes: relevance (topical/background/procedure/metadata), and
requested scope (included/excluded/mixed/unspecified; not-applicable for nontopical
units). Rejected work can remain topical. Mixed indicates that the unchanged unit
contains both included/remaining constraints and excluded/overbroad work; it is
not a fabricated accepted passage. Keep-together recommendations can be included
constraints without requesting a split. Headings alone are framing, not concrete
topic detail; matching them may still help navigate to associated content.

`passage_scope.py` regenerates all units from the unchanged raw64 contexts, pins
upstream source/reference reviews, and records every excluded structural gap.
Every span/gap has Unicode-code-point and UTF-8-byte coordinates and a digest.
The entire original source is accounted for; whitespace/table framing stays in
explicit gaps. It performs no rewriting or inference. Annotation validation rejects
stale manifests, source/offset changes, reordered/missing/duplicate units, unknown
fields and inconsistent relevance/scope combinations.

## Source-unit findings

There are74 units:39 topical/included,2 topical/mixed,16 background,12 procedure
and5 metadata. The easy/next-tier request includes4 concrete units; the remaining13
request includes5 remaining-work/order units rather than prior completion evidence.
Reorganization evidence includes30 units, including constraints to keep cohesive
analyzers together. Installation status is a separate task. The broad all23
interpretation and the migration proposal each remain a single mixed paragraph.
Treating either as wholly accepted or wholly irrelevant loses information.

This review covers all units of the six chosen pairs, **not all candidate records
for21 references**. Wider annotation and independent quality evaluation remain open.

## Fixed ranking controls

Freeze current-user, preceding-user, captured-user-chain cosine and recency;
top1/top3; no threshold tuning. Recency takes the newest selected record and then
its original reading order. Cosine ties use manifest source order/unit index.
Use the existing offline single-thread/singleton MiniLM encoder and identical
unchanged source strings, normalized signed-byte directions, and2–512-token
preflight with no truncation. All candidate units fit; maximum encoded input is109
tokens. Missing prior-user support at the remaining-work anchor abstains for the
preceding/chain modes. No fallback to a different query or partial candidate set.

| Mode | Own supported anchors | Own top3 returned | Own topical | Own included | Own mixed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Recency | 4 | 10 | 2 | 0 | 2 |
| Current user | 4 | 10 | 7 | 5 | 2 |
| Preceding user | 3 | 7 | 2 | 0 | 2 |
| Captured user chain | 3 | 7 | 2 | 0 | 2 |

On the common3-anchor cohort, current-user top3 has5topical/3included/2mixed out
of7 returned; each other control has2topical/0included/2mixed out of7. A one-unit
migration source necessarily returns only one result even at top3. Do not compare
unmatched own-support denominators as equal workloads.

Current-user top1 retrieves only one included unit across four anchors. It prefers
the easy-fix and remaining-work headings over specific details, although top3 adds
some concrete scope. For deferred reorganization it reaches the older boundary
record; previous/chain queries instead favor installation material. Migration's
mixed paragraph remains unresolved regardless of ranking. This is different from
the record-level recency result: the granularity and metric change, and a useful
source identity does not guarantee useful atomic topic evidence.

Next test bounded structural section associations (heading plus original child
units) separately from standalone units, retaining provenance and scope exclusions.
No phrase-specific approval rules, automatic endorsement, stronger encoder or
production geometry change follows from this diagnostic. Fine-grained mixed-scope
boundaries and all-source coverage remain requirements.

## Reproducibility and performance limits

Two source manifests are byte-identical. Two fresh ranking captures match every
nonmeasurement field, including scores, model/source hashes, complete4-method
grids, support decisions and counts. Each capture has81 inference calls/129 cache
hits,138.64–145.74ms inference and343.05–344.52ms total after imports. These are
experimental four-case costs including model setup/input validation, not ingestion
overhead or production search latency. No generative/remote calls occur. Additional
encoding is not justified for production until matched quality and complete-ingestion
gates pass. No memory gate or new RSS result is claimed.

146 lab tests pass. Tests exercise Unicode/emoji/combining marks/CRLF, table gaps,
protected code, causal intact source coverage, dual-offset corruption, complete
annotations, topical rejection, recency reading order, missing ranking units/grids,
abstentions, nonfinite scores and first/second score drift. This is Python-side
fixture validation, not Rust parity CI.

Private root `topic-python-review-v3-20261001`:

- `passage-scope-manifest-{first,second}.json`: SHA256
  `95970be91022b27e85660e2027ad70e7c66aa2c2e817463c332aff6408e484db`.
- `passage-scope-review.json`: SHA256
  `3c2deddffe46485a902b9692bea5347f4e832380eaef34fc8c73635ef9f160d1`.
- `passage-ranking-{first,second}.json`: SHA256
  `cd8f1ebb5f8c9bef1d222180a9e6c87cc5cc0331d6b23c2043125789d05d6d1a`,
  `3e05c9f0ece52a2623b8e4f917cc6b4ec589ab9d88fb82ed86cfadfad8fc6920`.
- `passage-ranking-confirmed.json`: complete repeat and matched-support summaries,
  including strict source-coordinate/count validation and comparator provenance.

Run `passage_scope.py` with contexts/packets/source-review/reference-review/selection
and fresh private output; add `--annotations` for the review summary. Run
`passage_ranking.py` with the same inputs plus manifest/annotations/installed snapshot.
`compare_passage_ranking.py --first ... --second ... --manifest ... --output ...`
requires full source-unit coverage. An exploratory manifest predates the final
raw-bound/dependency pins and an earlier comparator summary predates the final
coordinate/count checks; both are excluded. Histories and judgments remain outside Git.
