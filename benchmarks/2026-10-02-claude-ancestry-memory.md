# Whole-index Claude ancestry memory control

## Question and setup

Does native operational ancestry increase whole-job memory even when searchable
dialogue stays fixed? This is a synthetic Linux control, not a representative
history benchmark or a production throughput gate.

`tools/benchmark-ingestion-memory.py` generates a fixed four-case matrix. Each
source contains a chain of native Claude `progress` UUIDs followed by one user
message whose parent is the last operational event. Dropping these events before
ancestry resolution would change the tested contract. Each case runs the actual
`mash index` command against a fresh local SQLite database with installed local
models, source processing, inference, and persistence. Tracing is enabled in all
four cases. Sources use distinct sessions; dialogue text is identical.

The run used source HEAD `8fa17820b5348a1d4fda1f9229ee4cee80e87296`, the existing
Cranelift debug binary, Python 3.13.5, and Linux 6.12.101 on Xenia. It is a single
run per case in the order below, without warmup/repeats or throughput claims.

| Operational events per source | Sources | Input bytes | Peak active parsers | Sampled RSS MiB | Sampled PSS MiB | Elapsed s | User/system CPU s |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 1 | 163 | 1 | 342.92 | 339.56 | 1.995 | 1.604 / 0.202 |
| 100,000 | 1 | 10,500,163 | 1 | 361.94 | 358.58 | 2.974 | 2.372 / 0.267 |
| 300,000 | 1 | 31,500,163 | 1 | 401.60 | 398.24 | 5.414 | 4.450 / 0.357 |
| 100,000 | 2 | 21,000,326 | 2 | 381.50 | 378.05 | 3.434 | 3.690 / 0.288 |

RSS/PSS are external `smaps_rollup` sampled maxima; MiB = KiB / 1024. Sampling
targets only the direct indexing process while its executable matches the mash
binary, at a requested 20 ms interval plus observation overhead. Parser concurrency
comes from the actual terminal admission diagnostics, not from source count.
The two-source case reserved an estimated peak 86,098,456 bytes under the
134,217,728-byte admission budget. That reservation is not an RSS limit.

All four commands exited successfully. Every source produced exactly one stored
conversation, one canonical user message and one vector. Canonical content was
checked against the generated source. The three single-source sorted
`(id, role, content)` digests were identical:
`f682752032d57fe924dbc263075305e974bbd04b4a3e02f27f31819f72776d95`.
The two-source case produced two distinct message identities.

## Interpretation and next implementation scope

Sampled peak RSS rises by about 58.68 MiB between zero and 300,000 operational
events with searchable dialogue held constant. This is evidence of a whole-job
growth problem worth addressing, not an allocator attribution or a per-record
memory coefficient. The two-source control confirms that admission permits
overlapping parsers for this workload.

Source inspection identifies an unbounded ancestry map in
`app/ingestion/parsers/claude.rs`: `Identity.parents` retains native UUIDs and
parent links. `parse_jsonl` observes records before extracting dialogue, so the
map also contains operational events. `resolve_parent` additionally retains a
visited set while walking a chain. Compaction replay retains duplicate/change
metadata in `app/ingestion/parsers/claude/replay.rs`.

The next implementation should stage exact ancestry lookup and traversal state
once a bounded inline budget is exceeded, using private scratch storage. Preserve
root/missing/ambiguous distinctions, duplicate/reparent detection, proven native
compaction restoration, cycle detection and session transitions. I/O failures must
prevent publication of an incomplete snapshot. Re-run this same control after
that change, alongside actual replay/lifecycle checks. Dialogue keys, identity
maps, reconciliation lineage, replay metadata, and other providers remain separate
parts of the broader memory requirement; staging just the parent map cannot close
that requirement.

## Reproduction and evidence

Run with models already installed through `mash models install`:

```bash
env TMPDIR=/home/james/scratch/achi-tmp PYTHONDONTWRITEBYTECODE=1 \
  python3 tools/benchmark-ingestion-memory.py \
  --binary /home/james/scratch/achi-targets/integrated/debug/mash \
  --models /home/james/.local/share/mash/models \
  --output /home/james/scratch/mash-artifacts/claude-ancestry-memory-NEW-RUN
```

Use a fresh output directory outside Git. Original evidence is retained in
`/home/james/scratch/mash-artifacts/claude-ancestry-memory-20261002/capture.json`,
with per-case native input hashes, input files, command output, traces, databases,
canonical digests, admission snapshots and timestamped memory samples. The
adjacent `.log` records terminal case summaries.

Binary SHA-256:
`c1f07ccce9edaf948511628b239d9d79f4280cd5b0b5b2120523e7e3f943ff25`.
Sampler SHA-256:
`ce35db21ec516e1b045662adedbb05655a8a5715e59313e48fe3a5e6e5f1f502`.
The binary hash was checked again after the matrix.

Samples are lower bounds on peak memory, exclude child processes and the observer,
and do not establish model-versus-parser stage attribution. Status RSS/HWM
counters are approximate. The retained `wait4` high-water marks include the
pre-exec process lifetime and are not subtracted or treated as stage peaks.
Tracing, procfs sampling, scheduler/cache state and a debug build affect timing.
There is no fleet result, hard memory cap, topic-performance result, or claim that
operational-only ancestry covers large dialogue/replay histories.

Tracking: `mash-c0d.7.4.3.4.3` (measurement); parent
`mash-c0d.7.4.3.4` remains open for implementation and full memory validation.
