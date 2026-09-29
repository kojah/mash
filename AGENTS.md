# mash development instructions

## Product boundary

mash is a JSON-only Rust CLI and stdio MCP server for Claude Code and Codex JSONL histories.
Keep the CLI command surface limited to `index`, `search`, `view`, `status`,
`forget`, `models install`, `mcp`, and `serve`.

- Use Rusqlite as the sole SQLite library.
- Use SQLite FTS5 for local lexical search.
- Use an authenticated Axum HTTP API backed by tokio-postgres and pgvector for one shared index per configured
  node group. Support local-only, shared-only without SQLite, and both indexing,
  with independent provider allowlists. Keep local operations offline-capable.
  Only the HTTP service accesses PostgreSQL. Never restore SSH query fan-out.
- Use the official Rust MCP SDK (rmcp) for the stdio server and share CLI operations.
- Keep one concrete FastEmbed semantic backend with exact cosine search, RRF,
  and bounded reranking.
- Do not add provider traits, registries, daemons, ANN indexes, compatibility
  shims, alternate output modes, a TUI, export, or analytics.
- Support opt-in debounced synchronization with `index --watch` and MCP startup
  `--watch`. Keep it within the running process; do not install a daemon.
- Keep ingestion to the two concrete mash-owned JSONL parsers. Do not add
  external application databases, event-log providers, provider traits,
  registries, plugin systems, or compatibility layers.
- Never download models outside the explicit `models install` command.
- Unsafe Rust is forbidden.

## Implementation style

Prefer concrete functions and structs. External input returns typed JSON errors
and never panics. Await PostgreSQL I/O and model-work boundaries. Keep Rusqlite,
parsing, and local inference on blocking workers; bound model work and serialize
MCP mutations while permitting read concurrency. Add a dependency only when it
is smaller and clearer than a local implementation of the required behavior.

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
