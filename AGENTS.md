# mash development instructions

## Product boundary

mash is a Rust CLI with JSON output and stdio MCP server for local agent histories.
Node and server configuration is strict TOML only. Do not add JSON compatibility or conversion.
Keep the CLI command surface limited to `init`, `index`, `search`, `list-chats`, `view`,
`status`, `forget`, `export`, `import`, `models install`, `mcp`, `serve`, and `dash`
(the local read-only web dashboard).
`init` is first-run setup: it writes configuration for detected histories, registers
Mash with agent CLIs through their own commands, and indexes; it never downloads models.

- Use Rusqlite as the sole SQLite library.
- Use the common Qdrant engine and collection schema for local and shared lexical/vector search.
- Keep one silver schema (`app/storage/silver.sql`) for local indexes and for
  each node's shared silver database on the server. Local views live in Qdrant;
  neither views store may be certified against history that silver lacks,
  and both must stay replayable from silver (`index --rebuild-local-views --full`,
  `serve --replay-views`).
- Model agent history directly as chats, messages, turns, and branch ancestry.
  Turns are derived from silver after every write, never archived or published.
  Keep message identity independent of display order and use stable message IDs
  for branch ancestry. Do not introduce a generic object model, external content
  connectors, connector ingress, or a Python SDK. Slack, Obsidian, and other
  non-agent content are outside the product boundary.
- Do not check in schema upgrade code. Each release opens only its own silver
  and views schemas and rejects others untouched; views are rebuilt, shared silver
  is republished, and local silver crosses releases only through export and
  `import --keep-identity` (`tools/upgrade-local-index.sh`).
- Use an authenticated Axum HTTP API for one shared index per configured node
  group. Shared silver is server-side SQLite (no views tables); shared views live in
  Qdrant.
  Commit silver before synchronizing views; do not build transaction, staging or
  versioning machinery on top of Qdrant. Native history requires a local SQLite index; support
  local-only or both indexing, where shared publishes exactly the local index.
  Keep local operations
  offline-capable using a local Qdrant instance. Local CLI/MCP accesses Qdrant
  directly without Axum; the authenticated HTTP service owns shared access.
  Keep local writer admission across silver commits and gold certification.
  Never restore SSH query fan-out.
- Use the official Rust MCP SDK (rmcp) for the stdio server and call the same typed application core as the CLI.
  Keep CLI argument parsing/output and MCP protocol handling in thin wrappers;
  MCP must not construct CLI arguments or call CLI handlers.
- Keep concrete local FastEmbed and remote HTTP embedding options with exact
  cosine search, RRF, and bounded local reranking. Support Voyage and the
  OpenAI-compatible embeddings API without provider traits or registries.
- Qdrant publication uses the generated Tonic client with bounded protobuf requests,
  explicit completion checks and no automatic mutation retries or REST fallback.
  Reads and collection setup use the existing reqwest client and typed REST models, named dense/sparse
  vectors on Qdrant 1.19+. Default to HNSW vector retrieval with ef_runtime=200;
  permit explicit exact search. Never report uncertified views as current, and
  fail closed on incompatible existing schemas or superseded views settings.
- Do not add provider traits, registries, installed daemons, compatibility
  shims, alternate output modes, a TUI, or analytics. Portable silver export/import
  is supported; exclude deployment credentials, models and provider files.
  Clearly document that exports preserve sensitive canonical history.
- CLI searches execute directly. MCP runs in one process; reconnect after upgrades.
- Support opt-in debounced synchronization only with `index --watch` in a
  separate process, plus the plugin's throttled one-shot SessionStart catch-up
  (a detached incremental `mash index`, opt-out with `MASH_AUTO_INDEX=off`). MCP reads the refreshed index and permits explicit indexing;
  it does not own watchers. Do not install a daemon. Superseded long-lived
  runtimes refuse new writes; already admitted operations may finish.
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

## Dashboard front end

`dashboard/` holds the front end for `mash dash`, a local, read-only web
dashboard (Beads mash-twg6; design mockup linked there). It uses Svelte 5, Vite,
TypeScript 7 (`svelte-check --tsgo`, with TypeScript 6 kept only as its
compatibility dependency), Tailwind CSS 4, Lucide Svelte, pnpm, Biome and Vitest;
follow `.agents/skills/frontend-development-prefs/SKILL.md` and
`.agents/skills/typescript-coding-conventions/SKILL.md`. TanStack Query owns
server-state caching and refresh; `dashboard/src/lib/api.ts` validates real API
responses. Keep submitted search results and displayed transcript text stable
until the reader requests an update. Before completing front-end changes, run `pnpm run lint`,
`pnpm run check`, `pnpm run test` and `pnpm run build` in `dashboard/`.
Render all chat content as text or through a sanitizer; archived history is
untrusted.

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

### Protocol models

Use the bounded TLA+ models in `protocol/` when changing concurrency, retry or
acknowledgement semantics, crash recovery, publication revision guards,
tombstones, or ordering between silver commits and views/certification writes.
Add a small model when a new protocol introduces consequential interleavings
that ordinary examples cannot cover. Parsers, presentation, ranking quality,
and routine sequential transformations generally belong in ordinary tests.

