# Candidate numerical contract for parent guidance

Frozen before any candidate corpus outcomes, 2026-10-02.
P0 `mash-c0d.19.2.20.8.3.3.3.9.4.3`. Research only; the earlier exact-float
captures, production retrieval and clustering remain unchanged.

The audits found tiny reassociation differences despite close scalar geometry.
An epsilon comparator is not transitive: A can be close to B and B to C while
A is not close to C. Do not use one as a sorting comparator.

Candidate v1 uses the exact rational value of each finite binary64 input:

- Multiply that value by the fixed integer 10^12 and round to nearest integer,
  ties to even, using integer division/remainder. No floating multiplication,
  decimal string conversion, clamping or tuned resolution.
- Positive eligibility remains the original `score > 0`, independent of bins.
  Tiny positive scores rounded to bin zero remain eligible. No source cap.
- Sort eligible items by descending integer bin then ascending unique
  nonnegative global ID; ranks are ordinal, not shared competition ranks.
- Fuse source and positive-parent ordinal ranks with exact reduced rational
  `1/(60+source_rank)+1/(60+parent_rank)`. Absent positive parent adds zero.
  Sort exact rationals descending then global ID, return at most20. Decimal
  float scores are display only; persist numerator/denominator as port evidence.
- Source/parent IDs and ownership are exact; reject malformed/nonfinite scores,
  bool IDs, duplicate IDs, missing parents and mismatched arrays.

This intentionally changes near-tie decisions versus exact-float reference.
The 10^-12 bin width is an engineering candidate, not a measured universal
error bound, tuned quality optimum, selected method or proven inference parity.
Bin boundaries can still split arbitrarily close values. Given an independently
justified absolute score error bound, calculate exact lower/upper endpoints;
report whether both quantized key and positive eligibility stay invariant.
Do not quietly enlarge the tolerance, normalize away discrete differences or
claim robust parity when an interval crosses a boundary. The diagnostic does
not establish that a supplied bound covers a model/platform or complete ranking.

Fixtures must include exact half bins with even/odd neighbors, signed zero,
one-ULP values around boundaries, tiny positives, arbitrary IDs, exact fusion
ties, reordered inputs, rejected malformed ownership, and generated differential
comparison against a separately written rational oracle. Standard-library
reference avoids NumPy/SciPy score kernels; it does not recompute embeddings.

Before adoption: paired complete-corpus comparison against the prior exact-float
direct/topic/thread reference, all changed IDs/ranks and original query/span
metrics retained, begin/end pins and complete repeat, independently reconstructed
formula/metrics, scope/cost/memory measurements and living paper. Freeze a
held-out protocol before observing its outcomes; do not tune bins on held-out
queries or labels. Coding-session labels/multitags/ancestors, guided-agent
utility, shared online lifecycle, complete ingestion <=10% loss and selected
executable Python/Rust parity remain separate gates.
