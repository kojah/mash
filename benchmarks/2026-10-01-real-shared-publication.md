# Real-history shared publication under a 2 GiB database budget

At commit `5773d04`, the production authenticated HTTP service successfully
published a frozen real-history snapshot to Qdrant. Redis was OOM-killed while
publishing the same snapshot under the same database memory limit. This supports
keeping Qdrant as the default, but does not establish a general backend winner
or complete the `mash-dna` evaluation.

## Corpus and method

- Xenia: Linux x86-64 KVM, 16 exposed AMD EPYC 9645 CPU threads. Host load was
  uncontrolled; the load average near completion was approximately 16.8/14.3/12.7.
- Fresh optimized release binary built with `cargo build-clif --release`.
  The release profile uses LLVM optimization; Cranelift remains the development
  iteration option.
- Frozen accepted contributions from schema-34 SQLite: 789 conversations
  (515 Claude Code, 274 Codex), 225,267 canonical messages, 29,453 compatible
  384-dimensional signed-byte vectors, and 409,479,397 characters of canonical
  message content. SQLite `length(TEXT)` counts characters, not UTF-8 bytes.
- Read-only SQLite transaction; bounded NDJSON batches of at most 128 records
  or approximately 1 MiB, with canonical content, search projections, metadata,
  and native context-parent edges preserved. No vector-only corpus substitution.
- Wire body: **550,139,311 bytes**. SHA-256:
  `a6a1909d299d8623f26e12d639734cf13e73c7d09654280658ab95bfb4351eb4`.
- Qdrant 1.19.1 and Redis 8.2: separate disposable evaluation containers, each
  limited to 4 CPU cores and 2 GiB RAM. Qdrant used cached placement and exact
  retrieval; Redis used FLAT/exact. Qdrant had a small pre-existing smoke
  collection; Redis was freshly started with persistence disabled.
- Each backend received a new isolated group through production `mash serve`
  with bearer authentication. The configured node snapshot cap was explicitly
  raised from its 64 MiB default to 2,048 MiB. The API process itself was on the
  host without a separate memory limit.
- One initial publication per backend, Qdrant followed by Redis. A Qdrant
  candidate-search diagnostic began near the end of Redis publication; this
  overlap and uncontrolled host load prevent a controlled throughput comparison.

The frozen contributions come from a refresh that remained incomplete because
one native history record is malformed. They are suitable for measuring storage
publication of accepted data. This run does **not** prove the full native corpus
is ready, repair that record, or change any local readiness marker. Original
SQLite and provider histories were opened read-only.

## Publication result

| Backend | Transfer to API | Upload through publication | Result | Observed API peak RSS |
| --- | ---: | ---: | --- | ---: |
| Qdrant cached/exact | 2.455 s | 95.146 s | HTTP 200, revision 1 | 1,246,572 KiB |
| Redis FLAT | 1.953 s | 27.946 s until failure | Retryable HTTP 503; container OOM | 1,245,784 KiB |

Qdrant's response confirmed all 789 conversations, 225,267 messages, and 29,453
searchable messages/embeddings. The measured publication rate was about 2,368
canonical messages per second, including upload, validation, snapshot staging,
and active-head publication. This is one observation, not a percentile estimate.

Docker confirmed Redis terminated with `OOMKilled=true` and exit code 137.
Its failure duration is not an ingestion-speed result. Redis with a larger
memory budget has not been tested here.

Periodic Docker samples observed Qdrant at **1.944 GiB** and Redis at
**1.658 GiB** before the latter died. These are sampled working-set readings,
not exact peaks, allocation totals, or steady-state resource requirements.
Qdrant succeeded close to the database budget; this run does not establish
comfortable headroom for concurrent publishers or larger groups.

The API high-water marks show that bounded network batches alone do not bound
publication memory. Both adapters build canonical snapshot state, and Qdrant
also serializes the full snapshot for immutable storage. Follow-up
`mash-c0d.21` tracks publication allocation and admission bounds while preserving
canonical context, atomic revisions, and replacement/deletion correctness.

## Candidate API diagnostic after publication

The Qdrant service was reopened against the same published group. A persistent
HTTP connection made 50 requests per scope and result unit, 300 requests total.
Each scope reused the same 50 distinct real document vectors and ten diagnostic
lexical strings. Provider constraints were checked on both candidate lists.
No other-node exclusion was requested because the group contained one node.

