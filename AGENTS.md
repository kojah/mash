# mash development instructions

## Product boundary

mash is a Rust CLI with JSON output and stdio MCP server for local agent histories.
Node and server configuration is strict TOML only. Do not add JSON compatibility or conversion.
Keep the CLI command surface limited to `index`, `search`, `list-chats`, `view`, `status`,
`forget`, `export`, `import`, `models install`, `mcp`, and `serve`.

- Use Rusqlite as the sole SQLite library.
- Use SQLite FTS5 for local lexical search.
- Keep one silver schema (`app/storage/silver.sql`) for local indexes and for
  each node's shared silver database on the server. Local views live in a separate,
  paired SQLite database; neither views store may hold history that silver lacks,
  and both must stay replayable from silver (`index --rebuild-local-views --full`,
  `serve --replay-views`).
- Native and connector history share one silver object model: the `objects` table
  with core roles (container, segment, entry, document), namespaced kinds and
  parent edges, plus native-only side tables. Native turns are segments derived
  from silver after every write, never archived or published; each native
  provider declares its topology in code (`source::native_schema`).
- Do not check in schema upgrade code. Each release opens only its own silver
  and views schemas and rejects others untouched; views are rebuilt, shared silver
  is republished, and local silver crosses releases only through export and
  `import --keep-identity` (`tools/upgrade-local-index.sh`).
