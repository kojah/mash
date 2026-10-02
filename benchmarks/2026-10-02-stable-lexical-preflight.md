# Stable lexical frame preflight and normalization boundary repair

Scope: source-count lineage, deterministic encoding and numerical input guards.
No topic tree fitted, labels evaluated, models downloaded/inferred, LLM used or
production method selected. Parent bead `mash-c0d.19.2.20.8.3.3.3.7` remains open.

## Boundary repair and historical evidence

`SparseAverageTree` now requires a **positive finite computed norm**, matching the
dense prototype's acceptance decision. Tiny1e-18 input is accepted; overflow1e308,
underflow1e-200 and duplicate-coordinate cancellation fail before node/identity/
statistics mutation. Caller input is preserved. CSR reciprocal scaling differs
from dense division by1.11e-16 on the tiny fixture: an initial bit-exact assertion
failed, then the fixture used the already declared1e-12 numerical tolerance.
This is not a relaxed clustering or quality gate. Existing prefix/topology/count
parity fixtures still pass. Two new boundary fixtures and five frame fixtures:
enabled full lab **516 tests pass in2.669s**. No Rust/application changes.

The previous sparse-average controls remain pinned to historical commit
`1d2a35b`, including their original normalization helper. Captures and numerical
outcomes were not rewritten or rerun under an altered source pin. Reproduce those
controls from that commit; current-source comparison must not pretend it is the
same frozen implementation. This guard repair proves boundary behavior on
fixtures, not a new corpus quality score or chosen Rust implementation parity.

## Source-only frame contract

[STABLE_LEXICAL_PROTOCOL.md](../research/topics/STABLE_LEXICAL_PROTOCOL.md) frozen
before corpus outcomes. Feature hashing avoids fitting coordinates to future
vocabulary, at the cost of collisions; [official background](https://scikit-learn.org/stable/modules/feature_extraction.html#feature-hashing).
Our candidate uses fixed16384 nonnegative buckets, SHA256(domain + UTF8(term)),
first8bytes little-endian mod16384; domain `mash.topic.lexical.v1` plus NUL.
Source counts use Python3.11 Unicode lowercase/token regex and subtract exact
case-sensitive transcript markers, retaining adjacent words. Weight each original
term by1+ln(count) **before** bucket coalescing, then float64 L2. No fitted IDF,
vocabulary, mutable DF, projection or embedding changes. Unknown terms hash
directly; prior rows do not depend on arrival/provider/node order.

Marker-only TF and fixed existing English/acknowledgement-filter TF policies are
prespecified controls, not claims that filtering helps geometry. Filtered-empty
blocks explicitly retain raw geometry with fallback state and no invented label.
Raw-empty sources remain unassigned with no direction, never silently dropped.
Version descriptor/fingerprint includes hashing/normalization/token/Unicode,
marker regex/flags, policy/stoplist/fallback semantics. Mixed frames reject;
descriptor change requires new generation/rebuild/publication. Groups keep separate
namespaces. No edit/delete/restart service support is implied by stateless encoding.

Tests cover future/order invariance, original count preservation, exact markers,
flagged fallback/unassigned behavior, mixed-frame rejection, invalid count/token
schemas and per-term weighting under collision. Hash golden `cbioportal` bucket805
was independently checked with system sha256sum:
`2503274576f78b237f5ae057836e7d56e72aed91cba73457059594bedd44d4c4`.
The normalization/dynamic Python adapter is not the final typed Rust aggregate
representation; selected Unicode/token/numeric parity fixtures remain required.

## Complete source and encoding preflight

Reconstruct counts from every88,336 original QMSum source turns and compare exactly
to the frozen marker-only count ledger. Previous vocabulary is used solely to
decode/verify the source ledger, not to fit coordinates or learn weights. All
8,196 immutable physical blocks retain original ownership. No heldout source or
gold-driven selection. Two encoded CSR banks8,196x16,384:

| Policy | Ready | Flagged raw fallback | Empty | Nonzero coordinates | Blocks with collisions | Colliding term surplus |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Marker-only TF | 8,196 | 0 | 0 | 611,407 | 1,392 | 1,780 |
| Static-filter TF | 8,078 | 118 | 0 | 321,549 | 546 | 610 |

All nonempty directions have unit norm within1e-12; capture every fallback/source
identity and all matrix file hashes. Colliding surplus is distinct terms minus
distinct buckets per block, not corpus-wide collision probability or observed
topic error. Filtered fallback1.44% is visible; no support loss hidden in metrics.
Hashed coordinates require unhashed original terms for readable labels; collision
rates alone establish neither usefulness nor accuracy.

Frame fingerprints:

- Raw `3f47b8d3a3635b28325fd586a96957d58b1a6385bd1c0187cce9420961f5b8ab`.
- Filtered `cf110d1df2b7c097147676746b11d576b2d97fc864d0d151417cae082b837587`.

Outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-stable-frame-preflight-first-20261002/results.json`, SHA256
  `1c989b0d905270a3b667e6a62c4f12e73ef8cce50e4a075a47a55675731ead86`.
- Repeat `topic-qmsum-stable-frame-preflight-second-20261002/results.json`, SHA256
  `97e9c6e7f7a0f89b4889a085a15eb07d4b05e15284c886b44b512692e1848958`.
  Every non-timing field and all encoded matrix files/ownership/fallback/descriptor/
  source/contract/helper pins exact. No topic algorithm result in this capture.
- Preflight driver `topic-qmsum-stable-frame-preflight-20261002.py`, SHA256
  `db59c0296dcf6202196a2626ed84b26b8353706642a427ae180e743d9e18327e`.
- Independent `topic-qmsum-stable-frame-preflight-audit-20261002.py`, SHA256
  `aed518b892a9bf90ebe5139ab862cbbdcb9e5fa1456cee7f4e1b4a48607af316`;
  `topic-qmsum-stable-frame-preflight-audit-first-20261002.json` verifies true.
  No frame helper imports: aggregate frozen original source counts, independently
  compute scalar hash byte order/term weights/collision sums/norm, check all
  ownership/state/coordinate fields.16,392 block-policy cases/932,956 coordinates,
  maximum coordinate error0. Shared previously audited source-count ledger and
  NumPy/SciPy aggregation; not independent tokenization, tree quality or Rust parity.

## Scoped cost and next gate

One numerical-library thread. First/repeat offline after-import wall18.040/18.512s,
CPU15.590/16.042s. This includes large captured label/source ledgers and count
verification/encoding/persistence; no isolated hashing, topic or complete-ingestion
speed claim. Imports/final result JSON/repeat comparison outside timer, inside
external scope. No intentional benchmark overlap, uncontrolled host load; no
speed conclusion from variation.

Rusage peaks1,676,500/1,676,628KiB; sampled RSS/PSS/statusHWM
1,676,984/1,669,340/1,676,984 and1,677,372/1,669,496/1,677,372KiB.
561/579 samples,1/1 vanished, median/max gaps40.737/81.774 and39.779/77.204ms.
One target including imports/source/large label ledger, excludes observer/children;
intrusive sampled lower bounds, status approximate, launcher high water possible,
no subtraction. Not production topic memory or local inference worker usage.

Next fit rotation-only both orders on these two frozen banks plus collision-free
TF diagnostics, retaining original-source support and offline TF-IDF reference.
Evaluate collision distortion, all/far/recurrence, labels and representatives.
Stable encoding does not prove global tree/state lifecycle or semantic ancestor/
multitag/guided-search quality. Complete ingestion<=10% loss and selected
Python/Rust differential CI remain unverified; keep topic selection open.
