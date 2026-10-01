# Shared replacement while serving candidate searches

At `08087f1`, a production HTTP Qdrant service published the frozen real-history
corpus, then replaced it while serving message/thread candidate searches and
rejecting mutations from a second configured node. Both publications returned
HTTP 200 with all 789 conversations, 225,267 messages, and 29,453 searchable
messages/vectors. Revisions advanced from 1 to 2, and the changed thread title
was visible afterward. No partial publication or scope violation was observed.

## Configuration and workload

- Qdrant 1.19.1, cached placement, exact vector retrieval, 4 CPUs, **8 GiB**
  container limit. The API process had no memory limit.
- One isolated node group, two authenticated nodes, `max_concurrent_uploads=1`,
  and a 2,048-MiB node cap. The second node's mutations were rejected while the
  first node's replacement occupied the slot.
- Original body: 550,139,311 bytes; replacement: 550,139,333 bytes. Same bodies
  and title-only revision change as the [snapshot-read experiment](2026-10-01-shared-snapshot-reads.md).
- Thirty baseline searches, then 198 searches during replacement: alternating
  message/thread candidates, rotating unfiltered/Claude/Codex scopes, and 30
  stored document vectors with three diagnostic lexical strings. One reader
  issued requests sequentially, pausing 500 ms between overlap iterations.
- Provider filters and node ownership were checked on every returned hit.
  This does not exercise multiple simultaneous readers, context retrieval,
  full CLI/MCP search, query embedding, reranking, or relevance judgments.

The corpus contains accepted contributions from an incomplete native refresh.
Original provider histories and SQLite stayed read-only, and readiness was not
changed. This is not a complete-current-corpus benchmark. Host load was
uncontrolled; these are single observations rather than repeated trials.

## Results

| Measurement | Baseline | Replacement with searches |
| --- | ---: | ---: |
| Message candidates, samples | 15 | 99 |
| Message candidates, p50 | 24.1 ms | 40.3 ms |
| Message candidates, p95 | 41.1 ms | 68.2 ms |
| Thread candidates, samples | 15 | 99 |
| Thread candidates, p50 | 52.1 ms | 81.0 ms |
| Thread candidates, p95 | 105.1 ms | 167.2 ms |
| Mixed candidates, p95 | 93.8 ms | 145.2 ms |

All 228 candidate requests returned HTTP 200. The baseline includes the first
query after publication; it is not a separately controlled warm/cold comparison.
The repeated diagnostic workload and document vectors do not establish
natural-language query accuracy, ANN recall, or end-to-end latency targets.

Initial publication took **98.5 seconds**, including **1.90 seconds** of upload
transfer. Replacement took **113.2 seconds**, including **1.86 seconds** of
transfer. Counts remained unchanged; no messages or canonical context were
discarded. These times cannot establish a speed improvement over earlier runs
with different resource limits and host load.

The second node received **197 retryable HTTP 503 responses** to `forget`
attempts. An additional upload probe received retryable 503 in **2.30 ms**, before
its deliberately incomplete NDJSON body could reach storage. This corroborates
the shared mutation count limit under a real large replacement; cancellation
lifetime is covered by the deterministic regression tests in `08087f1`.

Maximum sampled API high-water RSS was **793,884 KiB** (about **775 MiB**).
Qdrant's maximum sampled container memory was **2.049 GiB**, above the previous
2-GiB evaluation limit. The database stayed running and was not OOM-killed.
Sampling can miss peaks, and container memory includes more than process RSS.
The API retained allocations after earlier phases; this is not a measurement of
incremental memory per reader or per mutation.

## Implications and remaining work

One large replacement can coexist with this bounded candidate workload at the
tested capacity. Large histories should continue to use one mutation slot.
This run does **not** validate the default four slots for this corpus, a 2-GiB
database limit, or a total API RAM bound. There is no process-wide memory budget
yet, and backend staging/old revisions have their own memory costs. It also does
not prove safety for simultaneous context reads or multiple API processes.

Keep `mash-c0d.21` open for memory-aware admission, simultaneous accepted
publishers/readers, explicit resource-budget failures, and supported backend
budgets. The observation motivates budgeting both the API and database rather
than treating a request count as a RAM guarantee.

Private artifacts are in
`/home/james/scratch/mash-artifacts/shared-concurrency-20261001`: executable
harness, exact configuration, binary/body hashes, aggregate request responses,
resource samples, and verification/cleanup evidence. The dedicated backend
started with no collections; its two owned collections, container, and anonymous
volumes were removed after the benchmark and API process completed. Unrelated
services were untouched.
