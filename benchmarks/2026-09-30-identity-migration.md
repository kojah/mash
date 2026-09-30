# Identity-only migration and local search measurements

Measured on Xenia using a frozen schema-32 local index with 177,170 messages
and 23,792 vectors. This stored snapshot predates Claude identity repairs;
these measurements do not establish complete history preservation or relevance.
Private raw artifacts are in `mash-artifacts/search-perf-96e07f7` outside Git.

## Explicit schema upgrade

An identity-only upgrade previously invalidated all derived search data. Running
explicit `index --all-history` with empty provider roots took 157.9 seconds,
including 152.0 seconds of embedding work and 22,578 model inferences.

The targeted schema 32-to-33 migration invalidates only Claude checkpoints.
The same explicit command on a fresh copy took 189.6 ms with zero model
inferences. All 23,792 vector byte sequences, norms, dimensions, generations
and message IDs were unchanged; the full vector fingerprint was
`e6da3c9aea288fc68ffa5d2c45fa62385a87f892498de2a4e9aab978a567d86b`.
Canonical message count was unchanged. All 269 Codex checkpoints survived;
508 Claude checkpoints were invalidated.

This isolates migration overhead, not the time to replay configured Claude
sources. Regression tests separately verify that replay restores a parent and
subagent and invalidates readiness when their data changes. Tests also verify
preservation of incomplete/dirty state and atomic rollback after a checkpoint
deletion failure.

## Local search

Seven queries were tested in both thread and message mode, with local search,
user role, a 14-day window and five results. Queries included cbioportal,
cornucopia, indexing progress, voyage-code-4, database performance, warm models
and SSH removal. For all 14 cases, ordered result IDs and ordered matching
message IDs agreed between the old release on schema 32, the new direct
development build and its warm worker on the migrated snapshot.

| Development build, process wall time | Threads | Messages |
| --- | ---: | ---: |
| Direct execution median | 1,700 ms | 1,336 ms |
| Warm worker median | 810 ms | 392 ms |
| Warm worker cbioportal | 780 ms | 360 ms |

The first cold worker thread search took 1,805 ms. Each query/mode has one sample;
these are exploratory measurements rather than a statistical latency budget.
Old release timings are not compared with development timings because their
compiler profiles differ. No shared service or fleet deployment was changed.

Validation: 408 tests passed, including real-model and isolated Redis/PostgreSQL
integration tests; Clippy passed with warnings denied; SQL code generation passed.
