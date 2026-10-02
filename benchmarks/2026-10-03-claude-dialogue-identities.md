# Claude dialogue identity retention

`mash-c0d.7.4.3.4.6`, Xenia, 2026-10-03. This is a development metadata control,
not a whole-index memory or throughput result.

Previously, native dialogue UUIDs accumulated in a vector, then normalization
created a second tree mapping every UUID to a stable message ID and its first
physical position. Both structures remained live during reconciliation. The
new ledger keeps ordered optional UUIDs and exact first positions inline until
its 2 MiB retention accounting budget is exceeded. It then migrates to private
scratch SQLite with a 1 MiB page cache, no mmap and disk temporary storage.
Stable message IDs are derived when needed. The complete dialogue ledger is
available before normalization, preserving references to later physical rows.

The ledger retains every ordinal, including missing UUIDs. Duplicate UUIDs do
not replace their first position. Staging write/read failures abort the source;
partially failed writes poison and remove the store. Directories/files retain
the existing owner-only Unix permissions and are removed on completion/failure.
The store is disposable parsing state, not a recoverable publication database.

## Paired metadata control

The ignored `dialogue_identity_memory_control` runs either the previous vector
plus stable-ID/first-position tree or the new production ledger. Each arm creates
300,000 positions, omits every thirteenth UUID, and cycles other UUIDs over
250,000 values to exercise duplicates. It reads every ordered row and every
present first-position identity into a digest. There are no message payloads,
models, index transactions or shared requests. Both arms use the same frozen
Cranelift development test binary, sequentially, in reference/staged then
staged/reference order. No owned compilation was running during measurement;
other host load was uncontrolled. `/usr/bin/time` measures each complete process.

| Run | Peak RSS, KiB | Elapsed, seconds | User / system CPU, seconds |
| --- | ---: | ---: | ---: |
| Reference first | 110,668 | 1.46 | 1.37 / .05 |
| Staged first | 40,380 | 4.86 | 4.68 / .10 |
| Staged repeat | 40,444 | 5.55 | 4.83 / .11 |
| Reference repeat | 110,876 | 2.07 | 1.46 / .08 |

Peak memory falls from about 108 MiB to 39.5 MiB, roughly 64%, in this workload.
The staged metadata control takes 2.7–3.3 times as long. These are two ordered
controls, not latency percentiles or a statistical confidence interval. They
include test-runtime startup and cannot isolate SQLite, hashing or allocator
cost. They establish neither a whole-job memory cap nor full ingestion overhead.
Lineage/reconciliation maps, spool offsets, model workspaces, concurrent sources
and other native adapters remain separate costs.

All four runs produce the same digest:
`ded993d6313612c7278e1552164e3f91963d64c90ec90c0a54f5e9c341b2ab5a`.
Logs, time counters, source hashes and frozen executable hash are retained in
`/home/james/scratch/mash-artifacts/claude-dialogue-keys-20261003`.
The measured source snapshots and executable are retained there. Subsequent
Clippy cleanup borrows the normalization ledger and uses `is_multiple_of(13)`
in the control's missing-key condition; the production ledger itself is
byte-identical to the measured source.

## Correctness scope

Forced inline/disk controls compare ordered keys and first positions for missing,
duplicate, empty, Unicode and embedded-NUL keys. Corrupt rows, removed tables,
partial insert failures and scratch permissions/cleanup have explicit checks.
The existing exhaustive 343-small-graph ancestry oracle now also forces the
dialogue lookup through disk storage.

A 12,000-message native-shaped JSONL transcript crosses the production dialogue
budget. Its additional forward-reference child and duplicate root preserve all
12,001 unique contents, stable IDs, lineage and parent ordinals. This sanitized
parser fixture is not a first-party live session or a semantic inference test.
Existing fork, compaction, conflicting identity and transactional indexing tests
remain required; this isolated diagnostic does not replace them.

The final standard Cranelift suite passed all 596 tests in 8.639 seconds;
115 opt-in tests were excluded. Two explicitly installed local-model checks
also passed in 3.236 seconds: core indexing trace correlation and MCP thread
discovery/scoped message search. Strict all-target Clippy, formatting, diff
checks and the single doctest passed. These checks
do not validate remote deployment or every excluded backend/model control.
