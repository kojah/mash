# Correctness checks

Mash combines bounded [protocol models](../../protocol/README.md), tests
against real storage, targeted failure regressions, and focused mutation audits.
Each checks a different claim; none proves the whole distributed system correct.

The entry points are:

| Check | Command | Setup |
| --- | --- | --- |
| Bounded protocol exploration | `make protocol-check` | Explicit `make protocol-tools` installation |
| Revision/freshness mutation audit | `make mutation-check` | Explicit `make mutation-tools` installation |

Mutation audits remain explicit because they rebuild and retest each variant.
On Xenia all Make targets use `agent-test` for heavy work. No check installs
embedding models or uses production backend data.

## Replay interruption

`qdrant_replay_interruption_never_leaves_current_metadata_and_retry_repairs`
uses disposable Qdrant and per-store test hooks after invalidation and document
deletion. A fresh service handle must report stale views, refuse dense search,
preserve silver, and repair through replay. It is included in the existing CI
Qdrant test selection. It does not terminate a process or simulate power loss;
the separate persisted-restart contract covers process termination.

Run locally through `agent-test` with `MASH_TEST_QDRANT_URL` pointing to a
disposable, memory-limited Qdrant 1.19+ service:

```bash
agent-test env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=1 RUSTC_WRAPPER= \
  cargo test-clif --locked --lib --run-ignored only \
  -E 'test(qdrant_replay_interruption)' --no-tests fail --test-threads 1
```

The local release validation runner limits the broad nextest suite to four
concurrent tests, as the focused runner does. SQLite-heavy tests otherwise
compete for disk access at host CPU-count concurrency and can exceed their
existing deadlines even inside the per-job memory budget.

## Direct local Qdrant acceptance

Local CLI/MCP calls Qdrant directly; it does not start an Axum service. Run the
local lifecycle tests against one disposable Qdrant instance, with both
`MASH_TEST_QDRANT_URL` (REST reads/setup) and `MASH_TEST_QDRANT_GRPC_URL`
(point mutations) set to that instance:

```bash
agent-test env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=1 RUSTC_WRAPPER= \
  cargo test-clif --locked --lib --run-ignored only \
  -E 'test(local_qdrant) or test(qdrant_status_rejects)' \
  --no-tests fail --test-threads 2
```

These tests cover committed silver before replay, stale status, hidden/deleted
records, failed inference, and namespace isolation after
copying or replacing silver. Public CLI/MCP acceptance adds first-run setup,
readiness, pagination, evidence, privacy, and streamed rebuild progress. Tests
using real embedding models require an explicit `MASH_TEST_MODELS_DIR`.

The former SQLite vector scan and ANN replay benchmarks were removed with
SQLite gold. Their historical captures do not describe the Qdrant implementation.
Measure end-to-end indexing and search separately from transport benchmarks;
transport byte counts and latency alone do not establish indexing throughput.

## Mutation audits

[cargo-mutants](https://mutants.rs/) changes expressions or function bodies in
an isolated copy and asks whether selected tests reject the changed behavior.
The default scope is silver revision validation and provider
timestamp-provenance validation, with their corresponding tests
in `.cargo/mutants.toml`. The enrichment selection is limited to
`Timestamps::is_current_for`; Codex paginated-history tests cover native metadata
origins alongside the existing JSONL message-boundary evidence tests.
This is a narrow test-quality audit, not a workspace coverage score.

```bash
make mutation-tools                  # Explicit pinned Linux x86_64 download.
bash tools/mutation-check.sh --list  # Review scope without building.
make mutation-check                  # Baseline first; then isolated mutations.
```

For native chat certification, set the disposable `MASH_TEST_QDRANT_URL`, review
`bash tools/mutation-check.sh --list-certification`, and run
`make mutation-certification`. Its separate scope covers `Store::certify` with
real-backend coverage-merge, stale-head and parallel-baseline regressions, plus
common Qdrant freshness predicates and local reader identity checks. Set
`MASH_MUTATION_DIFF` to an existing unified diff to bound any audit to changed
hunks; the same filter applies to its list mode. Retain that diff with the
results and check that the selected list is nonempty. This does not disable
the baseline, source isolation, job limits or timeouts.

The installer pins cargo-mutants 27.1.0 and verifies its release archive SHA-256.
Requires Bash, curl, tar, sha256sum, flock, the repository nightly with Cranelift,
and cargo-nextest. On Xenia `make` runs the audit through `agent-test`. Audits use
one mutant at a time and four build tasks, a ten-minute build deadline and a
two-minute test deadline per scenario. No automatic download happens during a
check. Artifacts and a persistent build cache stay outside Git under
`/home/james/scratch/mash-artifacts/mutations` on Xenia or the user cache elsewhere;
`MASH_MUTATION_ARTIFACTS` overrides this. A lock serializes access to that cache.

Inspect the generated `mutants.out` outcomes, logs and diffs. A survivor is a
question to investigate: is a contract assertion missing, is the selected test
set wrong, or is the mutation behaviorally equivalent? Add meaningful regression
assertions for real gaps. Review unviable mutations and timeouts separately;
they do not demonstrate detection. Keep baseline failures visible. Changing the
scope requires reviewing the selected tests too. Run audits manually for relevant
changes or periodic test reviews; ordinary CI runs the unit and backend
regressions without paying mutation build costs on every push.
