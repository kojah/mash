# Copilot CLI payload streaming

Compared commit `9ee6768` with streaming native CLI normalization on Xenia,
using Cranelift development binaries and a fresh SQLite database for each run.
The native v1 `events.jsonl` fixture contains a session header and 4,096
`user.message` events with 32,768 bytes each (128 MiB canonical text). Their
internal `source` keeps them context-only: zero embeddings or model inferences.
This measures the complete parsing/storage job without inference costs.

| Run | Peak process RSS | CLI elapsed |
| --- | ---: | ---: |
| Baseline 1 | 173.8 MiB | 2.951 s |
| Baseline 2 | 174.4 MiB | 2.580 s |
| Baseline 3 | 163.6 MiB | 2.357 s |
| Streaming 1 | 59.5 MiB | 2.480 s |
| Streaming 2 | 58.5 MiB | 2.452 s |

Peak RSS fell about 64–66%; elapsed times overlap the baseline range.
Streaming uses the existing 16 MiB inline-or-spooled message builder during
native parsing instead of retaining all bodies until the parser returns.
Per-agent context heads, clear semantics, event IDs, roles, search projections,
title selection and session timestamps remain unchanged.

All runs produce identical SHA-256 digests across canonical conversations,
messages, parent relationships, source fingerprints and checkpoints:
`6344a294d2ea18b92f67b0bb06dbef12632f80638beae5da1af80e12454cb655`.
The digest hashes table labels and length-delimited, ID-sorted JSON rows; it
excludes volatile index-run timings.

Validation: 446 full Linux contracts passed, including real models, PostgreSQL
and Redis; ten manual benchmarks were excluded. After extracting the new large
fixture into its own test module, all 13 Copilot tests passed again. Clippy,
formatting and doctests passed. The large fixture test checks spill activation,
human-only title selection, exact message IDs, context lanes, model-context
clearing, cleanup and rejection of a malformed final record after spilling.

Artifacts are under
`/home/james/scratch/mash-artifacts/copilot-cli-staging-9ee6768`, including
the original binary, native fixture, local-only TOML configuration, fresh
databases, per-run stdout/resource files and `results.json`. The CLI command
was `index --all-history --config node.toml --db RUN.sqlite3 --models-dir MODELS`
with explicitly installed models and `TMPDIR=/home/james/scratch/achi-tmp`.
`/usr/bin/time` measured peak RSS and wall time; the table reports the CLI's own
elapsed time.

This bounds retained message bodies, not total indexing memory. Event identity
sets and agent-head metadata still grow with event/lane count. Other native
replay adapters and MacBook/concurrent-source measurements remain in
`mash-c0d.7.4.3.4`; this fixture does not establish inference throughput or a
hard whole-job memory ceiling.