- Use an authenticated Axum HTTP API for one shared index per configured node
  group. Shared silver is server-side SQLite (no views tables); shared views live in
  Qdrant.
  Commit silver before synchronizing views; do not build transaction, staging or
  versioning machinery on top of Qdrant. Native history requires a local SQLite index; support
  local-only or both indexing, where shared publishes exactly the local index. External
  connectors deliver to each destination they choose (local and/or a node's shared silver)
  independently: node publication never carries connector records, and a node's shared
  connector records arrive only through direct ingress. Keep local operations
  offline-capable. Only the HTTP service accesses Qdrant. Never restore SSH query fan-out.
- Use the official Rust MCP SDK (rmcp) for the stdio server and call the same typed application core as the CLI.
  Keep CLI argument parsing/output and MCP protocol handling in thin wrappers;
  MCP must not construct CLI arguments or call CLI handlers.
- Keep concrete local FastEmbed and remote HTTP embedding options with exact
  cosine search, RRF, and bounded local reranking. Support Voyage and the
  OpenAI-compatible embeddings API without provider traits or registries.
- Qdrant uses the existing reqwest client and typed REST models, named dense/sparse
  vectors on Qdrant 1.19+. Default to HNSW vector retrieval with ef_runtime=200;
  permit explicit exact search. Never report uncertified views as current, and
  fail closed on incompatible existing schemas or superseded views settings.
- Do not add provider traits, registries, installed daemons, compatibility
  shims, alternate output modes, a TUI, or analytics. Portable silver export/import
  is supported; exclude deployment credentials, models and provider files.
  Clearly document that exports preserve sensitive canonical history.
- CLI searches execute directly. MCP runs in one process; reconnect after upgrades.
- Support opt-in debounced synchronization with `index --watch` and MCP startup
  `--watch`. Keep it within the running process; do not install a daemon.
- Use concrete, enum-selected history adapters with typed input models. The
  provider set includes Claude Code, Codex, OpenCode, GitHub Copilot, Cursor,
  and Antigravity.
  Read upstream application databases without writing to them; respect their
  WAL files and session identities. Keep provider settings in the enum-keyed
  collection shared by configuration, routing, watching, and ingestion. Do not
  add provider traits, registries, plugin systems, or guessed format aliases.
- Never download models outside the explicit `models install` command.
- Unsafe Rust is forbidden.

## Codebase discovery

Codebase Memory MCP is configured for this project in `.codex/config.toml`
and `.mcp.json`. Prefer its graph tools for structural code discovery:
`search_graph`, `trace_path`, `get_code_snippet`, and `get_architecture`.
At session start, confirm the `mash` project and current generation with
`list_projects` or `index_status`. Use task-directed verification and check
`check_index_coverage` for every evidence path before relying on graph results.
Read the reported source ranges when coverage is partial, stale, or unknown.
Use ordinary source searches for literals/configuration or when MCP is unavailable.
Refresh the graph after material source changes with `index_repository`.

## Implementation style

Prefer concrete functions and structs. External input returns typed JSON errors
and never panics. Await shared HTTP I/O and model-work boundaries. Keep Rusqlite,
parsing, and local inference on blocking workers; bound model work and serialize
MCP mutations while permitting read concurrency. Add a dependency only when it
is smaller and clearer than a local implementation of the required behavior.

Use typed Rust models for known serialized schemas throughout the application.
Use enums for providers, roles, phases, states, modes, and other known alternatives;
preserve unknown variants when external vocabularies are extensible. Keep generic
JSON limited to opaque provider tool payloads and SDK boundaries requiring it.

Use `apply_patch` for source edits. Preserve unrelated worktree changes. Do not
create compatibility copies or versioned replacement files.

## Python conventions

Use `.agents/skills/python-coding-conventions/SKILL.md` when changing Python.
The same skill is linked under `.claude/skills` and `.codex/skills`.
Run Python through `uv`; use Ruff and `ty` for the affected modules and run
the full Python test suite before completing Python changes.

Dependency-free CI fixtures take precedence over library defaults in the skill. Keep existing stdlib
`argparse`/`json` entry points and canonical codecs where replacing them would
change pinned bytes, historical reproduction, or the dependency-free execution
contract. New validated records should be typed; new standalone tooling may use
Click, Pydantic and orjson when its environment explicitly provides them.

New research experiments use Optuna for registered trial selection/records and
MLflow for experiment tracking. Keep metadata in local SQLite and artifacts
under the private research directory; log parameters, provenance hashes and
aggregate results. Do not autolog or upload raw chat text, annotations, embeddings
or per-chat predictions to MLflow. Preserve frozen baselines and keep new
candidates separate. Do not rewrite historical studies just to add tracking.

## Verification

Keep project logs, downloads, research captures and other durable work artifacts
under `/home/james/scratch/mash-artifacts`, outside Git. Do not use `/tmp/mash*`
for these artifacts. Earlier temporary artifacts were moved to
`/home/james/scratch/mash-artifacts/tmp-migration-2026-09-30`.
Use the persistent target and test temporary directories below for validation.

Use the persistent Xenia target and temp directories:

```bash
env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=0 cargo test-clif

env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=0 cargo clippy --all-targets -- -D warnings

cargo fmt --check

env TMPDIR=/home/james/scratch/achi-tmp \
  CARGO_TARGET_DIR=/home/james/scratch/achi-targets/integrated \
  CARGO_INCREMENTAL=0 cargo test --doc
```

The real-model integration test is ignored by default and requires an explicit
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.
Keep full shared-backend validation on disposable services separate from
real-corpus benchmarks and bulk collection cleanup. Large Qdrant collection
deletion can delay unrelated requests and make short integration-test deadlines
fail. Give benchmarks their own backend instance and clean up only their owned
groups/collections after their process handles have completed.
The ignored `shared_backend_persisted_restart_contract` owns and restarts fresh
Qdrant containers, plus real API processes. Preload the
same image used by CI and set `MASH_TEST_SERVER_BINARY` to the built mash binary;
set `MASH_TEST_DOCKER_SG=1` on Xenia for Docker group access. It rediscovers host
ports after database restarts and never restarts the externally supplied test
services. CI also removes fixtures with its exact run-owner label if the test
fails or times out. This verifies acknowledged writes after graceful database
restart and SIGKILL process termination using the same persisted host storage,
not power-loss durability or failover.

Use `cargo build-clif`, `cargo run-clif`, and `cargo test-clif` for development
iteration. Retain Clippy, formatting, and doctest validation. On the installed
August 2026 nightly, Clippy needs `RUSTFLAGS=-Znext-solver=no` to avoid its known
async-code compiler crash.

The workspace defaults include `mash`, `mash-query` and `mash-dev`. Query-only iteration can use
`env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query`.
See [docs/crate-boundaries.md](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/crate-boundaries.md); integrated search validation remains required
when changing query behavior.

For repeated unit-test edits, use the persistent target/temp directories above
with `CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif --lib`, adding a focused
`-E` expression when appropriate. Warm incremental builds substantially reduce
small test-edit rebuild time on Xenia; the initial build is slower and uses more
disk cache. Bypass the local wrapper for this path because its incremental build
failed. This is an iteration workflow: retain full CLI/MCP, real-model and shared
backend verification for changes affecting those paths, and the required final
checks above. Do not treat a unit-only run as evidence for integration behavior.

## Git

Work on and push `main`; it is the repository's sole development branch.

## Deployment

Redeploy by default after every change pushed to `main`, following
`.agents/skills/mash-deployment/SKILL.md` (linked under `.claude/skills` and
`.codex/skills`), unless the user says not to.

Build once per platform and reuse the validated artifact for nodes on that platform.
Ordinary binary deployment must not run `index` automatically. Inspect local
`status` and, for shared publication, `status --shared` after installing. Refresh
only when the configured artifacts, source interpretation or publication target
need repair; do not skip required provider/privacy/schema migrations. A query-model
availability failure needs explicit model setup, rather than repeated indexing.
Keep background synchronization or explicit ingestion separate from redeployment.
See [docs/redeployment.md](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/redeployment.md) for readiness checks, measured costs and pending delta work.

Run deployment and refresh directly with `bash tools/fleet.sh`, explicit targets
and the private Ansible inventory. The `mash-dev` crate contains only the RMCP
readiness/refresh probe; it must not duplicate indexing or schema logic. Keep
deploy and refresh separate and raw command logs private. See deployment/README.md.

## Agent work tracking


Use Beads (`bd`) for substantive work and discovered follow-ups. On a fresh clone,
run `bd bootstrap`; then run `bd prime` for the workflow and `bd ready` for available
tasks. Create focused issues, claim work with `bd update ID --status in_progress`,
record dependencies with `bd dep add`, and close issues only after validation.
Preserve existing issues. Beads uses a separate Dolt database; do not commit its
runtime files or credentials. Before finishing work, run `bd dolt commit` and
`bd dolt push` as well as committing and pushing the implementation.
