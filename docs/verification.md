# Correctness checks

Mash combines bounded [protocol models](../protocol/README.md), generated tests
against real storage, targeted failure regressions, and focused mutation audits.
Each checks a different claim; none proves the whole distributed system correct.

The entry points are:

| Check | Command | Setup |
| --- | --- | --- |
| SQLite generated sequences | `make property-check` | Repository Rust toolchain and nextest |
| Qdrant generated recovery sequences | `make property-backend-check` | Disposable `MASH_TEST_QDRANT_URL` |
| Bounded protocol exploration | `make protocol-check` | Explicit `make protocol-tools` installation |
| Revision/freshness mutation audit | `make mutation-check` | Explicit `make mutation-tools` installation |
| Connector recovery mutation audit | `make mutation-connectors` | Mutation tools and disposable `MASH_TEST_QDRANT_URL` |

`PROPTEST_CASES` increases generated cases and `PROPTEST_RNG_SEED` makes an
investigation repeatable. Ordinary CI runs SQLite properties in its usual test
job; its existing `test(qdrant)` selection also runs bounded generated connector
sequences and named regressions. Mutation audits remain explicit because they
rebuild and retest each variant. On Xenia all Make targets use `agent-test` for
heavy work. No check installs embedding models or uses production backend data.

The backend generator varies connector writes, deletions and interruptions
between actual point writes and persisted digest writes. It compares physical
Qdrant records to independent expected content after successful recovery, then
shrinks a failing operation sequence. Its per-instance interruption hook is
compiled only into tests. This simulates a durable boundary failure, not process
termination or a remote request that continues after timing out.
The default is 16 sequences of up to four operations, across lexical and dense
configurations; dense cases use a synthetic local embedding endpoint.

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

The local release validation runner limits the broad nextest suite to four
concurrent tests, as the focused runner does. SQLite-heavy tests otherwise
compete for disk access at host CPU-count concurrency and can exceed their
existing deadlines even inside the per-job memory budget.

## Local vector retrieval measurements

The ignored `benchmark_local_filtered_vectors` benchmark reads an explicitly
prepared local index and installed models. It records real quantized query
vectors, SQL-admitted vector IDs and exact top-20 scores for native and connector
history. It measures scoring with preloaded vectors separately from the existing
dense retrieval path, which loads vectors and hydrates the selected hits.

Run it on a frozen corpus copy, with an unused report filename outside Git:

```bash
agent-test env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=0 RUSTC_WRAPPER= \
  MASH_TEST_MODELS_DIR=/path/to/explicitly-installed/models \
  MASH_TEST_SEARCH_DB=/path/to/frozen/index.sqlite3 \
  MASH_TEST_VECTOR_REPORT=/home/james/scratch/mash-artifacts/vector-reference.json \
  cargo nextest run --release --locked --jobs 4 --lib --run-ignored only \
  -E 'test(benchmark_local_filtered_vectors)' --no-tests fail --nocapture
```

The benchmark never installs models or writes the index. The report contains
private identities and query embeddings; keep it private. Scopes exercise kind,
date, union, negation and empty filters. Three repetitions per query/scope expose
warm variation; they do not establish cold-cache latency or confidence intervals.
All scope buffers stay resident during measurement, so peak benchmark memory is
not the memory footprint of a normal search. Captured scope-load times include
both filtering and vector loading. These measurements exclude lexical search,
fusion, reranking and CLI startup, and do not certify end-to-end performance.

`benchmark_local_ann_pipeline` replays private ANN candidate files through the
same local hydration, fusion, diversification, contextual reranking and result
collection functions. It uses the message/chat candidate limits (50/200),
flexible lexical matching and a first page of 20 results. Supply the candidate
JSON files as an OS path-separated `MASH_TEST_ANN_INPUTS` list, plus a new private
`MASH_TEST_PIPELINE_REPORT` path and the same model/corpus variables above. Select
`test(benchmark_local_ann_pipeline)` in the optimized nextest command. This
ignored manual benchmark has a ten-minute bound.

The replay records actual warm-process SQLite pipeline time and estimates ANN
pipeline time by adding captured ANN retrieval, canonical vector rescoring and
scope preparation to the measured shared pipeline work. The USearch estimate
also includes measured SQLite ID prefiltering; the Edge payload path uses its
indexed predicate. Index opening, process startup and publication/freshness
machinery are outside this estimate. Keep candidate-generation source hashes and
the frozen corpus with reports. Compare ordered result IDs and supporting IDs
alongside timing; overlap measures agreement with the existing search, rather
than human relevance. The small-set exhaustive adapter is a control and must
preserve the exact pipeline's result identities and supporting evidence.

## Mutation audits

[cargo-mutants](https://mutants.rs/) changes expressions or function bodies in
an isolated copy and asks whether selected tests reject the changed behavior.
The scope is silver revision validation, dense/lexical freshness manifests,
and provider timestamp-provenance validation, with their corresponding tests
in `.cargo/mutants.toml`. The enrichment selection is limited to
`Timestamps::is_current_for`; Codex paginated-history tests cover native metadata
origins alongside the existing JSONL message-boundary evidence tests.
This is a narrow test-quality audit, not a workspace coverage score.

```bash
make mutation-tools                  # Explicit pinned Linux x86_64 download.
bash tools/mutation-check.sh --list  # Review scope without building.
make mutation-check                  # Baseline first; then isolated mutations.
```

For connector recovery, set `MASH_TEST_QDRANT_URL` to your disposable backend,
review `bash tools/mutation-check.sh --list-connectors`, then run
`PROPTEST_RNG_SEED=42 make mutation-connectors`. Its separate configuration
selects connector synchronization and shared document-cleanup mutations, with
generated recovery, node-isolation and HTTP publication tests. Review the list
when changing either source file: field-deletion candidates can include adjacent
cleanup helpers. Results describe that selected scope, not a coverage percentage.

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
changes or periodic test reviews; ordinary CI runs the property and backend
regressions without paying mutation build costs on every push.
