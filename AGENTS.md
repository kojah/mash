# mash development instructions

## Product boundary

mash is a Rust CLI with JSON output and stdio MCP server for local agent histories.
Node and server configuration is strict TOML only. Do not add JSON compatibility or conversion.
Keep the CLI command surface limited to `index`, `search`, `list-threads`, `view`, `status`,
`forget`, `models install`, `mcp`, and `serve`.

- Use Rusqlite as the sole SQLite library.
- Use SQLite FTS5 for local lexical search.
- Use an authenticated Axum HTTP API backed solely by Qdrant for one shared index
  per configured node group. Require a local SQLite index; support local-only or
  both indexing with independent provider allowlists. Keep local operations
  offline-capable. Only the HTTP service accesses Qdrant. Never restore SSH query fan-out.
- Use the official Rust MCP SDK (rmcp) for the stdio server and call the same typed application core as the CLI.
  Keep CLI argument parsing/output and MCP protocol handling in thin wrappers;
  MCP must not construct CLI arguments or call CLI handlers.
- Keep concrete local FastEmbed and remote HTTP embedding options with exact
  cosine search, RRF, and bounded local reranking. Support Voyage and the
  OpenAI-compatible embeddings API without provider traits or registries.
- Qdrant uses the existing reqwest client and typed REST models, named dense/sparse
  vectors, and conditional group-catalogue publication on Qdrant 1.19+. Default to exact
  vector retrieval; permit explicit HNSW. Keep immutable staging invisible, enforce
  revisions across API processes, and fail closed on incompatible existing schemas.
- Do not add provider traits, registries, installed daemons, compatibility
  shims, alternate output modes, a TUI, export, or analytics.
- CLI searches may share an on-demand, per-user local worker. It must exit after
  60 seconds of inactivity, accept only searches, and preserve direct execution
  with `search --no-worker`. Do not install it as a system service.
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

The research lab's frozen experiment contracts and dependency-free CI fixtures
take precedence over library defaults in the skill. Keep existing stdlib
`argparse`/`json` entry points and canonical codecs where replacing them would
change pinned bytes, historical reproduction, or the dependency-free execution
contract. Do not migrate archived experiments or their code pins as stylistic
cleanup. New validated records should be typed; new standalone tooling may use
Click, Pydantic and orjson when its environment explicitly provides them.

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

The workspace defaults include `mash` and `mash-query`. Query-only iteration can use
`env CARGO_INCREMENTAL=1 RUSTC_WRAPPER= cargo test-clif -p mash-query`.
See `docs/crate-boundaries.md`; integrated search validation remains required
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

Build once per platform and reuse the validated artifact for nodes on that platform.
Ordinary binary deployment must not run `index` automatically. Inspect local
`status` and, for shared publication, `status --shared` after installing. Refresh
only when the configured artifacts, source interpretation or publication target
need repair; do not skip required provider/privacy/schema migrations. A query-model
availability failure needs explicit model setup, rather than repeated indexing.
Keep background synchronization or explicit ingestion separate from redeployment.
See `docs/redeployment.md` for readiness checks, measured costs and pending delta work.

## Agent work tracking

Topic modeling is the first priority. Follow the research sequence: compile
prior art, shortlist approaches, perform reproducible Python experiments and
record results, select a method, then implement it in Rust. Keep the Python lab
isolated from the application and private corpora/artifacts outside Git.
Before topic work, read `research/topics/SELECTION_CRITERIA.md` and
`research/topics/WORKLIST.md`. Respect the finite experiment cap; historical
follow-up notes do not authorize new algorithm rounds or restore hierarchy.
Keep pending acceptance gates open and continue other production work while
independent review is pending.
Update `paper/main.tex` and `paper/references.bib` alongside substantive topic
experiments with procedures, setup, prior art, results, analysis and limitations.
Topic modeling must not use an LLM: no generative summaries, rewriting, labeling,
taxonomy synthesis or paid topic-inference calls. Prefer reusing existing local
embeddings with lightweight online clustering and lexical labels. Any additional
non-generative model work must be measured, bounded and justified by quality.
Measure complete ingestion with topics enabled versus disabled, including context
processing, clustering, label maintenance and persistence, including draining
asynchronous topic work. Following the user's generous performance budget, there
is no fixed percentage overhead rejection gate. Prioritize topic quality and
genuine online discovery, and disclose the measured quality/cost tradeoff. Report
initial indexing, incremental edits/deletes, p95 update latency, CPU, peak RSS,
publication lag and sustained-arrival backlog on supported node classes. Topic
work must not block ordinary search or accumulate an indefinitely growing backlog.
Follow `research/topics/SELECTION_CRITERIA.md`; do not promote a method using
clustering-only timings or quality alone.
Keep exploratory development distinct from held-out evaluation; do not claim
semantic quality, novelty or guided-search gains without corresponding evidence.
Before promoting a selected Python topic method to Rust, require the executable
reference and shared stage fixtures described in `research/topics/PORT_CONTRACT.md`.
Freeze source/offset, numerical, tie and lifecycle semantics; test fixed-vector
algorithm parity separately from full inference, and enforce differential CI
before shipping the Rust implementation. Parity does not replace semantic
quality or complete-ingestion performance gates.
For Linux topic memory measurements, retain rusage and per-program-image counters
and use `research/topics/memory_audit.py` for external RSS/PSS evidence before a
performance gate. Rusage can preserve pre-exec launcher peaks; do not attribute
them to the workload or subtract peaks. Status counters are approximate and
externally sampled maxima are lower bounds. Define target/child scope and account
for monitoring overhead; these research audits are not production ingestion
measurements.
Build the living manuscript using `paper/build.py` with output outside Git.

Use Beads (`bd`) for substantive work and discovered follow-ups. On a fresh clone,
run `bd bootstrap`; then run `bd prime` for the workflow and `bd ready` for available
tasks. Create focused issues, claim work with `bd update ID --status in_progress`,
record dependencies with `bd dep add`, and close issues only after validation.
Preserve existing issues. Beads uses a separate Dolt database; do not commit its
runtime files or credentials. Before finishing work, run `bd dolt commit` and
`bd dolt push` as well as committing and pushing the implementation.
