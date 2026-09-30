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

Pending-message planning and canonical conversation parsing still need aggregate
memory bounds, tracked in Beads `mash-c0d.7` and `mash-c0d.7.4`. This change does
not impose a global RSS ceiling.

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
