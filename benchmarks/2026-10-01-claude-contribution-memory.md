# Streaming Claude contribution decoding

Physical Claude source snapshots can contain multiple copies of the same native
messages. Previously, canonical reconciliation first collected all generated
SQLite contribution rows, including their content and search projections, then
deduplicated them. It now feeds typed rows directly into reconciliation, dropping
matching copies after validation. Decoding failures propagate through the
fallible contribution iterator; no partial conversation is returned.

## Controlled phase measurement

Measured on Xenia with the Cranelift unit-test executable built from `19aeb36`
plus this change. Each measurement ran in a separate process; fixture creation
and compilation were outside `/usr/bin/time` peak-RSS measurement.

The fixture uses the production source/contribution table definitions and
indexes: eight physical sources, each containing the same 2,048 native messages
with a linear ancestry chain. Each content is 32 KiB, with an explicit short
search projection on every 32nd message. This represents 512 MiB of physical
content and 64 MiB of distinct canonical content. The SQLite file is
546,476,032 bytes. Both paths use the same generated query and reconciliation;
the baseline collects rows first, while the streamed path calls `claude_thread`,
including metadata selection and canonical timestamp derivation.

| Run | Collect rows first: peak RSS | Stream rows: peak RSS | Collect phase | Stream phase |
| --- | ---: | ---: | ---: | ---: |
| 1 | 563,008 KiB | 104,616 KiB | 1,527 ms | 1,498 ms |
| 2 | 562,960 KiB | 103,920 KiB | 1,410 ms | 1,365 ms |

All four runs retained 2,048 messages and produced the same digest,
`8ddfc776508712f5d73b079c8a969c70fd3c958c9455d38e6fd7b5ef43ece12b`, over every
message's identity, ordinal, context, role, canonical content, search projection,
timestamp and native parent identity. Peak RSS includes checksum generation;
the reported phase time stops before checksum generation.

The measured reduction is about 81%. These are contribution-loading measurements,
not whole-indexing-job RSS or end-to-end latency. Parser output and each distinct
canonical conversation still retain payloads in a vector/DAG. Disk-backed
normalized-message staging remains open under `mash-c0d.7.4.3`.

## Reproduction

Raw logs, fixture metadata/database and the exact test executable path are under
`/home/james/scratch/mash-artifacts/claude-contribution-memory-19aeb36`.
Run the ignored diagnostic explicitly against a pre-created fixture:

```bash
env MASH_BENCHMARK_DB=/path/to/fixture.sqlite3 \
    MASH_BENCHMARK_STATE_MODE=streamed \
    /usr/bin/time -f '%M' -o peak-rss-kib.txt \
    /path/to/current/mash-unit-test-executable \
    --exact storage::claude_sources::tests::memory::benchmark_claude_contribution_memory \
    --ignored --nocapture
```

Use `baseline` for the prior collecting algorithm. Do not measure through Cargo,
which would include compilation. Keep the fixture out of Git and exclude the
explicit diagnostic from routine test runs. Normal tests cover matching copies,
branch ancestry, selected-source metadata/projections, conflicting copies and
late decoding errors, including errors from an otherwise excluded source.

## Validation

The final Linux suite passed 429 tests with 10 explicit benchmarks skipped,
using `cargo test-clif --run-ignored all --no-fail-fast -j2 -E 'not test(benchmark)'`
and explicitly installed models plus disposable PostgreSQL/Redis services.
It includes CLI/MCP, watcher and provider contracts, source lifecycle/migration,
shared routed/snapshot publication and the new late-corruption rollback/retry
test. Clippy with warnings denied, formatting and doctests passed. Native Mac
validation is still a separate outstanding gate.

The first full run exposed the worker's hardcoded `/tmp` socket directory: this
host's `/tmp` ACL denies writes by the user, defeating the on-demand cache and
failing its usage-validation test. A separate fix makes the worker respect the
platform temporary directory (`TMPDIR` here); all four worker tests passed in
the final run. The suite elapsed 140.558 seconds while additional compiler checks
ran concurrently; this is not an isolated test-speed comparison.
