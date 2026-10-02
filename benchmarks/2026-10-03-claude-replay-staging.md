# Bounded Claude compaction replay metadata

Tracking: `mash-c0d.7.4.3.4.5`, within the still-open aggregate ingestion-memory
work. This changes parsing bookkeeping, not search ranking or topic selection.

## Behavior

The previous compaction replay tracker retained a `BTreeSet` of every duplicate
UUID and a `BTreeMap` of changed original parents. Both could grow throughout a
large duplicate block, even after native ancestry and message payloads spilled.
The tracker now shares the existing concrete `Parents` staging mechanism for
membership and saved changes: up to 2 MiB of inline accounting, then a private
SQLite store with a 1 MiB page-cache setting, no mmap, and disk-backed temp work.
This is a separate budget from the native ancestry store; neither is a process
RSS cap. No provider source or persistent index schema changes.

An ambiguous marker means a duplicate was seen without a saved parent change;
a known parent retains the first original state that can be restored. Further
copies cannot overwrite that saved state. Restoration still requires the exact
native boundary, logical parent, unreplayed anchor and preserved tail. It streams
one entry at a time into native ancestry without collecting the table in RAM.
A new event or the existing dialogue-interruption rule clears the replay block
and releases its store. Read/write/decoding or visitor failures abort the source;
partial restoration is never a published source snapshot.

## Synthetic measurement

Linux/Xenia, development Cranelift binary. The explicit ignored
`benchmark_claude_replay_metadata` runs either the previous in-memory reference
or the staged production tracker in the same frozen test binary. Both use the
same disk-spilling native ancestry implementation. It generates N operational
events pointing to one retained ancestor, a fresh anchor, N duplicate events
changing those parents to root, then a valid compaction boundary. It verifies
every restored parent and hashes every expected key/parent pair. It does not
retain dialogue payloads, load models or publish an index.

Initial single-process captures ran in order reference100k, staged100k,
staged300k, reference300k. A Cargo build ran concurrently, so elapsed times are
contended development diagnostics, not isolated throughput or production latency.
`/usr/bin/time` measures each test process directly, excluding Cargo/compiler
memory. CPU is user plus system time; RSS is the kernel process high-water mark,
including the linked test runtime and allocator retention. These are single
captures with no statistical confidence bound.

| Replay records | Tracker | Peak RSS, MiB | Elapsed, seconds | CPU, seconds |
| --- | --- | ---: | ---: | ---: |
| 100,000 | In-memory reference | 63.17 | 5.82 | 4.24 |
| 100,000 | Staged | 43.11 | 6.41 | 4.69 |
| 300,000 | In-memory reference | 110.14 | 12.87 | 11.80 |
| 300,000 | Staged | 42.42 | 18.37 | 14.35 |

Every parent-state assertion passed. Paired digests are identical:

- 100k: `568d7379293bca048d64d0f0cddf6bc0a2dc0a6c37504d976840773712193938`
- 300k: `fe0c2d1aa1e708fe5b5e486cf654edc7cfa12d22534c5131ba3765496f44f928`

The observation supports bounded replay-map retention at the cost of additional
SQLite work. It does not establish whole-ingestion throughput, whole-job memory,
fleet behavior, real-history accuracy or a universal 42 MiB process limit.

A 300k repeat after owned compilation and model checks completed gave:

| Tracker | Peak RSS, MiB | Elapsed, seconds | CPU, seconds |
| --- | ---: | ---: | ---: |
| In-memory reference | 110.48 | 11.34 | 11.07 |
| Staged | 42.86 | 12.79 | 12.39 |

The digest exactly matches both initial 300k captures. In this repeat, peak RSS
was about 61% lower, with about 13% more elapsed time and 12% more CPU. External
host load remained uncontrolled (one-minute load approximately 11.42 to 9.30);
this is one matched development comparison, not a population estimate or a
release-build ingestion target. Repeat logs/timings and
`claude-replay-staging-repeat-measurement-20261003.json` retain the measurements.

Private captures are under `/home/james/scratch/mash-artifacts` with prefix
`claude-replay-staging-` and suffix `20261003`: measurement JSON includes binary
and implementation hashes; per-mode/count logs and `.time` files retain test
results, elapsed, CPU and kernel RSS. To repeat, build/list the Cranelift lib test
binary and run it directly with `MASH_BENCH_REPLAY_MODE=reference` or `staged`,
`MASH_BENCH_REPLAY_COUNT=100000` or `300000`, and the persistent `TMPDIR`, using
`--exact ingestion::parsers::claude::replay::tests::benchmark_claude_replay_metadata
--ignored --nocapture`. Keep compiler/model/backend activity separate when
interpreting wall time.

## Correctness scope and remaining work

The per-arrival differential fixture compares with the old replay rules across
forced-disk and inline stores: unchanged/conflicting/repeated duplicates, a
replayed anchor, missing-UUID dialogue and operational events, matching and
mismatching tails. Store tests cover exact key order, visitor short-circuiting,
invalid UTF-8 keys/parents, corrupt states, read/write failure, and cleanup.

The native-shaped integration control now has 20,000 original operational
parents, a retained dialogue anchor, and 20,000 changed duplicate parents. Both
ancestry and replay cross their real production budgets. It indexes six canonical
messages, resolves the anchor through the restored operational chain, reopens
the persistent index and checks the unchanged source checkpoint. Existing
unproven-replay and payload-conflict rollback/retry controls also pass. Installed
local-model MCP thread/scoped-message and core index-tracing controls pass; they
exercise ordinary Claude ingestion, not this huge replay workload.

Dialogue key vectors, identity maps, reconciliation lineage, spool metadata and
other native adapters still have separate memory costs. The corrupt real Claude
source remains untouched and prevents complete real-corpus indexing validation.
The parent memory and real-corpus performance gates stay open.

Final checks: 590 standard tests passed in 9.238s (114 opt-in tests excluded),
strict all-target Clippy passed, formatting/diff checks passed, and one doctest
passed. Two additional installed-model controls above passed in separate test
processes. Final validation logs use the `claude-replay-staging-` prefix and
`20261003` suffix in the private artifact directory. These checks do not replace
the remaining fleet, complete-corpus or independent topic-quality gates.
