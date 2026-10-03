# Shared backend process-crash recovery contract

`shared_backend_persisted_restart_contract` exercises real HTTP API processes
against its own Qdrant container. It uses
the same Qdrant image as CI, existing local images only, and tiny typed
fixtures. It never restarts production or externally supplied test services.

Qdrant receives acknowledged snapshots for two nodes in one group and a
replacement snapshot that removes an old thread. The test verifies group/node
isolation, local-node exclusion, provider/conversation filters, lexical and vector
retrieval in message and thread modes, canonical context-only messages, thread
catalog metadata and freshness. It first replaces the API and gracefully restarts
the database, verifying exact freshness and rejecting a stale revision before
publishing a new one.

The second recovery cycle sends `SIGKILL` to the database while the API remains
alive, confirms Docker reports the database stopped, and starts the same
container with the same persisted storage. It then replaces the API process,
repeats the read/freshness checks, rejects another stale revision without changing
freshness, and verifies a new acknowledged publication. Database readiness and
HTTP operations have bounded timeouts. Container RAII and the CI owner-label
cleanup cover normal completion and test failure respectively.

Qdrant has its storage mounted on a Docker volume. This contract tests process crashes with the host and
storage still running; it does not establish power-loss durability, replication
failover, network-partition behavior, in-flight request outcomes, production-size
latency or survival under every alternative persistence setting. Synthetic vectors
isolate storage recovery from model inference.

On Xenia, after building the current `mash` binary with `cargo build-clif`, run:

```bash
env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=0 RUSTC_WRAPPER= \
  MASH_TEST_DOCKER_SG=1 \
  MASH_TEST_RESTART_OWNER=local-crash-recovery \
  MASH_TEST_SERVER_BINARY=/home/james/scratch/achi-targets/integrated/debug/mash \
  cargo test-clif --run-ignored only --no-tests fail -j 1 \
    -E 'test(shared_backend_persisted_restart_contract)'
```

Use a unique owner value for concurrent runs. After the test process is terminal,
inspect containers with that exact `mash.test.owner` label and remove only owned
leftovers if cleanup failed. The fixture rediscovers host ports after restart.

Historical validation before the Qdrant-only transition on Xenia on 2026-10-02 passed the combined three-backend recovery
contract in 15.051 seconds. The API binary was built from `cd8d9ea`; this change
modifies tests and documentation only. The full Rust/CLI suite passed 570 tests
in 8.039 seconds. Captures are retained outside Git in
`/home/james/scratch/mash-artifacts/crash-recovery-{build,contract,suite}-20261002.log`.
No containers remained under the run's exact owner label after completion.
All-target Clippy with warnings denied, formatting and the doctest also passed;
Clippy/doctest logs use the same `crash-recovery` artifact prefix.

Hosted CI for `cd8d9ea` did not start its Rust job: GitHub's check annotation
reports failed account payments or a spending limit. Local checks do not establish
hosted CI or deployment acceptance. The annotation is preserved separately in
`ci-cd8d9ea-annotations-20261002.json` under the same artifacts directory.
