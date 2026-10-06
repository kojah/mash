# Search performance investigation, 6 October 2026

The straightforward shared-search defect was planner configuration: Mash sent
`exact=false`, but Qdrant retained its default 10,000 KiB scan threshold. Native
telemetry verified a fresh live query scanning all eight segments, taking 6.23
seconds. Request parameters alone did not establish actual HNSW execution.

New message collections use a 10 KiB scan threshold and a 1,024 KiB indexing
threshold. Existing HNSW groups require explicit query activation to schedule
the physical change. Exact overrides remain available. Activation preserves
silver, embeddings, graph topology, memory placement and node publications;
the backend rebuilds affected indexes once. Ordinary deployment does not ingest.

## Live baseline

Read-only MCP calls used four varied queries, the installed default of 20 thread
results, and contextual reranking. Each node ran two passes with local and
combined scope. First `cbioportal` calls are separate from the seven subsequent
calls per scope. These are small samples under existing host activity, not an
SLA or a controlled comparison of machines. Core timings exclude MCP startup
and SSH measurement transport.

| Node / scope | Subsequent median | Subsequent range | Initial call |
| --- | ---: | ---: | ---: |
| Xenia, local | 675 ms | 313–1,237 ms | 2,432 ms |
| Xenia, combined | 1,213 ms | 1,055–5,603 ms | 5,810 ms |
| Dev MacBook, local | 649 ms | 351–923 ms | 883 ms |
| Dev MacBook, combined | 2,001 ms | 1,768–5,395 ms | 5,365 ms |

Both initial combined calls hit the five-second shared-candidate deadline and
fell back. The SQLite query also approached that deadline without fallback.
The personal MacBook was offline and was not measured.

## Owned real-data planner comparison

An isolated Qdrant 1.19.1 instance used a copied, inactive retained real corpus.
The live backend was not restarted, evicted or changed for these experiments.
After optimizer queues and running jobs drained, eight query vectors were
tested with all-node, other-node, Codex, user-role and Xenia filters. Each of the
40 cases paired an exact query with HNSW at `ef_runtime=200`, requesting 200
candidates without payloads. These timings isolate native dense retrieval;
they are not complete Mash search timings or a matched deployment speedup.

| Measurement | Result |
| --- | ---: |
| Mean overlap with exact top 200 | 99.2875% |
| Minimum overlap with exact top 200 | 93% |
| Exact top-20 candidates included in HNSW candidates | 100% in every case |
| Median native HNSW time | 68.9 ms |
| Median native exact time | 116.3 ms |
| Actual filtered HNSW segment searches | 256 |
| Bounded small-cardinality segment scans | 24 |

Approximate overlap is a candidate-recall proxy, not a human relevance score.
Empty or very small filtered subsets still legitimately scan. The first cases
had larger timings under host contention; the one-time rebuild exceeded the
initial ten-minute observation window. Do not describe a configuration PATCH
or HTTP readiness alone as proof that a backend migration has finished.

## Local configurations rejected

Existing real-corpus inference benchmarks compared CPU thread budgets and
reranking batch sizes while checking result identities and scores. One CPU
thread slowed search; eight threads did not establish a useful, consistent gain
over the current default. The current reranking batch size of four had the best
observed median. Sorting by actual token lengths added tokenization overhead
without a speedup; the prototype was reverted. Those options were not shipped.

## Validation and evidence

The complete default suite passed 639 tests, with 125 explicitly ignored tests.
Ten native configuration/activation tests passed, including preservation of
canonical history and publication identities. Strict Clippy, formatting and
the deployment-helper rollback/activation checks passed. Native correctness
fixtures ran on a separate disposable backend; they were not mixed with the
large benchmark rebuild. An initial response-field mismatch in the new typed
decoder (`optimizer_config` on reads versus `optimizers_config` on writes) was
caught and fixed before deployment.

Private measurement records are outside Git:

- `/home/james/scratch/mash-artifacts/latency-current-20261006`
- `/home/james/scratch/mash-artifacts/search-performance-20261006`

They include provenance, stage timings, optimizer progress, native execution
counters and candidate comparisons. Credentials and raw history are not
included in public documentation. Deployment and fresh complete-search
acceptance measurements remain separate from these candidate tests.
