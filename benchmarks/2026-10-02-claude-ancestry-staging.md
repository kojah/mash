# Exact Claude ancestry staging

## Change and preserved behavior

The [whole-index control](2026-10-02-claude-ancestry-memory.md) showed growing
memory with operational-event ancestry while searchable dialogue stayed fixed.
Claude ingestion now uses a concrete `Parents` store with an inline accounting
budget of 2 MiB, then exact private scratch SQLite lookup. Accounting includes
UUID/parent byte lengths and a conservative per-entry allowance; it is not a
hard process RSS limit. Replacing an existing key adjusts the accounting rather
than counting it as a new entry.

Missing parent fields, explicit roots, known parents and ambiguous ancestry stay
distinct. Keys and parents use exact UTF-8 bytes, without case folding, Unicode
normalization or truncation. Compaction replay still marks reparented native UUIDs
ambiguous and restores original parents only when the existing uninterrupted
native boundary proof passes. A read, write, migration or decode failure propagates
as a source error. Failed writes poison the store and remove its scratch state.

Parent traversal uses Brent cycle detection instead of retaining every traversed
UUID. It keeps two persistent UUID strings and counters, follows the same path in
order, and preserves early stops at retained dialogue, unknown ancestors, roots,
missing links and ambiguity. This bounds retained traversal state independently
of chain length, not the length of an individual externally supplied UUID.

Scratch storage uses the existing private scratch utilities, owner-only Unix
directory/file permissions, a 1 MiB SQLite page-cache setting, no mmap, and one
transaction. It is disposable parsing state with journaling/sync disabled; it is
never the published index and is never recovered as a valid source snapshot.
Connection closure precedes directory cleanup. Spill traces contain entry count
and inline budget, without native UUIDs, source contents or paths.

## Matched full local indexing control

The same sampler and native input hashes were used before and after this change,
on Xenia with installed local models and a Cranelift debug binary. There was one
run per case, in the same order, without warmup or replication. Original baseline
source HEAD was `8fa17820b5348a1d4fda1f9229ee4cee80e87296`; the staged run used this
change on top of `e1c4c82a99e225ddd43090a9da18a4545fbb910f`.

| Operational events per source | Sources | Before RSS MiB | Staged RSS MiB | Before elapsed s | Staged elapsed s | Observed spill stores |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 1 | 342.92 | 341.19 | 1.995 | 2.207 | 0 |
| 100,000 | 1 | 361.94 | 345.49 | 2.974 | 4.348 | 1 |
| 300,000 | 1 | 401.60 | 347.82 | 5.414 | 7.460 | 1 |
| 100,000 | 2 | 381.50 | 348.13 | 3.434 | 3.850 | 2 |

RSS values are current-image externally sampled `smaps_rollup` maxima, not hard
bounds; MiB = KiB / 1024. In the 300,000-event case, observed peak RSS fell by
53.78 MiB and elapsed time rose by about 38%. The staged single-source growth from
zero to 300,000 events was 6.63 MiB, versus 58.68 MiB in the original control.
That is evidence of the intended retention improvement, not an attribution of
every byte to ancestry or a prediction for real session histories.

Staged user/system CPU seconds were respectively 1.696/0.296, 3.794/0.239,
6.788/0.341, and 5.013/0.256. Staged sampled peak PSS was 316.38, 320.32, 322.78,
and 322.67 MiB. PSS depends on sharing with other processes and is not treated as
a fixed mapping of parser memory. Two-source admission diagnostics again recorded
`peak_active_sources=2`. The timing difference between single- and two-source
cases is not sufficient evidence of throughput scaling.

All cases exited successfully and retained exactly one canonical conversation,
message and vector per source. Before/after comparisons verified identical native
file sizes/hashes, canonical message digests, and complete stored embedding rows
including message ID, generation, dimensions, norm and vector bytes. Every
nonempty operational chain recorded one actual spill per source; the zero-event
case did not spill.

## Correctness checks and remaining scope

- Store tests cover all four ancestry states, replacements, exact keys including
  null bytes and Unicode variants, migration, bounded page-cache configuration,
  private permissions, cleanup, poisoned writes, invalid stored states/UTF-8 and
  read failures. Read failures are errors rather than missing-ancestor fallbacks.
- Proven/unproven native compaction restoration runs against both inline and
  forced-disk stores.
- An independent copy of the previous visited-set traversal is the oracle for
  343 exhaustive three-node graphs, all starts, with/without a dialogue stop.
  It compares resolved IDs/positions and error messages against the production
  traversal after crossing the real spill threshold.
- A native-shaped ingestion fixture crosses the production threshold with
  20,000 operational events, performs compaction replay, commits SQLite, reopens
  it and verifies retained context, message count and checkpoint skip behavior.
- The complete standard suite passed: 580 tests, including existing ownership,
  conflicts, rollback/retry, session transitions, CLI/MCP and watcher checks.
  Final focused validation passed 26 Claude checks and the installed-model CLI
  indexing/hybrid-search test (27 total). Clippy with warnings denied, formatting
  and the single doctest also passed.

The control measures complete local indexing but deliberately varies operational
ancestry only. It does not prove a whole-job memory cap, representative ingestion
throughput, fleet behavior, or large-dialogue/replay bounds. Dialogue key vectors,
message identity maps, reconciliation lineage, replay duplicate/change metadata
and remaining native adapters still need bounded handling and validation. This
change does not close the parent memory requirement.

Artifacts are outside Git:
`/home/james/scratch/mash-artifacts/claude-ancestry-staged-20261002/capture.json`
and its per-case logs, native inputs, databases and timestamped memory samples.
The adjacent `comparison.json` records verified before/after capture hashes,
canonical/full-embedding parity, spill counts and observed concurrency.
Reproduce with the command in the baseline report, choosing a fresh output
directory. Sampler SHA-256 is unchanged:
`ce35db21ec516e1b045662adedbb05655a8a5715e59313e48fe3a5e6e5f1f502`.
Staged binary SHA-256:
`9af0853c6f84945ad8d17d0e4f4ef9e3856ced03361226ff416c064ba58f6c9b`.
The sampler checks binary integrity after the matrix. Tracing/procfs sampling
perturb timing; sampled peaks exclude the observer and children; retained rusage
high-water marks include pre-exec lifetime and are not subtracted.

Tracking: `mash-c0d.7.4.3.4.4`; parent `mash-c0d.7.4.3.4` stays open.
