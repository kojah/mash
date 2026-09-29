# Shared index comparison — 2026-09-30

Baseline: commit 98dd113, optimized release builds. Xenia hosted both databases and the HTTP API. Dev MacBook, personal MacBook, and Xenia also participated in network searches. Redis is now the default; PostgreSQL remains selectable.

## Results

Pooled HTTP latency in milliseconds; throughput is median trial requests/second. Each row uses three trials. Mixed workloads have eight readers and three independent writers, each publishing three 64-message updates.

| Messages | Readers / writers | Backend | Read p50 | Read p95 | Requests/s | Update p50 |
|---|---|---|---:|---:|---:|---:|
| 3,000 | 1 / 0 | PostgreSQL | 44.60 | 61.18 | 20.54 | — |
| 3,000 | 1 / 0 | Redis | 9.63 | 14.90 | 99.46 | — |
| 3,000 | 8 / 0 | PostgreSQL | 71.45 | 98.10 | 114.01 | — |
| 3,000 | 8 / 0 | Redis | 43.57 | 64.98 | 187.29 | — |
| 3,000 | 8 / 3 | PostgreSQL | 73.04 | 110.14 | 104.27 | 113.58 |
| 3,000 | 8 / 3 | Redis | 64.52 | 102.07 | 113.14 | 909.09 |
| 15,000 | 1 / 0 | PostgreSQL | 188.39 | 239.77 | 5.35 | — |
| 15,000 | 1 / 0 | Redis | 12.85 | 17.25 | 72.28 | — |
| 15,000 | 8 / 0 | PostgreSQL | 359.91 | 513.31 | 22.33 | — |
| 15,000 | 8 / 0 | Redis | 67.85 | 85.85 | 116.38 | — |
| 15,000 | 8 / 3 | PostgreSQL | 334.36 | 446.43 | 22.73 | 182.91 |
| 15,000 | 8 / 3 | Redis | 101.53 | 177.82 | 39.45 | 1832.12 |

9,840 local read requests and 108 updates completed without API errors. A further 1,200 queries ran from the three deployed machines over Tailscale, in two trials per backend with four readers per machine. Network round trips from the MacBooks were approximately 112–122 ms; this raises the latency floor independently of database choice.

Redis Search scans a compact FLAT vector representation. Our PostgreSQL query scans SQL rows, checks vector dimensions, joins conversations, and ranks lexical matches. EXPLAIN ANALYZE on the 15,000-message corpus recorded approximately 101 ms for the semantic query and 25 ms for lexical retrieval, with **zero physical disk reads**. PostgreSQL was already serving cached data, so these results cannot be explained simply as RAM versus disk.

Redis's current publication implementation stages an entire node snapshot for atomic visibility, even for small updates. PostgreSQL updates changed rows transactionally. This explains Redis's substantially slower incremental updates; switching the default optimizes the measured read workload.

## Method and limits

Both engines received identical deterministic synthetic histories: three nodes, 1,000 or 5,000 messages per node, and 1,024-dimensional dense vectors. Both performed exact cosine retrieval, without approximate indexes. The harness checked semantic top results and counts. Five warm-up queries preceded each reader's measurements; backend ordering alternated. Serial trials issued 100 reads, eight-reader trials 400, and mixed trials 320 reads.

Each disposable database had four CPU cores and a 4 GiB container limit. Redis used its Search module, AOF persistence, appendfsync everysec, and a 3 GiB noeviction limit. PostgreSQL used pgvector on PostgreSQL 17, default shared_buffers (128 MiB), synchronous_commit on, and JIT on; the measured plans did not invoke JIT. These persistence policies have different crash-durability guarantees. Xenia is a shared AMD EPYC 9645 host with 16 visible CPUs and approximately 62 GiB RAM.

Timings include authenticated HTTP transport, retrieval, and publication, but exclude model inference, query embedding, and client reranking. Synthetic lexical matching does not establish relevance parity between engines. The largest corpus is much smaller than the real histories; these numbers are not full-corpus performance predictions. New chunked Redis snapshot storage is covered by integration tests, but the numbers above measure the stated baseline.

Reproduce using [the benchmark harness](../tools/benchmark-shared.py), separate disposable database/API instances, and MASH_BENCH_TOKEN_0/1/2 environment variables. Never use production histories or credentials for this fixture benchmark.

## Voyage estimate

A read-only scan of the configured 90-day source window found roughly 550,670 searchable messages across the three machines. Extracted search text suggests **88–147 million tokens**, using a coarse 3–5 characters per token heuristic. At voyage-4's listed $0.06 per million tokens, initial embedding would cost approximately **$5.28–$8.81** before credits. voyage-code-4 at $0.12 per million would be approximately $10.57–$17.61.

Voyage lists 200 million free tokens for these models; remaining allowance depends on account usage. These are estimates, not tokenizer counts or billing guarantees. Provider prefixes, repeat ingestion, deduplication, and long-input handling affect usage. Fifteen extracted messages exceeded 96,000 characters and may require chunking to fit the model input limit. No paid inference or real-history publication was performed: VOYAGE_API_KEY is not configured on the nodes.

Pricing source: [Voyage pricing](https://docs.voyageai.com/docs/pricing).

