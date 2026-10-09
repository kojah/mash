# Filtered Redis HNSW policy evaluation

Filtered searches can benefit from HNSW, but forcing graph traversal across all
filters is a regression. This experiment supports further selective-policy
evaluation; it does not change production's exact-retrieval default.

## Method

The same frozen corpus as the [initial pilot](2026-10-01-hnsw-pilot.md) supplied
100,000 quantized 384-dimensional document vectors. Corpus SHA-256 remains
`6a5e4eb28914dbb2801d0805bba5876364302738b8a98f674b240256f4b9c252`.
Each of 72 scenarios used 100 fixed-seed document-vector queries, requesting 21
neighbors and removing the query document before evaluating at 20. Query vectors
remain in the graph, so these are not out-of-sample user-query results.

Exact Redis FLAT is the reference. HNSW uses M=16, EF_CONSTRUCTION=200 and
EF_RUNTIME=10/50/200. Filter policies are automatic, explicit ADHOC_BF, BATCHES
with automatic batch sizing, and BATCHES with fixed sizes 32/128/512. Explicit
brute force rejects EF_RUNTIME as irrelevant; it is measured once per filter.
Filters admit one provider (32,140 vectors) or the most recent 50,001, 10,001,
or 1,001 vectors, based on actual timestamp thresholds rather than assuming
exact percentages.

Every returned neighbor is checked against the filter and corpus bounds. Exact
reference result counts were independently checked against the matching
population, accounting for source exclusion. Reports retain each query's
latency, returned/expected count, exact-ID recall and distance-boundary recall.
The latter admits equally distant alternatives within 1e-6 of the exact top-k
boundary; it is not a relevance judgment.

This single sequential run uses a reused loopback Redis connection and includes
neither embeddings nor HTTP, lexical retrieval, fusion, reranking or CLI startup.
Host load started at 5.87/7.36/8.00. No concurrent writer, cold-cache or fleet
trial was performed; comparisons do not control all cache/order/load effects.

## Main results

The graph rows below use EF_RUNTIME=200 and automatic batch sizing.

| Filter population | Exact FLAT median / p95 ms | HNSW automatic median / p95 ms | Forced graph median / p95 ms | Graph boundary recall mean / minimum |
| --- | ---: | ---: | ---: | ---: |
| Provider: 32,140 | 9.09 / 13.60 | 16.27 / 24.15 | 3.62 / 41.45 | 98.55% / 85% |
| Recent 50%: 50,001 | 23.67 / 27.88 | 25.71 / 31.17 | 2.65 / 13.76 | 98.95% / 80% |
| Recent 10%: 10,001 | 8.49 / 9.82 | 8.05 / 10.02 | 4.42 / 15.02 | 97.35% / 65% |
| Recent 1%: 1,001 | 0.50 / 0.93 | 0.74 / 1.27 | 8.59 / 19.99 | 98.70% / 80% |

Corresponding forced-graph exact-ID recall means are 98.35%, 98.70%, 97.30%
and 96.00%. Automatic and explicit brute-force filtered cases had 100% exact-ID
recall in this run. No 100k scenario returned fewer neighbors than its reference,
but recall losses remain despite complete result counts.

Fixed batching does not uniformly improve the tails. At EF_RUNTIME=200, batch
size 512 improved recent-50% p95 to 10.19 ms (mean boundary recall 98.75%,
minimum 75%). Provider p95 instead rose to 54.96 ms. Batch size 32 made provider
p95 205.37 ms, versus 13.60 ms for exact FLAT. Favorable median-only comparisons
would hide these regressions.

## Execution and explanation

All 72 profiles match the requested policies: 48 forced-batch scenarios used
HYBRID_BATCHES; 16 automatic filtered scenarios and four explicitly forced
brute-force scenarios used HYBRID_ADHOC_BF. Four unfiltered scenarios used
STANDARD_KNN. The speedups therefore reflect actual graph traversal, rather
than merely requesting HNSW while Redis scores the filter population directly.

Provider-filtered query timings also varied with the source document. For the
33 queries whose source belonged to the selected provider, forced graph median
was 1.26 ms; for the 67 outside that provider it was 20.42 ms. This is consistent
with needing more graph traversal to find qualifying neighbors when nearby
candidates fail the filter. It does not isolate semantic correlation, physical
layout or cache effects. Filter population alone is insufficient to predict
the best policy.

A preceding 1k smoke run found incomplete result sets for some fixed-batch
settings on the recent-1% filter, whose population was smaller than requested k.
At EF_RUNTIME=10, batch size 32 had shortfalls in all five smoke queries and mean
boundary recall 83.64%. The harness records shortfalls and penalizes recall
instead of discarding failed queries or treating their fast timing as success.

The official [Redis vector documentation](https://redis.io/docs/latest/develop/ai/search-and-query/vectors/#how-filtering-works)
describes batch traversal, adaptive brute force and runtime controls.

## Recommendation and reproduction

Keep exact retrieval as the production default. Broad filtered searches provide
a credible use case for optional graph traversal, but narrow filters and tail
latency need an exact fallback. Neither a simple population threshold nor a
fixed batch size is validated by this run. Before shipping a policy, evaluate
actual out-of-sample user queries, relevance/ranking effects, end-to-end latency,
concurrent mutations and repeated controlled runs under the existing performance
budget. Faster vector retrieval alone does not establish faster, relevant search.

The historical run used 100,000 frozen vectors, 100 queries and `k=20`, with
profiles and filtered policies enabled. The obsolete pilot harness has been
removed from the repository.

Reports are retained privately at
`/home/james/scratch/mash-artifacts/hnsw-filtered-policy-20261001/report.json`
and `hnsw-filtered-smoke-20261001/report.json` in the same artifact root.
All pilot indexes and keys were removed after each run; unrelated test namespaces
were preserved. Frozen vectors and reports stay outside Git.

Tracking: `mash-c0d.11.3` (this comparison), `mash-c0d.11` (broader evaluation).