| Unit | Provider scope | p50 | p95 | p99 |
| --- | --- | ---: | ---: | ---: |
| Messages | All | 40.09 ms | 81.05 ms | 2,032.48 ms |
| Messages | Claude Code | 36.31 ms | 60.78 ms | 91.86 ms |
| Messages | Codex | 33.35 ms | 45.99 ms | 61.40 ms |
| Threads | All | 71.55 ms | 160.16 ms | 190.70 ms |
| Threads | Claude Code | 75.74 ms | 125.34 ms | 210.35 ms |
| Threads | Codex | 71.11 ms | 154.45 ms | 181.27 ms |

There was no discarded warm-up. The first all-provider message request took
2,032.48 ms and is included in the table. With 50 samples, p99 is the maximum.
Qdrant optimization/page-cache state and host contention were not controlled.

These are **candidate API latencies**, including HTTP response transfer and
decoding. Query vectors came from stored documents and include self matches;
they are not embeddings of the diagnostic strings. This is not a natural-language
relevance evaluation, ANN recall comparison, or end-to-end CLI/MCP result latency.
Model inference, local retrieval, fusion, and reranking are excluded. Redis has
no comparable search result because its publication failed.

## Evidence and remaining work

Private artifacts are under
`/home/james/scratch/mash-artifacts/shared-publication-20261001`: read-only spool
construction and API scripts, corpus hash/counts, per-backend publication
responses, resource samples, terminal Redis state, all candidate timings,
collection settings/counts, and cleanup verification. The spool contains private
history and remains outside Git. Only the benchmark's two owned Qdrant
collections were deleted; the pre-existing smoke collection was preserved.
The Qdrant evaluation container was stopped; Redis was already terminal.

`mash-dna` remains in progress. Required further evidence includes cold placement,
ANN versus exact retrieval, larger-memory Redis and PostgreSQL baselines,
representative multi-node and compound scopes, real query embeddings with
reviewed relevance judgments, full local/shared fusion and reranking latency,
replacement/restart/concurrent publication costs, and reliable storage/RAM
measurements. These results do not spend the separate post-refresh end-to-end
search optimization cycle or resolve its native-corruption prerequisite.

## Follow-up: stream canonical serialization to a private file

The snapshot-spooling implementation accompanying this report replaces the full
JSON `String` allocation in both Qdrant and Redis publication with a private,
size-limited temporary file. Serialization and incremental BLAKE3 hashing run
on a blocking worker. Network staging reads bounded chunks; Qdrant chunks retain
valid UTF-8 boundaries, while Redis retains its exact binary chunk layout and
manifest format. Redis reuse decisions use the spool's digest. No history fields
or context edges are dropped, and active-head publication remains conditional.

Repeating Qdrant publication with the exact same frozen NDJSON body produced:

| Measurement | Before | With spooled serialization |
| --- | ---: | ---: |
| API process peak RSS | 1,246,572 KiB | 697,736 KiB |
| Upload through publication | 95.146 s | 86.141 s |
| Published messages / embeddings | 225,267 / 29,453 | 225,267 / 29,453 |
| Highest sampled database working set | 1.944 GiB | 1.971 GiB |

The API peak RSS decreased by **44.0%**, approximately 536 MiB, in this initial
publication. Runtime was lower in this observation, but there is only one run
per implementation and host load was uncontrolled; this is not proof of a
throughput improvement. Early follow-up publication overlapped doctest
compilation. Both runs used cached/exact Qdrant with the same 4-core/2-GiB
database limits and a host API process without a memory limit.

This change trades a full heap buffer for temporary file I/O and reclaimable
filesystem cache. It does not eliminate the in-memory validated snapshot,
replacement-read buffers, backend payload/index allocations, or the risk from
concurrent large publishers. Qdrant still approached its database memory limit.
Redis's database OOM was not retried at the same insufficient budget; live Redis
tests verify chunk manifests, digest-based reuse, replacement, and forgetting,
but its real-corpus resource improvement remains unmeasured.

Validation: 550 tests passed, including explicit real-model, PostgreSQL, Redis,
Qdrant, CLI, and MCP tests; 10 manual benchmarks were excluded. The new encoding
test checks exact serialized bytes and digest, size limits, and UTF-8 chunk
boundaries. A live Qdrant regression checks multi-chunk canonical content and
serialization-budget rejection without replacing the active head. Clippy with
all targets/features, formatting, doctests, and whitespace checks passed.

Follow-up artifacts are in
`/home/james/scratch/mash-artifacts/shared-publication-spooled-20261001`, including
the binary hash, publication response, resource samples, comparison, collection
counts, and cleanup evidence. Both owned collections were removed and the
evaluation container was stopped. `mash-c0d.21` remains in progress for
replacement/concurrent memory measurements and publication admission bounds.
