# Exact vectorized parent-rank guidance

Frozen before corpus outcomes, 2026-10-02, P0
`mash-c0d.19.2.20.8.3.3.3.9.4.1`. Optimize implementation, not methodology.
Reuse all original counts/features/queries/scopes and the audited scalar
parent-rank capture. No parameter/candidate/cutoff/stoplist/model change, LLM,
download, inference, held-out data or production selection.

Vectorize ownership lookup, full positive-rank reciprocal contributions and
final ordering. Construct only the at-most20 returned Python records. Keep all
source/group positive candidates and exact float/global-ID ties. Integer source
and parent identities are required; no allocation indexed by maximum global
ID. In-range float values are not rounded, clipped or quantized. This does not
resolve the separate near-tie Python/Rust contract.

Run the scalar and vectorized functions sequentially on IDENTICAL source/group
score arrays for all1095 specific queries in both meeting/global scopes.
Every field of vector output must equal scalar output and the frozen prior
capture. Fresh direct source IDs/scores must replay prior baseline too. Native
queries/spans, reference denominators, labels/group inventory and metrics stay
unchanged. Preserve complete non-timing repeat with start/end code pins and
reuse the independent original-count/query/span/formula/rank/metric audit;
scalar geometry evidence versus shared IEEE kernel exact replay remains
explicit. Generated fixtures exercise ties, one-ULP differences, arbitrary IDs,
zeros/negative values, empty pools and invalid ownership.

Capture matched scalar/vector stage timings, complete scope and external
RSS/PSS/rusage. Both implementations in one target makes memory a combined
process measurement, not attribution to either implementation. Sequential
pairs always scalar then vector; cache/order/host effects remain uncontrolled.
Report query ranking scopes, not deployed E2E latency or ingestion overhead.
No silent candidate cap. Validate broader/held-out scaling separately.

Parent issue stays open for robust numerical contract, held-out/coding topic
quality, bounded global lifecycle/query/ingestion and selected Python/Rust
reference parity. Faster exact replay does not select the production method.
