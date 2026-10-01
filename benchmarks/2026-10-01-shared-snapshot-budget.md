# Size-weighted admission for canonical snapshot mutations

Redis/Qdrant mutation planning materializes canonical snapshots. The existing
`max_concurrent_uploads` limit gives a small and large mutation the same slot.
The new process-wide `max_mutation_snapshot_mib` budget also accounts for incoming
upload bytes plus the existing checksummed canonical payload before typed
decoding. Each reservation rounds that sum up to MiB, with a minimum of one MiB.
`forget` reserves the existing payload alone. Reservations follow blocking work
and stay held through publication/error cleanup; cancelled HTTP requesters do not
release them while the detached mutation task continues.

This is a **serialized-work budget**, not a hard RSS, temporary-disk or database
memory limit. Reader work, decoded representation overhead, allocator caches and
backend persistent/staging data still require capacity planning. PostgreSQL's
batched mutation path does not materialize these full canonical snapshots and
does not use this specific budget.

## Real two-node comparison

Both runs used a fresh isolated Qdrant 1.19.1 instance, cached placement, exact
retrieval, four CPUs and an **8-GiB** container limit. API processes had no memory
limit. Each run authorized three nodes in one group and retained the count limit
of **four**. Two nodes each uploaded the same frozen 550,139,311-byte real-history
body: 789 conversations, 225,267 messages and 29,453 searchable messages/vectors.
The second upload started two seconds after the first body's transfer finished.
Replicating one corpus makes upload weights comparable; these are not distinct
natural histories from two machines or a search-quality evaluation.

The baseline binary is `08087f1`, SHA-256
`fb6a084afd4034157d76d38a2dd08ccdf3b28058850181223030fbeca79bb774`. Its publication
and snapshot-planning code is unchanged through the pre-budget `3178556` commit.
The changed release binary SHA-256 is
`349d9ca8d1d5128d0ddeaef381e4ec27f7d4aeac0374b4ada4a7f7f87d711c74`.

| Measurement | Count-only baseline | 1,024-MiB snapshot-work budget |
| --- | ---: | ---: |
| First upload | 200, 120.9 s | 200, 80.8 s |
| Concurrent second attempt | 200, 124.6 s | Retryable 503, 1.61 s |
| Second retry after first completes | Not needed | 200, 83.6 s |
| Peak sampled API high-water RSS | 1,358,752 KiB | 692,920 KiB |
| Maximum sampled database memory | 1.617 GiB | 1.624 GiB |

Each upload rounds to **525 MiB**. Two such reservations exceed 1,024 MiB even
though the count limit permits both requests. The rejected attempt transferred
its body in 1.47 seconds and was refused before snapshot planning. Its node still
reported zero messages; retrying after capacity returned published all expected
counts with revision 1. Both baseline nodes also published successfully, and both
changed-run nodes ended with the same expected status counts.

Observed API peak RSS was **49.0% lower**, at the cost of serializing these large
publications: the changed run completed their accepted mutations in roughly
165 seconds, versus roughly 128 seconds for the baseline's overlapping pair.
The individual durations cannot establish a throughput improvement. Host load
was uncontrolled, and local compilation/tests overlapped part of the baseline;
these are single observations with sampled peaks. The budget limits active
serialized work, not retained allocator RSS or all possible representation
amplification. Database memory did not meaningfully fall in this sample.

The changed server was then restarted with a **32-MiB** work budget. A third
node's full-body upload returned nonretryable HTTP 400 in **2.45 seconds**,
including **2.31 seconds** of transfer. Its error named
`max_mutation_snapshot_mib`; the node remained empty, and both previously accepted
nodes' status counts were unchanged. No backend was OOM-killed. These observations
validate policy rejection/retry on this corpus, not a safe 2-GiB database limit
or automatic sizing for other datasets.

The corpus contains accepted contributions from an incomplete native refresh.
Original SQLite/provider histories remained read-only and readiness was not
changed. Earlier evidence of Redis OOM at a 2-GiB limit remains unresolved by
this Qdrant comparison; large Redis capacity still needs separate evaluation.

## Validation and remaining limits

All **567 tests passed**, including explicit real models, PostgreSQL, Redis,
Qdrant, CLI and MCP contracts; 10 manual benchmarks were excluded. Clippy with
all targets/features, doctests, formatting and whitespace checks passed.

Regressions cover rounded old-plus-new accounting across clones, overflow and
invalid limits, permanently oversized versus temporarily busy work, and worker
reservation lifetime after cancellation. Live Redis/Qdrant HTTP contracts check
unchanged revisions after rejection, pressure across two nodes, independent
canonical reads, successful later publication/forget, and capacity recovery after
planning errors. CLI startup rejects invalid limits before backend access. The
real release-server comparison additionally exercises the TOML setting and
weighted reservation during large async publication.

The default is 1,024 MiB; valid limits are 1–65,536. Configure this alongside the
per-node payload cap and mutation count limit. Larger full replacements reserve
both old and incoming bytes and may require an explicit increase. Inputs may be
privately spooled first; this budget does not limit all transport buffers, disk
usage or metadata/context readers. Budgets are independent across API processes.
The privacy-conscious `reserve_snapshot_work` span reports work/limit MiB and
accepted/busy/oversized outcomes.

Keep `mash-c0d.21` open for overall memory/resource guarantees, multiple-reader
workloads, cross-process/backend capacity and realistic Redis budgets. This
change provides enforced size-aware mutation admission without pretending that
serialized bytes equal process RAM.

Private artifacts under
`/home/james/scratch/mash-artifacts/shared-budget-20261001` contain the harness,
binary/config/body hashes, responses, resource samples, full-test log and cleanup
evidence. Full validation used a separate disposable Qdrant service. Each real
comparison backend and its anonymous volumes were removed after the request/API
handles completed; unrelated services were untouched.
