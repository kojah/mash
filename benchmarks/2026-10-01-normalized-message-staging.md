# Normalized message staging

Measured on Xenia with Cranelift development binaries, comparing commit
`3587769` against the staged-message implementation. Each run indexed a fresh
SQLite database from the same Claude JSONL file: 4,096 native messages with
32,766 bytes of context-only content each (128 MiB canonical payload).
No embeddings were generated, so this isolates parsing, reconciliation and
storage rather than inference throughput.

| Implementation | Peak process RSS | CLI elapsed |
| --- | ---: | ---: |
| Baseline, repeat 1 | 307.5 MiB | 3.56 s |
| Baseline, repeat 2 | 307.6 MiB | 3.36 s |
| Baseline, repeat 3 | 307.8 MiB | 3.53 s |
| Staging, repeat 1 | 79.0 MiB | 5.48 s |
| Staging, repeat 2 | 79.3 MiB | 5.33 s |

Staging reduces peak RSS by about 74% in this fixture and adds about 1.9 seconds
of disk I/O and checksum work. Small conversations remain inline. Cached
summaries avoid rescanning staged content just to calculate titles, timestamps
and searchable counts. This is a memory bound on retained normalized payloads,
not a total process-memory ceiling or a search-latency measurement.

Canonical conversations, messages, parent relationships, source contributions,
fingerprints and checkpoints produce identical SHA-256 digests for the baseline
and staged databases: `5c4c0507eb58b5627bc242a27c64a201e780d74c3e3ca12e8c5858629e8e4771`.
The full Linux suite passed 438 tests, including real models, PostgreSQL and
Redis; ten manual benchmarks were excluded. Fault tests cover truncation,
checksums, invalid framing, write/flush failure, cleanup, vector reuse and
unchanged shared batch composition.

Artifacts and scripts are outside Git under
`/home/james/scratch/mash-artifacts/message-staging-3587769`:
`summary-repeats.json`, `final-digests.json`, the original binary, fixture and
per-run stdout/resource measurements. Runs used `index --all-history` with the
fixture's local-only TOML configuration, explicitly installed models,
`TMPDIR=/home/james/scratch/achi-tmp`, and `/usr/bin/time` for peak RSS.

Remaining work: metadata grows with the number of messages, native adapters
may retain their replay buffers before normalization, and MacBook/fleet
measurements have not been repeated for this change. These remain tracked in
`mash-c0d.7.4.3.4` and the parent indexing work.
