# Development and verification

[Documentation](../README.md) · [Project home](../../README.md)

## Build and test

### Dashboard browser checks

Run `pnpm run lint`, `pnpm run check`, `pnpm run test`,
`pnpm run test:browser`, and `pnpm run build` in `dashboard/`.
The default browser suite uses mocked API responses to exercise UI behavior.
It does not establish that a running dashboard can search its real index.

Before handing over a preview, run the live browser checks against its actual
URL, including its network hostname and protocol. Choose a query that occurs
in that index:

```bash
MASH_DASHBOARD_URL=http://your-host:8790 MASH_DASHBOARD_QUERY=database pnpm run test:browser
```

This mode uses the deployed assets and real API without starting Vite or
mocking requests. It checks both Enter and button submission, successful
results, opening a matching transcript without a document reload, and browser
errors. The target needs current Qdrant views and available query models.
On Xenia, run these workloads through `agent-test` as for other tests.

### Rust checks

[Correctness checks](verification.md) describes generated Proptest
sequences against real SQLite, disposable-backend replay interruptions and
focused cargo-mutants audits. Property tests run with ordinary nextest tests;
mutation audits run explicitly with `make mutation-tools` and `make mutation-check`.
Bounded TLA+ checks for publication, tombstones and shared
view recovery live in [protocol/](../../protocol/README.md). Run `make protocol-tools`
once to install the pinned checker, then `make protocol-check`. These checks
include explicit expected counterexamples, including the historical replay
certification bug; they supplement implementation tests rather than certify
the Rust implementation.

Use Cranelift for development builds and tests. Install the nightly component
and use the Cargo aliases:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
cargo build-clif
cargo run-clif -- status
cargo test-clif
```

These aliases select Cranelift for mash and mash-query in the development profile. Dependencies
keep LLVM, so their existing build cache can be reused. `test-clif` requires
`cargo-nextest`. Use the usual commands below for standard LLVM verification
and `cargo build --release` for optimized builds. Cranelift is an experimental
nightly component and is available only on supported host platforms; see the
[Cranelift backend documentation](https://github.com/rust-lang/rustc_codegen_cranelift#usage).

```bash
cargo fmt --check
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

The handwritten workspace includes the application and the pure `mash-query`
crate. These default commands cover both. For an isolated query edit cycle,
use `env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query`.
See [crate boundaries](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/crate-boundaries.md) for responsibilities and
the integrated checks required when changing search behavior.

For repeated unit-test edits, keep one target directory and enable incremental
compilation explicitly:

```bash
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib
# Add a Nextest expression to run the affected unit tests:
env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib -E 'test(claude_copies::)'
```

On Xenia, warm incremental rebuilds after a small test edit took 7.7–8.6 seconds,
compared with approximately 32–35 seconds without incremental compilation.
The first incremental build took 41.7 seconds and created additional disk cache.
The empty `RUSTC_WRAPPER` bypasses a locally configured compiler wrapper; the
installed wrapper failed on this incremental build. `--lib` selects unit tests
and does not run CLI integration contracts. Keep the full suite, real-model and
shared-backend checks as completion gates. See the
[measured workflow](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-test-turnaround.md) for scope and limits.

CI also runs the [labeled hybrid relevance matrix](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-10-01-shared-relevance.md)
with explicitly installed models against local and shared Qdrant. The
report retains historical measurements of the removed backends.
The shared tests require their disposable backend endpoint and
`MASH_TEST_MODELS_DIR`. To run these checks together with those variables set:

```bash
cargo test-clif --lib --run-ignored only -j 1 -E 'test(labeled)' --success-output immediate
```

