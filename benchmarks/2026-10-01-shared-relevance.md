# Hybrid relevance across local and shared indexes

On Xenia, the public synthetic corpus in `docs/fixtures/search-relevance.toml`
passed 184 application searches: 23 queries in message and thread modes against
local SQLite, PostgreSQL/pgvector, Redis Search, and Qdrant. The four test contracts
completed in 27.724 seconds, excluding compilation.

All backends returned a labeled relevant first result for every positive query,
returned none of the explicitly forbidden results, and abstained on every query
labeled as having no relevant result.

| Storage | Message precision among returned top 5 | Thread precision among returned top 5 | Recall@5 | MRR@5 |
| --- | ---: | ---: | ---: | ---: |
| Local SQLite | 0.896 | 0.885 | 1.000 | 1.000 |
| PostgreSQL/pgvector | 0.896 | 0.885 | 1.000 | 1.000 |
| Redis Search, exact | 0.896 | 0.885 | 1.000 | 1.000 |
| Qdrant, exact | 0.896 | 0.885 | 1.000 | 1.000 |

## What was exercised

The corpus contains 17 threads and 20 native-shaped Claude records. Ingestion
retains 19 searchable messages; the internal-context-only record has no search
projection. Tests explicitly index with the installed local embedding and reranking
models, then search through the typed application core.

Each shared test publishes the local SQLite snapshot through the authenticated
mash HTTP client to a disposable node group, then performs shared-only searches.
It requires a successful shared read and no shared error, so a local fallback
cannot satisfy the relevance assertions. The path includes concrete storage
retrieval, lexical/vector fusion, reranking, abstention, and thread grouping.
All stores use exact vector retrieval for this matrix.

The queries cover paraphrases, technical model names, quoted phrases, exact
repository URLs, PR-versus-issue identity, hard negatives, and nonsensical input.
The added cases cover intersected provider/role/thread selections, provider and
role exclusions, nested `all`/`any`/`not`, scoped paraphrases, scoped phrase
exclusion, and a time cutoff that must exclude records without timestamps.

The gates enforce top-result relevance, explicit hard-negative exclusion and
empty-result judgments. Recall and MRR are averaged over the 16 positive queries;
precision divides the relevant matches by the actual returned count up to five,
then averages over positive queries. It is not precision divided by a fixed five.
Seven negative queries are checked separately for empty results.

## Reproduce and limits

Set `MASH_TEST_MODELS_DIR` to assets installed explicitly by `mash models install`.
Set `MASH_TEST_PG_URL`, `MASH_TEST_REDIS_URL`, and `MASH_TEST_QDRANT_URL` to disposable
services. Qdrant requires 1.19+ in the 1.x series. Run:

```bash
cargo test-clif --lib --run-ignored only -j 1 -E 'test(labeled)' --success-output immediate
```

The recorded run used PostgreSQL 17 with pgvector, Redis 8.2, and Qdrant 1.19.1.
Detailed results are retained outside Git at
`/home/james/scratch/mash-artifacts/shared-hybrid-relevance-20261001/expanded-matrix.log`.
CI runs the local contract with installed-model tests, and each shared contract
with its corresponding backend tests.

These are audit-authored synthetic labels, not independently reviewed user
judgments. This small corpus can detect regressions but cannot establish quality
on real history, large-corpus candidate truncation, ANN recall, combined local/peer
deduplication, or memory-pressure behavior. Equal aggregate metrics do not imply
identical intermediate rankings or general backend equivalence. Timings are from
a debug integration run with compilation/check activity on the host, not a
production latency comparison. Beads `mash-jh7` and `mash-dna` retain the broader
relevance and backend evaluation work.
