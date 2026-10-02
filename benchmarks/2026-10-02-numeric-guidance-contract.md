# Candidate transitive numerical guidance contract

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.3`, completed research study, no adoption.
[Protocol](../research/topics/NUMERIC_GUIDANCE_PROTOCOL.md).
No production, clustering, feature or historical capture change. This is a
numerical reference/fixture result, not corpus quality or performance evidence.

## Method and rationale

Earlier scalar feature audits agree within tolerance but do not prove identical
rank winners; parent reciprocal-rank contributions can amplify tiny differences.
A pairwise epsilon comparator is unsuitable for sorting because its equality
relation need not be transitive. The candidate assigns integer keys and defines
an actual total order instead.

Convert each supplied finite binary64 score to its exact rational value, scale
by the fixed integer 10^12 and round by integer division/remainder to nearest,
ties even. Keep original `score > 0` eligibility; do not discard tiny positive
scores that round to zero. Rank descending key, ascending unique global ID.
Ordinal source/positive-parent ranks feed the fixed k=60 adaptation as exact
reduced fractions; final order compares those fractions then global ID. Retain
complete source/parent pools and at most20 outputs. Typed dataclasses preserve
source/parent identities, raw scores, keys/ranks and exact rational weights.

This intentionally differs from the earlier exact-float reference. The fixed
10^-12 width is an engineering candidate, not a tuned optimum or proven error
bound. Arbitrarily close scores can straddle a bin boundary. Given a separately
justified absolute error bound, the reference checks exact interval endpoints
for key and eligibility invariance; crossing a boundary reports instability.
No blanket tolerance, hidden clamp or automatic production adoption follows.

## Fixture evidence

Nine standard-library tests pass, including:

- Exact representable half-bin values with even/odd positive/negative neighbors.
- One-ULP values around a boundary: different keys and explicitly unstable interval.
- Signed zero and the smallest positive binary64: zero keys but distinct eligibility.
- A nontransitive epsilon chain with permutation-invariant integer-key ordering.
- Exact rational fusion ties, global-ID tie ordering and absent-parent contribution.
- Sole-parent invariant for quantized direct order, explicitly differing from raw
  exact-float order on a near-tie case.
- Empty and 1,000-positive inventories with unchanged complete candidate counts.
- Rejected nonfinite/malformed/duplicate/bool IDs, ranks and missing ownership,
  including ineligible source rows.
- 200 deterministic generated inventories against a separately written oracle
  using `Fraction.__round__`, independent rank assembly and exact fractional
  fusion. Binary64 conversion/Fraction arithmetic are shared standard-library
  dependencies; this is not independently implemented Rust parity.

Full enabled research suite: 579 tests pass in 2.791s. This duration describes
the fixture suite, not ingestion/search latency. The numeric tests also pass
under the system Python without NumPy/model dependencies and are added to the
existing CI fixture step; remote CI execution is not yet verified.

No new corpus read, inference, model download, LLM call or parameter grid.
No numerical corpus quality, full-ingestion cost or memory performance gate was
run. Reference is deliberately scalar/exact; production cost must be measured,
not inferred from correctness fixtures.

## Complete corpus comparison

First target and complete repeat finished successfully; all non-timing fields
match exactly. Independent audit passes. This establishes reference arithmetic
and measured training drift/cost, not production adoption or inference parity.
First capture `topic-qmsum-numeric-guidance-first-20261002.json` SHA256
`0487c7f64bb354fc6b89b55b8e12fe5efbd233808773ec3b03aa2d7c5bcb5136`.
Repeat capture SHA256
`240e4337f1924ee4d3772a0c0094ecf309fb7d7d1cdb5ae24ff7b6b6051f29d6`.

| Scope/arm | Changed orders | Changed memberships | Recall improves/declines/ties |
|---|---:|---:|---:|
| Meeting/direct | 7 | 0 | 0/0/1095 |
| Meeting/topic | 7 | 0 | 0/0/1095 |
| Meeting/thread | 7 | 0 | 0/0/1095 |
| Global/direct | 119 | 87 | 0/0/1095 |
| Global/topic | 151 | 51 | 0/0/1095 |
| Global/thread | 183 | 64 | 1/2/1092 |

Direct/topic recall and precision stay exactly unchanged. Known-meeting direct
MRR .537946 to .537794, topic .542060 to .541908; NDCG also changes slightly.
Global topic MRR .197217 to .196778 and NDCG .090787 to .090728.
Global thread macro recall .068532 to .068709 but precision .036484 to .036438;
MRR .210416 to .210403. Gains/losses can have different sizes. This is neither
behavior-identical nor a demonstrated general quality improvement. Global
membership replacements can be unjudged; do not call them false positives.

First retrieval-plus-metric median/p95 milliseconds:

| Scope | Combined exact replay | Numeric direct | Numeric topic | Numeric thread |
|---|---:|---:|---:|---:|
| Meeting | .783/1.446 | 1.526/4.563 | .610/1.667 | .407/1.095 |
| Global | 73.385/301.582 | 96.883/284.743 | 174.159/530.602 | 141.462/505.052 |

Combined exact replay includes all three arms/metrics/equality checks; it is
not a per-arm matched baseline. Fixed order/cache/host/monitor effects remain;
source-array conversion/projection excluded. Direct timing can include release
of the previous scope's retained full ranked tuple at assignment. Do not infer
an isolated per-arm speed ratio or deployed latency. Scalar Fraction reference
is expensive in this full-pool prototype; correctness utility and production
implementation cost are separate questions. After-import wall822.550s,
CPU680.847s, setup20.448s, feature construction122.921ms. Imports/final JSON
serialization excluded from main timings, included externally; no ingestion gate.

Rusage peak2,197,912KiB; external RSS/PSS maxima2,198,880/2,190,723KiB,
approximate status HWM2,198,880KiB. Samples18,961, vanished0, median/max
gaps36.845/383.136ms. One target, all arms/unused input validation; children/
observer excluded, monitoring intrusive, sampled maxima lower bounds, status
approximate, launcher high water can survive exec. No subtraction or worker
memory attribution. First/repeat scientific targets sequential; brief result
inspection before repeat and held-out source/fixture/paper preparation during
repeat must be disclosed in final cost analysis.

Repeat after-import wall692.461s, CPU652.772s, setup16.757s,
feature construction131.197ms. Global combined-exact/direct/topic/thread
median/p95 69.751/133.976, 92.402/146.708, 161.152/483.181,
131.149/448.494ms. Same interval/allocation/order limitations as the first run.
Rusage2,197,744KiB; external RSS/PSS2,198,780/2,190,890KiB, approximate HWM
2,198,780KiB, samples17,898, vanished1, median/max gaps35.785/231.687ms.
Single-target monitoring scope and no-worker/no-ingestion limitations unchanged.

Independent audit artifact `topic-qmsum-numeric-guidance-audit-20261002.json`
SHA256 `44efb2e0beaac30a5ba6ebaa7ba612db6109630eeead62cfc54d280487aa8f55`.
All1,095 queries/6,570 cases/121,512 selected records/702,141 feature coordinates
pass; maximum scalar feature/numeric error2.22e-16. Original count/thread
pooling/query/span/ownership/bin/rational-order/formula/metric/drift reconstruction
without producer imports. Separate Fraction rounding oracle, but exact binary64
conversion/Fraction arithmetic shared; exact projection replay shares NumPy/
SciPy IEEE kernels. Not independent scalar tie-winner, platform-bound or Rust
parity. Auditor SHA256
`23bf4ae929e9deb6d28ef5e324aabe569437625526649009658b2ec25f640095`;
reusable audit reference SHA256
`a8335735133093502aed4115905b3bdc2c2fcbe81413d89d406127f6751d55a7`.

## Remaining work

The complete study runner is now `research/topics/qmsum_numeric_guidance.py`.
It uses the frozen thread-context control SHA256
`1d23e6af3bc94373b47ebb3900eb913fe974a6a8efae9815528059f7428f1dd4`,
reconstructs identical source/topic/thread features, replays all exact-control
records, then compares numeric direct/topic/thread arms in both scopes for all
1,095 queries. Retain complete positive pools; all changed orders, added/removed
IDs, ordinal ranks, exact rational weights, original metrics and eligibility
checks are captured. Known-meeting thread results must equal quantized direct
order/metrics. Record setup/projection/replay/per-arm retrieval-plus-metric
costs, CPU/rusage and external one-target memory samples. A separate auditor
reconstructs count pooling, query/spans, nearest-even bins using Fraction rounding,
exact rank weights, selection, drift summaries and metrics without producer
imports. NumPy/SciPy score projection and standard-library Fraction arithmetic
remain shared dependencies; no Rust equivalence or independent scalar winner
proof follows.

Two additional scientific-environment fixtures verify exact numerator/denominator
weights, integer keys/ranks and absent-parent metadata survive JSON round trips.
The full enabled suite at this stage passed 581 tests (3.023s). These fixtures overlap
the first corpus run briefly, as does the manuscript build; no corpus result, cost, repeat or audit pass is
claimed while that run is live. Do not restart on observation timeout. The
first/repeat/audit artifacts belong outside Git under the
`topic-qmsum-numeric-guidance-*-20261002` prefix. No new model work or held-out
read. This complete study may disqualify the scalar reference on cost even if
its numeric decisions are useful.

The complete paired comparison/repeat/audit/cost scope is now done. Retain this
as a research reference, not an efficient production implementation or evidence
that bins fix arbitrary inference drift. Held-out evaluation is frozen separately
before quality outcomes; see GUIDANCE_VALIDATION_PROTOCOL.md. This reference cannot
replace independent coding-session semantic labels/multitags/ancestors,
guided-agent utility, bounded shared online lifecycle/edit/delete/restart,
complete ingestion <=10% loss, justified platform score-error bounds or selected
executable Python/Rust parity.
