# Native identity staging

Compared commit `2642a6f` with exact inline-or-SQLite identity validation on
Xenia using Cranelift development binaries and fresh local indexes. Each native
Copilot CLI v1 fixture has a header, 300,000 operational delta events, and one
context-only message. No embeddings or model inferences were generated.

| Fixture / implementation | Peak RSS | CLI elapsed |
| --- | ---: | ---: |
| Ordered / baseline | 71.2 MiB | 2.633 s |
| Ordered / staged 1 | 45.3 MiB | 3.361 s |
| Ordered / staged 2 | 44.4 MiB | 3.545 s |
| Mixed / baseline | 70.5 MiB | 2.672 s |
| Mixed / staged | 44.4 MiB | 4.574 s |

Peak RSS fell about 36–38%. SQLite inserts add CPU/I/O, especially with mixed
IDs; this is a memory/throughput tradeoff. Small logs remain inline. IDs spill
above a conservative 2 MiB retention estimate. The disposable SQLite store uses
a 1 MiB page-cache target, disables memory mapping, and stores exact UTF-8 bytes
as BLOB keys. It uses no lossy hashes, normalization or probabilistic matching.
Operational and ephemeral identities are still validated.

Baseline and staged canonical conversations, messages, contexts, fingerprints
and checkpoints have identical digests for each fixture:

- Ordered: `c02b0b091fe984ce348bbf087f91aed9ad8cc279959cdad58bd6c83988ccd9d4`
- Mixed: `be6518adcd1ef3632d87cd06adbff6ca53721803346e91a804ff49d2d1f30da2`

Artifacts: `/home/james/scratch/mash-artifacts/copilot-identities-2642a6f`,
including baseline binary, fixtures, TOML configs, databases, resource/stdout
files and `results.json`. Ordered IDs are ascending 32-digit hexadecimal
integers. Mixed IDs use the reversible permutation
`(n * 0x9e3779b97f4a7c15d2b74407b1ce6e93 + 0x123456789abcdef) mod 2^128`.
Runs used `index --all-history` with explicitly installed models and
`TMPDIR=/home/james/scratch/achi-tmp`. `/usr/bin/time` measured peak RSS/wall time;
the table uses CLI elapsed time.

451 full Linux tests passed with real models, PostgreSQL and Redis; ten manual
benchmarks were excluded. Eight focused contracts passed again after stronger
byte-exact assertions. Clippy, formatting and doctests passed. Tests cover
spill-boundary duplicates, case/NUL/Unicode distinctions, cache settings,
cleanup, poisoned failed stores, and late ephemeral duplicates after 25,000
events preserving canonical data and checkpoints until repair.

A filesystem test found scratch directories inheriting mode 0775 on Xenia.
Sensitive embedding-plan and identity SQLite stores now share creation helpers
that create Unix directories as 0700 and files as 0600 before SQLite opens them.
Filesystem tests verify these modes. Creation refuses to overwrite files;
SQLite closes before the directory is removed.

Agent-head maps, Claude graph metadata, message offsets and other native replay
buffers remain in `mash-c0d.7.4.3.4`. This fixture does not establish a hard
whole-job memory ceiling, inference throughput, concurrency or MacBook results.
