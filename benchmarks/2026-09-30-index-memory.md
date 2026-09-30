# CPU indexing memory: 2026-09-30

A live 90-day incremental refresh reached 4,133,680 KiB peak RSS. A separate
frozen embedding-only workload reproduced the problem without parsing histories:
model loading peaked at 346,796 KiB, inference at 4,496,972 KiB. This attributes
the observed large peak to embedding inference; it does not establish a bound
for all ingestion workloads.

## Controlled comparison

Both policies used eight CPU models, two threads per model, and batches of eight
with the installed `all-MiniLM-L6-v2` dynamically quantized model. The frozen
inputs and their order were identical. Two alternating runs per policy used
optimized LLVM builds on Xenia. Compilation was completed before resource
measurement. Peak RSS was recorded with `/usr/bin/time -v`; timings below exclude
model loading and database writes.

| Frozen workload | Previous parallel policy | Bounded long-batch policy |
| --- | --- | --- |
| 289 unique texts from the 512 longest messages: median inference | 4.510 s | 7.462 s |
| Same long inputs: peak RSS across the two runs | 4.07 GiB | 1.35 GiB |
| 1,023 unique texts from 1,024 messages of 64–400 characters: median inference | 1.293 s | 1.337 s |
| Same ordinary inputs: peak RSS across the two runs | 0.512 GiB | 0.518 GiB |

All vector comparisons reported zero difference. The short workload was about
3.4% slower in this small sample; timing noise and other concurrent work limit
that comparison. The long workload was about 65% slower, with about 67% lower
peak memory. These are inference diagnostics, not search latency measurements
or a fleet-wide benchmark.

Reducing the quantization batch from eight to two lowered memory, but changed
vector components by up to 0.0221 and failed the diagnostic. Fewer models also
reduced memory while slowing inference. Disabling the CPU arena retained equal
vectors but still used approximately 2.5 GiB and had variable throughput.

## Scheduling

The new policy keeps every quantization batch intact. It checks tokenizer lengths
before inference, routes batches over 128 tokens to two reserved workers, and
allows short batches to use all eight workers. Inputs over 4,096 bytes are
conservatively routed to the long workers without a second full tokenization.
Only those two models can accumulate long-sequence workspaces. Results are
restored to input order before vector storage.

The model, token limit, thread count, batch membership, quantization and embedding
generation are unchanged. Native Apple Silicon indexing continues to use Core ML.
The search worker does not use this indexing scheduler.

Pending-message planning now uses a private temporary SQLite file. It retains
one inference wave of text and at most 1,024 duplicate message IDs per write.
Text deduplication, global byte-length/binary-text order, quantization batches and
checkpoint boundaries are preserved. A digest match verifies complete ordered
texts and message IDs, including duplicates. Planning failure leaves pending
work and existing vectors intact. Temporary files are removed on normal exit.

An optimized, separate-process diagnostic compared the former in-memory plan
with the disk plan; no models were loaded during these measurements:

| Frozen workload | In-memory plan | Disk plan |
| --- | --- | --- |
| 8,192 synthetic unique messages, 596,451,328 text bytes: peak RSS | 586.85 MiB | 37.86 MiB |
| Same synthetic workload: plan and ordered digest time | 1.325 s | 3.380 s |
| Real corpus, 3,240 messages / 2,802 unique texts: peak RSS | 27.93 MiB | 33.29 MiB |
| Same real sample: plan and ordered digest time | 43 ms | 209 ms |

Both workloads produced identical complete ordered digests. These are single
controlled samples, not fleet throughput or a global memory ceiling. The disk
plan trades temporary disk writes and fixed SQLite overhead for memory that no
longer scales with all pending text. Canonical conversation parsing and parser
queue retention still need aggregate bounds, tracked in `mash-c0d.7.4`.

A production CLI replay of the real sample stored all 3,240 vectors with 2,802
model inferences and 438 duplicate reuses. It finished in 66.75 seconds, with
1,506,176 KiB peak RSS during inference, retained all 188,028 canonical messages,
left no pending embeddings, and passed SQLite integrity checks. A separate
in-memory-plan inference replay of the identical frozen pending set checked
every stored vector byte and norm for exact equality; all passed. An older,
independently prepared snapshot had 16 different vectors and therefore was not
used as the same-input compatibility baseline.

To repeat this planning-only diagnostic, use a schema-current frozen snapshot
with a nonempty pending set. It is opened read-only and need not be search-ready:

```bash
MASH_TEST_SEARCH_DB=/path/to/frozen/index.sqlite3 \
MASH_BENCH_PLAN_POLICY=memory \
MASH_BENCH_PLAN_REFERENCE=/path/to/ordered-digest.txt \
  cargo test-clif --release --run-ignored only \
  -E 'test(benchmark_embedding_plan_memory)' --success-output immediate
```

Run `memory` first, then `disk` with the same reference. For peak RSS, compile
first and invoke the selected test binary directly under `/usr/bin/time -v`;
including Cargo would also measure compiler and runner memory.

The existing `benchmark_embedding_worker_budget` diagnostic also accepts
`MASH_BENCH_STORED_DB=/path/to/completed/replay.sqlite3` to compare its in-memory
plan's quantized vectors and norms with a completed CLI replay. Use the frozen
pending snapshot as `MASH_TEST_SEARCH_DB` and `MASH_BENCH_EMBED_POLICY=bounded`.
Its manual-test timeout is 180 seconds to accommodate thousands of long inputs;
ordinary correctness tests retain their existing timeout.