The installed August 2026 nightly can hit a [Clippy compiler crash](https://github.com/rust-lang/rust/issues/161495)
when checking async code. On that toolchain, use
`RUSTFLAGS=-Znext-solver=no cargo clippy --all-targets -- -D warnings`.

The storage integration tests require disposable Qdrant 1.19+ in
`MASH_TEST_QDRANT_URL` and its gRPC endpoint in `MASH_TEST_QDRANT_GRPC_URL`.
Tests start real HTTP services and create/remove isolated
collections for their owned groups:

```bash
MASH_TEST_MODELS_DIR='/path/to/explicitly-installed-models' \
  MASH_TEST_QDRANT_URL='http://localhost:6333/' \
  MASH_TEST_QDRANT_GRPC_URL='http://localhost:6334/' \
  cargo nextest run --run-ignored only --no-tests fail -j 2 -E 'test(qdrant)'
```

The real-model integration tests are ignored by default and require
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.
The shared-backend selections include Claude copy reconciliation with real
embeddings and require that directory too. Selected tests fail when prerequisites
are missing; `--no-tests fail` rejects an accidentally empty selection.

For an instrumented coverage report, install
[cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.9.1)
and the toolchain's `llvm-tools-preview` component. Set the model directory and
the Qdrant test URL above to installed models and disposable services.

Also set `MASH_TEST_SERVER_BINARY` to the instrumented `mash-server` binary produced
in the coverage target's `debug` directory. With an explicit `CARGO_TARGET_DIR`,
use `$CARGO_TARGET_DIR/llvm-cov-target/debug/mash-server`; without it, the default is
`target/llvm-cov-target/debug/mash-server`. Set `MASH_TEST_CLI_BINARY` to the `mash`
executable beside it for the MCP deployment probe. An explicit `CARGO_LLVM_COV_TARGET_DIR` instead
uses that directory directly, as defined by [cargo-llvm-cov 0.9.1](https://github.com/taiki-e/cargo-llvm-cov/blob/v0.9.1/src/cargo.rs#L63).
The suite builds the binary before running tests, so it need not exist before
the coverage command starts. Restart and snapshot-restore contracts
require Docker and the cached Qdrant image; set `MASH_TEST_DOCKER_SG=1` on Xenia.
They own their containers and API processes and do not restart the supplied
backend test services. Use the instrumented binary; forced API termination can
prevent its final LLVM profile flush, so the contract's runtime checks do not
guarantee that all child-process execution appears in the coverage report.
Snapshot restoration stops all fixture writers and restores both group
collections onto a fresh database after deleting the original database and
volume. See the [backup scope](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md#quiescent-backup-and-restore).

Then run:

```bash
rustup component add llvm-tools-preview
make coverage
```

This runs every test except the manual benchmarks, including the installed
model and all three shared-backend contracts. It writes LLVM JSON coverage to
`target/llvm-cov/coverage.json`; set `COVERAGE_OUTPUT` to choose another location.
The coverage task cleans workspace coverage artifacts before execution so older
binaries cannot contribute obsolete source mappings in a persistent target.
On nightly, use `CARGO_ARGS=--branch` to include
LLVM's experimental branch instrumentation. This does not measure MC/DC.
Generated SQL bindings, separate test files, standalone test helpers and the
test-only relevance evaluator and manual benchmark modules are excluded from
the report.
Tests embedded in production source files still contribute to those files'
coverage, so assess uncovered behavior as well as the aggregate percentage.
Coverage needs LLVM instrumentation; regular development continues to use
Cranelift. On the affected nightly, use `RUSTFLAGS=-Znext-solver=no make coverage`.

For bounded local build and CLI search measurements, use installed models and
a dedicated disposable Qdrant instance. The script copies a bounded sample of
provider histories; captures contain private history and must stay outside Git:

```bash
agent-test uv run --no-project python tools/benchmark-views.py \
  --binary /path/to/mash --models /path/to/installed/models \
  --qdrant-url http://127.0.0.1:6333 \
  --qdrant-grpc-url http://127.0.0.1:6334 \
  --output /home/james/scratch/mash-artifacts/local-views-measurement
```

The script compares lexical/dense replay and search timings while checking that
replay preserves canonical silver. Timings include CLI startup. Process memory
and CPU exclude the Qdrant server; SQLite size reports cover silver only.
Missing memory measurements fail the capture. These measurements do not establish
relevance or fleet performance. The former SQLite scan and ANN replay benchmarks
were removed with SQLite gold.

CPU indexing reserves two workers for long embedding batches and allows short
batches to use all eight workers. It preserves the dynamically quantized model's
batch membership and vector generation. See the
[indexing memory measurements](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-index-memory.md) for the
memory/throughput tradeoff and the manual worker-budget diagnostic. This is an
inference workspace limit, not a total indexing memory limit.

## Generated SQLite queries

SQLite query models are generated with sqlc 1.31.1 / sqlc-gen-rust 0.1.12.
Generated output is checked in; normal Cargo builds require no generator.
To regenerate as part of a build, install these versions and run
`make build CODEGEN=1` (add `CARGO_ARGS='--release --locked'` for release).
`make generate-queries` regenerates without compiling; `make check-generated`
checks for stale output. Generation reads the same SQLite schema SQL used at runtime.
CI checks regeneration against committed output. Dynamic canonical predicates
reuse the generated projection and decoder; schema initialization and writes remain explicit.

## Using Mash in Codex during development

Point Codex at the build output so rebuilding does not require copying or
reinstalling the executable. Build with Cranelift in a stable target directory:

```bash
env CARGO_TARGET_DIR=/absolute/path/to/build cargo build-clif --bin mash
```

Add this to your local `~/.codex/config.toml`, or a trusted project's
`.codex/config.toml`:

```toml
[mcp_servers.mash]
command = "/absolute/path/to/build/debug/mash"
args = ["mcp"]
startup_timeout_sec = 30
tool_timeout_sec = 120
```

Mash uses its normal node configuration, SQLite index and model directory.
To override those paths, put `--config PATH`, `--db PATH` and
`--models-dir PATH` before `mcp` in `args`. Forward the configured token
environment variable using Codex's `env_vars`, or use a local launcher that
loads your private node environment before executing the same build output.
Keep credentials and machine-specific launchers outside Git.

`mash mcp` runs in one process. Restart it after installing a new binary or
changing startup settings, and reload updated plugin instructions. Accepted
mutations are never replayed. Opted-in watching lasts for the process lifetime.

MCP `help` provides live schemas and typed examples without an index or models.
Help/status report the running build revision and contract fingerprint; supply
the installed skill fingerprint as `expected_contract` to detect stale guidance.

The `mash-history` skill's workflow template is `app/templates/history_skill.md`.
Its generated files live in `plugins/mash/skills/mash-history/`.
Its small live-help bootstrap and release stamp are generated from Rust with
`make generate-skills`; do not edit the generated `SKILL.md` or `contract.json`.
`make check-skills` checks generation, real argument validation and CLI examples.
Generation compares the contract stamp with the previous release and requires an increased
version in both plugin manifests whenever the contract or workflow changes.
Typed live-help examples also pass the real core validators. Update the installed
plugin and reload its instructions after changing the skill; consult live help
for current parameters instead of duplicating the API reference in the skill.

Development deployment uses `tools/build-dev-linux.sh` to reuse successful gates
for identical source/toolchain/build inputs while still stamping the release
binary for the current commit. Source changes invalidate the gate receipt, and
changes during validation or compilation fail the rollout. The API helper locks
concurrent rollouts and skips restarting an identical healthy release.
Ansible owns fleet selection, builds, API-first rollout, offline skips and
required schema migrations. `mash-fleet probe` verifies the installed build,
contract, query readiness and shared publication over MCP. Ordinary deployment
never ingests history or installs models. A silver upgrade runs one explicit sync
to re-ingest stale interpretation, rebuild local views and publish.

Use `bash tools/fleet.sh deploy shared-api xenia` with a private inventory.
Refresh remains explicit: `bash tools/fleet.sh sync xenia` or
`bash tools/fleet.sh local-views xenia`. See [deployment setup](../../deployment/README.md).
Runner logs and reports stay private. Skipped targets are not installed or ready.

## Work tracking

Agent work is tracked with [Beads](https://github.com/steveyegge/beads). Issue
history lives in a separate, private Dolt remote (`kojah/mash-beads`), not in this
repository. With `bd` and Dolt installed and access to that remote, run
`bd config set sync.remote git+https://github.com/kojah/mash-beads.git` and
`bd bootstrap` after cloning, then `bd prime` and `bd ready`. `bd dolt commit` and
`bd dolt push` persist tracking updates. The entire `.beads/` directory, including
configuration, stays local and is ignored by Git.

## Releases and fleet deployment

Ordinary commits do not deploy the fleet. Follow the canonical preflight,
validated build, installation and readiness workflow in
[deployment/README.md](../../deployment/README.md). Database upgrades require a
separate preservation and recovery plan; see [upgrades](../upgrades.md).
