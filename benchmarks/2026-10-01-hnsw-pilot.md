# Redis HNSW retrieval pilot

This pilot supports further evaluation, **not an ANN default or configuration
change**. HNSW substantially reduced unfiltered vector retrieval at 100k vectors,
but provider-filtered retrieval regressed and approximate results differed from
exact neighbors. End-to-end search and user-query relevance remain unmeasured.

## Method

On Xenia, Redis 8.2.10 on disposable loopback port 57379 indexed deterministic
1k/10k/100k subsets of 102,666 existing 384-dimensional quantized embeddings.
The frozen corpus includes provider and message-time metadata, without history
text. It contains one embedding generation; its SQLite SHA-256 is
`6a5e4eb28914dbb2801d0805bba5876364302738b8a98f674b240256f4b9c252`.

Each scenario uses 100 fixed-seed document-vector queries. Retrieval requests 21
neighbors and removes the source document before measuring recall at 20 (or the
available filtered population). Exact Redis `FLAT` results are the reference.
The query document remains in the graph: removing its returned match does not
make this an out-of-sample query evaluation, and can make ANN recall look better
than it would for actual user queries.
HNSW uses `M=16`, `EF_CONSTRUCTION=200`, and runtime breadth 10/50/200. Vector
values are the stored signed bytes converted to float32, with cosine distance.

Timings include the Redis round trip over a reused connection, excluding
embedding, HTTP, lexical retrieval, fusion, reranking and JSON CLI startup. The
algorithms run sequentially, with no controlled cold-cache or concurrency trial.
The host was busy: initial load averages were 7.11/9.53/9.41. Both indexes share
the same hashes; reported memory is vector-index allocation, not total RSS.

## Unfiltered results

| Vectors | Algorithm / runtime breadth | Median ms | p95 ms | Exact-ID recall | Distance-boundary recall |
|---:|---|---:|---:|---:|---:|
| 1,000 | FLAT | 0.126 | 0.223 | 100% | 100% |
| 1,000 | HNSW / 50 | 0.153 | 0.298 | 99.65% | 99.65% |
| 10,000 | FLAT | 0.589 | 0.869 | 100% | 100% |
| 10,000 | HNSW / 50 | 0.240 | 0.356 | 93.10% | 98.10% |
| 100,000 | FLAT | 6.721 | 8.054 | 100% | 100% |
| 100,000 | HNSW / 10 | 0.314 | 0.651 | 91.20% | 96.65% |
| 100,000 | HNSW / 50 | 0.524 | 2.265 | 95.65% | 99.10% |
| 100,000 | HNSW / 200 | 0.653 | 1.089 | 98.15% | 99.90% |

Distance-boundary recall counts returned neighbors within the worst exact top-k
distance plus `1e-6`. It allows equal-distance alternatives, which matter for
duplicate history embeddings; it does **not** establish message/thread relevance
or ranking parity. Exact-ID recall is also reported rather than hiding these
differences. At 100k and runtime breadth 200, the minimum per-query exact-ID recall
was 65%, despite the 98.15% mean. The pilot does not record per-query boundary
recall, so the worst relevance impact cannot be inferred from aggregate numbers.

## Filtering, construction and memory

At 100k vectors, the provider filter admitted 32,140 documents. FLAT median/p95
was 8.662/14.683 ms; HNSW with runtime breadth 200 was **14.614/16.892 ms**, with
100% exact-ID recall. The recent-time filter admitted 1,001 documents: medians
were 0.496 ms for FLAT and 0.451 ms for HNSW/200, but p95 rose from 0.751 to
1.787 ms. HNSW was therefore not consistently faster for filtered searches.

Redis chooses hybrid filtering strategies automatically. A subsequent diagnostic
run on the same frozen 100k corpus captured `FT.PROFILE` after each scenario
(20 document queries per scenario, independent timing run). Unfiltered retrieval
used `STANDARD_KNN`; both FLAT and HNSW provider/time-filtered probes used
**`HYBRID_ADHOC_BF`**, with 32,140 TAG entries or 1,001 NUMERIC entries read.
The HNSW filter path therefore scored the matching vectors directly instead of
using approximate graph traversal. This explains the lost graph-search advantage
and the filtered probes' exact recall. It does not isolate the remaining
HNSW-versus-FLAT overhead or prove that every possible query selects this policy.

See the official [vector-search documentation](https://redis.io/docs/latest/develop/ai/search-and-query/vectors/)
for graph parameters, adaptive filter policies and their accuracy tradeoffs.
The diagnostic report is
`/home/james/scratch/mash-artifacts/hnsw-profile-20261001/report.json`; add
`--profiles` to the harness to capture these iterator profiles. Explicitly forced
batch policies, actual user queries and concurrent updates remain unevaluated.

Building indexes over already loaded 100k hashes took 3.227 seconds for FLAT and
53.907 seconds for HNSW. Vector-index allocations were 151.65 and 180.77 MiB
respectively (about 19% extra for HNSW). These exclude hash ingestion, total
process memory, durable storage, incremental publication, and rebuild recovery.

## Recommendation and reproduction

Keep exact retrieval as the production default. Continue the tracked evaluation
with actual user-query embeddings, per-query/tie-aware relevance judgments,
execution profiles for production filters, concurrent publication/deletion and
end-to-end hybrid search. Even the large unfiltered gain saves approximately six
milliseconds here; embedding and reranking can dominate the full request.

The [pilot harness](../tools/benchmark-vector.py) requires no Python packages. It
reads the source SQLite database without changing it, freezes vector metadata in
a private artifact directory, and uses isolated Redis namespaces with cleanup.
It only accepts the designated disposable test port and never flushes Redis.
Do not run alongside other performance measurements. For example:

```sh
python3 tools/benchmark-vector.py \
  --source /path/to/existing-index.sqlite3 \
  --artifacts /path/to/private/pilot-artifacts \
  --sizes 1000 10000 100000 --queries 100 --k 20
```

Reuse the frozen corpus with `--frozen /path/to/pilot-artifacts/vectors.sqlite3`
instead of `--source`, writing to a new private report directory. Frozen vectors
remain sensitive artifacts and must stay outside Git. The full report for this
run is `/home/james/scratch/mash-artifacts/hnsw-pilot-20261001-final/report.json`.

Tracking: `mash-c0d.11.1` (this pilot), `mash-c0d.11.2` (filter profiling),
`mash-c0d.11` (remaining algorithm evaluation).

The subsequent [filtered-policy comparison](2026-10-01-filtered-hnsw.md)
measures forced graph traversal, per-query recall/counts and selective-filter
latency tails. Its results supplement this initial automatic-policy pilot.