## Parser admission and diagnostics

Parser workers now reserve estimated retention before reading a source and keep
that reservation through queued delivery and SQLite application. Admission uses
four times the discovered source-byte estimate plus 1 MiB, within a 128 MiB
reservation budget. Oversized sources occupy the whole budget and run alone;
valid histories are not rejected or truncated. Waiting requests are admitted in
ticket order so large sources cannot starve behind smaller ones.

This controls coexistence of estimated source allocations, not process RSS or
the size of a single canonical conversation. A live source may grow after
discovery, and provider normalization may differ from the estimate. Large
single-conversation staging remains tracked in `mash-c0d.7.4.3`.

Structured indexing progress now includes `parser_admission` with the budget,
estimated reserved bytes, active and waiting source counts, peak reservations,
and accumulated admission wait time. Reservations include results waiting in
the channel or being applied. These counters contain no history text or paths;
they flow through the existing CLI stderr and MCP progress mechanisms. Complete
request/stage tracing remains a separate task, `mash-c0d.9`.

Separate-process synthetic diagnostics retained every expected message:

| Workload / build | Unrestricted admission | 128 MiB admission |
| --- | --- | --- |
| Xenia, optimized: 8 files / 256 messages / 268,467,376 source bytes, peak RSS | 294.41 MiB | 66.79 MiB |
| Same Xenia sample: parse and storage time | 1.449 s | 2.278 s |
| Dev Mac, development build: 8 files / 64 messages / 67,116,800 bytes, peak RSS | 124.44 MiB | 75.63 MiB |
| Same Mac sample: parse and storage time | 2.571 s | 2.432 s |

These are single samples with different corpus sizes/builds, not comparable
cross-machine throughput. The Xenia budget admitted one large source at a time,
with seven requests waiting; its cumulative wait time was 6.799 seconds across
those requests. The throughput cost should be assessed alongside memory use.

An isolated real 90-day refresh scanned 778 sources / 2.345 GB and skipped 749
unchanged checkpoints. Ingestion took 3.375 seconds and sampled ingestion RSS
peaked at 114,548 KiB. Search rebuilding and full embedding work brought total
time to 101.33 seconds; inference had a separate 1,808,312 KiB sampled peak.
The refresh completed with no pending vectors and SQLite integrity `ok`, but it
exposed a Claude session-ID collision between physical histories. Investigation
confirmed the shorter source was a subagent transcript: its records share the
parent's session ID but carry a separate native `agentId`. The parser now uses
both identifiers for subagent thread identity. Schema 33 invalidates checkpoints
on explicit indexing; transactional source ownership repair supports either
parent-first or agent-first application to an older index.

A subsequent frozen replay of these two real files retained all 13,845 normalized
messages across both threads in 1.824 seconds for parsing/storage, without model
inference. The source files had continued to grow since the earlier snapshot;
this replay uses independent private copies rather than live hard links. Tests
also cover distinct agents, edits, deletion, checkpoint skips, migration from
the old parent ID, and conflicting native agent IDs. This verifies the observed
parent/subagent collision fix, not full-corpus preservation or reconciliation
of duplicate copies of the same logical thread. That remaining reconciliation
work is tracked in `mash-c0d.10.1`; the broader gate `mash-c0d.10` remains open.

The explicit `benchmark_parser_admission_retention` accepts
`MASH_BENCH_PARSER_ROOT`, `MASH_BENCH_PARSER_MESSAGES`, and
`MASH_BENCH_PARSER_POLICY=parallel|bounded`. Use an isolated synthetic root and
the exact expected message count. Compile before timing the selected test
binary with `/usr/bin/time`; no embedding models are loaded by this diagnostic.

## Repeating the diagnostic

Use an isolated, schema-current SQLite snapshot with installed models and an
explicitly prepared `pending_embeddings` set. Keep that snapshot immutable while
comparing settings. The diagnostic opens it read-only and stores only text hashes
and vectors in the reference file; no histories are printed.

```bash
MASH_TEST_MODELS_DIR=/path/to/installed/models \
MASH_TEST_SEARCH_DB=/path/to/frozen/index.sqlite3 \
MASH_BENCH_EMBED_REFERENCE=/path/to/reference.json \
MASH_BENCH_EMBED_POLICY=parallel \
  cargo test-clif --release --run-ignored only -j 2 \
  -E 'test(benchmark_embedding_worker_budget)' --success-output immediate
```

Run the parallel policy first to create the reference, then repeat with
`MASH_BENCH_EMBED_POLICY=bounded`. Keep workers eight, threads two and batch eight
for that policy. Separate processes avoid carrying model allocator caches from
one policy into the other. The parallel diagnostic also accepts
`MASH_BENCH_EMBED_WORKERS`, `MASH_BENCH_EMBED_THREADS`, `MASH_BENCH_EMBED_BATCH`,
and `MASH_BENCH_EMBED_ARENA` for experiments; these are test-only controls.

The diagnostic checks ordered text hashes, vector dimensions and component
differences with a tolerance of `1e-5`. Vector equality is a compatibility check,
not a relevance judgment. Use the labeled search matrix and end-to-end checks
alongside it.
