# mash development instructions

## Product boundary

mash is a Rust CLI with JSON output and stdio MCP server for Claude Code and Codex JSONL histories.
Node and server configuration is strict TOML only. Do not add JSON compatibility or conversion.
Keep the CLI command surface limited to `index`, `search`, `view`, `status`,
`forget`, `models install`, `mcp`, and `serve`.

- Use Rusqlite as the sole SQLite library.
- Use SQLite FTS5 for local lexical search.
- Use an authenticated Axum HTTP API backed by PostgreSQL/pgvector (tokio-postgres)
  or Redis Search for one shared index per configured
  node group. Support local-only, shared-only without SQLite, and both indexing,
  with independent provider allowlists. Keep local operations offline-capable.
  Only the HTTP service accesses PostgreSQL or Redis. Never restore SSH query fan-out.
- Use the official Rust MCP SDK (rmcp) for the stdio server and call the same typed application core as the CLI.
  Keep CLI argument parsing/output and MCP protocol handling in thin wrappers;
  MCP must not construct CLI arguments or call CLI handlers.
- Keep concrete local FastEmbed and remote HTTP embedding options with exact
  cosine search, RRF, and bounded local reranking. Support Voyage and the
  OpenAI-compatible embeddings API without provider traits or registries.
- Do not add provider traits, registries, installed daemons, ANN indexes, compatibility
  shims, alternate output modes, a TUI, export, or analytics.
- CLI searches may share an on-demand, per-user local worker. It must exit after
  60 seconds of inactivity, accept only searches, and preserve direct execution
  with `search --no-worker`. Do not install it as a system service.
- Support opt-in debounced synchronization with `index --watch` and MCP startup
  `--watch`. Keep it within the running process; do not install a daemon.
- Keep ingestion to the two concrete mash-owned JSONL parsers. Do not add
  external application databases, event-log providers, provider traits,
  registries, plugin systems, or compatibility layers.
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
and never panics. Await PostgreSQL I/O and model-work boundaries. Keep Rusqlite,
parsing, and local inference on blocking workers; bound model work and serialize
MCP mutations while permitting read concurrency. Add a dependency only when it
is smaller and clearer than a local implementation of the required behavior.

Use typed Rust models for known serialized schemas throughout the application.
Use enums for providers, roles, phases, states, modes, and other known alternatives;
preserve unknown variants when external vocabularies are extensible. Keep generic
JSON limited to opaque provider tool payloads and SDK boundaries requiring it.

Use `apply_patch` for source edits. Preserve unrelated worktree changes. Do not
create compatibility copies or versioned replacement files.

## Verification

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

Use `cargo build-clif`, `cargo run-clif`, and `cargo test-clif` for development
iteration. Retain Clippy, formatting, and doctest validation. On the installed
August 2026 nightly, Clippy needs `RUSTFLAGS=-Znext-solver=no` to avoid its known
async-code compiler crash.

## Git

Work on and push `main`; it is the repository's sole development branch.
