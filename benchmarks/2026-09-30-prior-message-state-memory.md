# Prior-message state memory during index refresh

The vector-reuse path retained each prior message's searchable text in a
`BTreeMap`. Its generated SQLite query first collected all decoded rows, so a
large changed conversation retained another full searchable-text copy before
reconciliation. The replacement reads generated typed rows one at a time and
stores a 32-byte BLAKE3 text fingerprint alongside ID, ordinal and canonical
fingerprint. Vector reuse compares that fingerprint with the incoming text.
The database schema, stored vectors and embedding generation are unchanged.

## Controlled phase measurement on Xenia

A separately created synthetic SQLite fixture contains 16,384 messages,
512 MiB of canonical text and 503,322,786 bytes (approximately 480 MiB) of
searchable text. Rows include absent, empty and nonempty projections. The
database file is 546,287,616 bytes. Fixture creation does not contribute to the
measured process peak.

Each measurement launches the same compiled benchmark binary in a fresh process.
The baseline reproduces the previous generated `query_many` plus text-retaining
map; the streamed branch calls the production `message_states` implementation.
`/usr/bin/time` records peak RSS outside Cargo. Both paths compute the same
digest over ordered IDs, ordinals, canonical fingerprints and searchable-text
fingerprints. Runs alternate baseline/streamed and have warm filesystem caches.

| Run | Peak RSS (KiB) | Phase time (ms) |
| --- | ---: | ---: |
| Baseline 1 | 529,296 | 779 |
| Streamed 1 | 37,636 | 391 |
| Baseline 2 | 529,272 | 752 |
| Streamed 2 | 37,472 | 397 |

Every run retained 16,384 identities and produced digest
`d3847ac1bea5e1118f929f31a33494f8a03bef877d598cb278c12d359c82c423`.
Peak RSS fell approximately 93%, from 517 MiB to 37 MiB, for this phase and
fixture. Two observations per path do not establish latency percentiles or
whole-job throughput. The benchmark's elapsed time includes digest comparison;
the production path does not compute that aggregate digest.

Raw fixture metadata, per-process logs, RSS results and full-suite output are at
`/home/james/scratch/mash-artifacts/prior-state-memory-f598690`.
The explicit ignored diagnostic is
`storage::indexing::memory::benchmark_prior_message_state_memory`; it requires
`MASH_BENCHMARK_DB` and `MASH_BENCHMARK_STATE_MODE=baseline|streamed`.
Use a standalone test binary when measuring RSS, so Cargo/compiler memory does
not hide the lower peak. Do not include this phase diagnostic in ordinary CI.

## Preservation and remaining scope

Regression coverage checks that changed canonical context and timestamps retain
vectors when the search projection is unchanged; changed, empty and restored
projections correctly invalidate or queue embeddings. Existing ordinal/ancestry
and source-contribution tests preserve surviving vectors and branch context.
Validation passed 426 tests in the full Cranelift suite (9 skipped), including
fresh installed models, CLI/MCP and disposable PostgreSQL/Redis contracts.
The final 54 storage/Claude checks also passed after a field-name/comment
cleanup. Clippy with warnings denied, formatting and the doctest passed.

The prior-state map still scales with message count. Parsers retain whole
normalized conversations, and Claude reconciliation still materializes source
messages and a native ancestry DAG. This change removes one text-sized copy
during refresh; it does not establish a hard total RSS bound or solve initial
large-conversation indexing. Bead `mash-c0d.7.4.3` remains open for disk-backed
conversation staging.
