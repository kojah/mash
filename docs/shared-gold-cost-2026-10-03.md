# Shared gold rebuild costs, 2026-10-03

This measurement exercises the existing explicit rebuild workflow rather than
staged all-node activation. Local and shared canonical history remain unchanged
while the shared build switches dense → lexical-only → dense. It is a bounded
single-node cost measurement, not a fleet, concurrency or semantic-quality gate.
No default configuration changes follow from it.

## Setup and procedure

`tools/benchmark-shared-gold.py` uses SQLite's backup API to copy the certified
25-thread, 6,137-message sample from the
[local representation experiment](local-gold-cost-2026-10-03.md). There are
1,061 searchable messages. The benchmark does not open live provider roots or
modify the installed index. Its node TOML has an empty `[providers]` table and
the required local SQLite index; shared inference uses already installed local
AllMiniLML6V2Q assets. No downloads, remote inference or LLM enrichment occur.

The API and Qdrant run on the same 16-CPU Linux node, over authenticated loopback
HTTP. Qdrant 1.19.1 uses a fresh disposable container and private persistent
storage. The API process restarts for each recipe with explicit
`serve --activate-gold` after the initial publication. Source-free replacements
use `index --rebuild-shared-gold`. Related search is unavailable during this
recipe transition until the explicit rebuild completes; this experiment does
not establish uninterrupted search availability.

The binary is the debug Cranelift build of `94c59c4`, SHA-256
`61b20df0e75f2cb2b1fc177456a0018d01b47f9169907cc312c113a446fea543`.
These timings must not be compared as algorithm speedups against the earlier
optimized LLVM local measurements. There was no concurrent compilation during
the run; other host workloads were not controlled. CPU count is 16 and the
captured platform is Linux 6.12.101, x86-64, glibc 2.41.

Before and after every build and query matrix, all ten local canonical tables
retain SHA-256
`dd22a1958591301fc20038a6a2714d6873a332eb029f33d81645202ffb735935`.
The shared `/v1/silver` history retains SHA-256
`303fa9a4b9987d592ad36e0120940427f02602ec5450129c9ad209c9bd571ac0`.
Shared silver revision, source publication counter, checkpoints, tombstones and
source certificate remain unchanged across replacements. Query-only operations
leave the entire `/v1/state` response unchanged. Full retained-history responses
are used only for verification outside measured build/query intervals.

## Build and publication

| Operation | Wall s | CLI CPU s | Sampled CLI RSS MiB | Model inferences |
| --- | ---: | ---: | ---: | ---: |
| Initial dense publication from copied local index | 10.989 | 1.467 | 53.4 | 0 |
| Replace shared gold with lexical-only | 11.979 | 1.195 | 63.6 | 0 |
| Replace shared gold with dense | 60.635 | 80.100 | 1,295.9 | 1,061 |

Initial publication forwards all 1,061 compatible local vectors, reports zero
provider files scanned, and sends 13,377,703 bytes. Local preparation takes
1.389 s; whole-operation time includes the HTTP capability/publication path.

Lexical-only replacement reports 2.989 s downloading retained silver and
8.881 s publishing, with zero model loading or inference. Dense replacement
reports 2.639 s downloading, 1.908 s loading models, 44.659 s embedding and
10.022 s publishing. Phase timing instrumentation and whole-process measurement
have different boundaries, so these numbers need not sum exactly.

The lexical-only result demonstrates that publication has substantial cost even
without model work. Inspection of `qdrant_index::publish::encode_publication`
and `stage_snapshot` confirms that a gold-only replacement serializes and stages
the joined silver/gold revision again. This is evidence for evaluating simpler
rebuild efficiency improvements, not evidence that an atomic replacement
framework is necessary or that Qdrant itself accounts for all publication time.

CPU and memory figures cover the CLI process only. API-server/Qdrant CPU,
memory, background optimization and subsequent reclamation are excluded.
RSS is externally sampled at 20 ms and is a lower bound. The separately captured
`wait4` peak can include pre-exec launch memory; do not subtract it from another
process's peak. These are indexing peaks, not idle-worker memory.

## HTTP-inclusive fresh-process searches

Six queries (`cbioportal`, `rust`, `qdrant`, `search`, `embedding`,
`configuration`) run three times for each retrieval/filter combination: 180
CLI processes overall. Each row has 18 observations. The provider filter is
either absent or `codex`; default thread shape and limit are retained. Every
search uses `--shared --no-worker`. Related discussion uses fusion ranking,
without reranking. Dense retrieval retains the server's exact default.

| Build stage | Intent | Provider filter | Median ms | Nearest-rank p95 ms |
| --- | --- | --- | ---: | ---: |
| Initial dense | Exact | None | 50.40 | 228.03 |
| Initial dense | Exact | Codex | 42.44 | 105.84 |
| Initial dense | Related/fusion | None | 576.60 | 805.32 |
| Initial dense | Related/fusion | Codex | 584.99 | 814.65 |
| Lexical-only | Exact | None | 57.73 | 251.94 |
| Lexical-only | Exact | Codex | 51.83 | 125.51 |
| Rebuilt dense | Exact | None | 52.47 | 253.35 |
| Rebuilt dense | Exact | Codex | 46.51 | 108.23 |
| Rebuilt dense | Related/fusion | None | 565.40 | 704.22 |
| Rebuilt dense | Related/fusion | Codex | 522.00 | 580.32 |

The small query set mixes hits and misses. `cbioportal` and `qdrant` return no
results in this convenience sample; it is not a cBioPortal quality benchmark.
Filesystem/model pages may be cached, even though CLI processes are fresh.
There is no resident-worker, simultaneous-reader/writer, HNSW or remote-network
comparison. Eighteen observations give a coarse p95, not a service SLO.

## Backend storage and remaining gates

After the three stages, allocated backend files occupy 64.906, 98.230 and
128.277 MiB. Their logical lengths are 1,819.980, 1,852.007 and 1,883.988 MiB,
dominated by sparse/preallocated files. At the last snapshot, WAL files account
for about 89.9 MiB allocated and segments for 38.3 MiB. These totals include WAL,
deleted records awaiting reclamation and whole-container overhead, rather than
unique canonical content or active gold size. They cannot establish that
lexical-only gold needs more space than dense gold. Qdrant's asynchronous vector
counters likewise are not the active manifest's embedding count.

Private raw responses, process logs, measurements and backend files reside at
`/home/james/scratch/mash-artifacts/shared-gold-cost-20261003/measured` and its
sibling backend directory. The harness records binary/model/script hashes and
Qdrant version. Failed preliminary harness attempts are retained separately;
only the completed `measured/capture.json` supplies the tables above.

Fleet/remote latency, server/backend resource accounting, steady-state storage
after compaction, incremental edits/deletes, concurrent readers/writers and
held-out relevance remain open acceptance work. Prefer those evidence gaps and
ordinary retrieval quality over expanding advanced activation machinery.
