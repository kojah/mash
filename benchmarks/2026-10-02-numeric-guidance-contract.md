# Candidate transitive numerical guidance contract

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.3`, still in progress.
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

## Remaining work

Keep the bead open for full paired direct/topic/thread corpus comparison,
retaining changed IDs/ranks and all original query/span denominators, exact
repeat and independent audit plus scope costs/external memory evidence. Freeze
held-out evaluation before inspecting its outcomes. This reference cannot
replace independent coding-session semantic labels/multitags/ancestors,
guided-agent utility, bounded shared online lifecycle/edit/delete/restart,
complete ingestion <=10% loss, justified platform score-error bounds or selected
executable Python/Rust parity.
