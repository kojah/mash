# Correctness checks

Mash combines bounded [protocol models](../protocol/README.md), generated tests
against real storage, targeted failure regressions, and focused mutation audits.
Each checks a different claim; none proves the whole distributed system correct.

## Generated connector sequences

The [Proptest harness](../app/connectors/silver/tests/properties.rs) compares real
SQLite data, source revisions and stored receipts to an independent reference
after every operation. It generates writes/deletes across two sources, old
retries, conflicting batch identities, incorrect revisions, transactions that
must roll back, reopening, and export/import with identity preservation.
It runs in ordinary nextest/CI with 32 sequences of up to 24 operations.
A named sequence ensures every operation is exercised regardless of randomness.

```bash
make iterate FOCUS='test(connectors::silver::tests)'
PROPTEST_CASES=256 PROPTEST_RNG_SEED=42 \
  make iterate FOCUS='test(generated_connector_sequences)'
```

Proptest shrinks failures and reports the minimal sequence. Capture CI output;
use an explicit `PROPTEST_RNG_SEED` for repeatable investigations, then preserve
the minimal failure as a named regression. Automatic source-tree persistence is
disabled. Generated strings are synthetic, not private history. These tests do
not simulate SQLite power loss, network delivery or concurrent writers.

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

## Mutation audits

[cargo-mutants](https://mutants.rs/) changes expressions or function bodies in
an isolated copy and asks whether selected tests reject the changed behavior.
The initial scope is silver revision validation and dense/lexical freshness
manifests, with their corresponding unit tests in `.cargo/mutants.toml`.
This is a narrow test-quality audit, not a workspace coverage score.

```bash
make mutation-tools                  # Explicit pinned Linux x86_64 download.
bash tools/mutation-check.sh --list  # Review scope without building.
make mutation-check                  # Baseline first; then isolated mutations.
```

The installer pins cargo-mutants 27.1.0 and verifies its release archive SHA-256.
Requires Bash, curl, tar, sha256sum, flock, the repository nightly with Cranelift,
and cargo-nextest. On Xenia `make` runs the audit through `agent-test`. Audits use
one mutant at a time and four build tasks, a ten-minute build deadline and a
one-minute test deadline per scenario. No automatic download happens during a
check. Artifacts and a persistent build cache stay outside Git under
`/home/james/scratch/mash-artifacts/mutations` on Xenia or the user cache elsewhere;
`MASH_MUTATION_ARTIFACTS` overrides this. A lock serializes access to that cache.

Inspect the generated `mutants.out` outcomes, logs and diffs. A survivor is a
question to investigate: is a contract assertion missing, is the selected test
set wrong, or is the mutation behaviorally equivalent? Add meaningful regression
assertions for real gaps. Review unviable mutations and timeouts separately;
they do not demonstrate detection. Keep baseline failures visible. Changing the
scope requires reviewing the selected tests too. Run audits manually for relevant
changes or periodic test reviews; ordinary CI runs the property and backend
regressions without paying mutation build costs on every push.