Keep models synchronized with the actual code. A change to a modeled protocol
must review its model, TLC configuration, source mapping and assumptions in
`protocol/README.md`, and update them together when semantics change. Match
atomic actions to real transaction, lock and awaited I/O boundaries; explicitly
model failures between separate durable writes. Do not assume extra locking,
atomicity, fairness, delivery guarantees or writer fencing to make checks pass.
If no model change is needed, record why in the change's validation evidence.

Run `make protocol-check` for protocol changes; install the pinned checker
explicitly with `make protocol-tools` when needed. The checker includes bounded
safety/liveness checks and named expected counterexamples that verify the
properties detect broken behavior. Keep source behavior, deliberate mutations,
and proposed contracts clearly distinguished. Record implementation/model gaps
in Beads with a reproducing trace; when fixing a gap, update its model status
and add a corresponding Rust boundary or fault-injection regression. Passing
TLC checks establish properties of the bounded model, not proof that Rust or
the backing services implement it. They supplement the existing focused tests.

### Implementation checks

Use Proptest for agent-history invariants across varied inputs or operation
sequences, with an independent expected state. Generated retries, rejections,
reopenings and archive round trips supplement named regressions.
Keep cases bounded in normal CI, reproduce failures with `PROPTEST_RNG_SEED`,
and promote shrunk failures to focused deterministic regressions. Property
tests do not replace public API or disposable backend tests.
Keep shrunk failures as named regressions. Run backend sequence tests against
disposable Qdrant services with explicit resource limits.

Use `make mutation-tools` then `make mutation-check` for focused test-quality
audits when changing critical validation/freshness guards or strengthening their
tests. Scope lives in `.cargo/mutants.toml`; keep the selected tests aligned with
the selected source. Inspect surviving mutants for missing assertions, invalid
test selection or equivalent behavior. Do not weaken production checks or add
blanket exclusions to obtain a score. Unviable mutants and timeouts are not
evidence that a test detected faulty behavior. Keep these audits manual rather
than adding them to every development iteration; see `docs/verification.md`.

For failure ordering, prefer small per-instance test-only hooks at actual
durable boundaries. Assert persisted state and recovery after interruption;
distinguish simulated failures from process termination and power loss.

Use `make iterate FOCUS='test(changed_behavior)'` for the normal development loop:
formatting, Clippy, then explicitly selected nextest regressions. A missing or
empty selection fails; this mode never certifies full acceptance or builds an
optimized release. Preserve passing evidence until relevant inputs change.
Focused checks and relevant tooling fixtures are enough to commit and push.

Run `make validate-dev` once for a stable release batch, after focused checks
pass. It runs the broader gates without building a release binary. The release
runner reuses a receipt for identical source, toolchain and effective validation
inputs across commits; commit identity and `MASH_BUILD_REVISION` affect binary
stamping, not validation reuse. Never reuse a binary stamped for another release.
Do not repeat broad gates after every edit or push. Formatting and Clippy precede
expensive fixtures and broad tests, so cheap failures stop early.

Keep project logs, downloads, research captures and other durable work artifacts
under `/home/james/scratch/mash-artifacts`, outside Git. Do not use `/tmp/mash*`
for these artifacts. Earlier temporary artifacts were moved to
`/home/james/scratch/mash-artifacts/tmp-migration-2026-09-30`.
Use the persistent target and test temporary directories below for validation.

Use the persistent Xenia target and temp directories:
The raw broad commands below belong to release milestones; use the canonical
focused runner during ordinary implementation.

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

Use `cargo build-clif`, `cargo run-clif`, and `cargo test-clif` for directed
diagnosis; prefer the focused runner for iteration. Keep formatting and Clippy
in that loop, and doctests at release milestones. On the installed
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
failed. Include public CLI/MCP or retrieval/publication boundary regressions in
the focused selection when changing those paths. Run broader CLI/MCP, real-model
and shared-backend acceptance at a release milestone when those paths are affected.
Do not treat a unit-only run as evidence for integration behavior or full acceptance.

## Git

Work on and push `main`; it is the repository's sole development branch.

## Deployment

Deploy at an explicitly requested release or agreed stable-batch milestone, following
`.agents/skills/mash-deployment/SKILL.md` (linked under `.claude/skills` and
`.codex/skills`). A commit or push alone is not a deployment request. Batch small
validated changes; do not deploy ordinary implementation commits automatically.

Before a release build, run `bash tools/fleet.sh preflight TARGET...` or use
`deploy`, which includes that same preflight. It checks configured service
lifecycle, release records, authentication, install access and required APIs
before compiling or installing anything. All targets must declare read-only
`mash_deploy_preflight` commands; the legacy API installer uses
`check-dev-api.sh --deployment` for identity and write access, plus an explicit
check that its configured service manager agrees with the chosen installer.
The canonical runners select and synchronize the locked controller environment;
do not depend on a caller remembering `UV_PROJECT_ENVIRONMENT`.

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


Use Beads (`bd`) for substantive work and discovered follow-ups. Its Dolt history
lives in the private `kojah/mash-beads` repository, never in this one. On a fresh
clone, run `bd config set sync.remote git+https://github.com/kojah/mash-beads.git`
and `bd bootstrap`; then run `bd prime` for the workflow and `bd ready` for available
tasks. Create focused issues, claim work with `bd update ID --status in_progress`,
record dependencies with `bd dep add`, and close issues only after validation.
Preserve existing issues. Beads uses a separate Dolt database; do not commit its
runtime files or credentials. Before finishing work, run `bd dolt commit` and
`bd dolt push` as well as committing and pushing the implementation.
