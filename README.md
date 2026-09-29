# mash

mash (Multi-Agent Search History) searches Claude Code and Codex JSONL histories
through a JSON CLI and an MCP server built with the official Rust SDK. SQLite is the canonical store; SQLite FTS5, local FastEmbed
embeddings, reciprocal-rank fusion, and bounded cross-encoder reranking provide
hybrid retrieval. Nodes optionally publish to a joint PostgreSQL index, one per
node group, so shared history remains searchable while source machines are offline.

## Commands

```text
mash index [--full] [--provider claude-code|codex]... [--since-days N|--all-history]
mash search <query> [--shared] [--limit N] [--provider PROVIDER] [--days N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status
mash forget <conversation-id>
mash models install
```

All operational commands emit one JSON value. Bare `mash` prints help. Global
options are `--config PATH`, `--db PATH`, and
`--models-dir PATH`.

## Models and indexing

Search is semantic by default and has no production lexical-only mode. Install
models explicitly, then build the local index:

```bash
mash models install
mash index
```

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, updates FTS incrementally for small changes, embeds only
new or changed searchable messages, and excludes raw tool output from FTS and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Other targets use the
quantized ONNX CPU model. After upgrading an existing Mac installation, rerun
`mash models install` once to install the FP32 asset and compile its Core ML
cache.

Without a configuration file, mash indexes these built-in roots when present:

- Claude Code: `~/.claude/projects`, `~/.config/claude/projects`
- Codex: `~/.codex/sessions`, `~/.local/share/codex/sessions`

## Configuration

The rename changes the default configuration and data directories to `mash`,
and the database filename to `mash.sqlite3`. To reuse an existing achi installation,
pass its paths with `--config`, `--db`, and `--models-dir`, or move its configuration,
database, and installed models into the new locations reported by `mash status`.

mash reads `config.json` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned JSON. Provider presence enables that provider;
roots must be absolute. `since_days` defaults to 90 and may be `null` for all
history. CLI indexing flags override these settings.

```json
{
  "version": 1,
  "providers": {
    "claude-code": {"roots": ["/home/james/.claude/projects"]},
    "codex": {"roots": ["/home/james/.codex/sessions"]}
  },
  "index": {"since_days": 90},
  "shared": {
    "group_id": "personal",
    "node_id": "xenia",
    "database_url_env": "MASH_DATABASE_URL"
  }
}
```

Omit `shared` for a fully local installation. A configured shared connection
never changes the default search scope. Configure the same group ID and database
on every member, with a distinct node ID for each local database. Group IDs select
separate PostgreSQL schemas; every node within a group contributes to one joint
index. The database can live wherever you choose.

The former `nodes`, `local_node`, SSH destinations, `--node`, `--local-node`,
and hidden federation workers are removed. To migrate a configuration, move this
machine's `providers` and `index` settings to the top level and optionally add
`shared`. Old configuration shapes are rejected rather than guessed.

## Shared PostgreSQL index

Provision a PostgreSQL database and supply its connection string through the
configured environment variable. Credentials are not stored in the JSON config,
returned by status, or echoed in connection errors. TLS with certificate and
hostname verification is required by default, using public CA roots. Explicit
`sslmode=disable` is supported for a trusted local development instance.

```bash
export MASH_DATABASE_URL='postgresql://USER:PASSWORD@HOST/DATABASE?sslmode=require'
mash index
mash search 'sqlite migration' --shared
mash view MESSAGE_ID --shared xenia --context 2
```

The first publication creates the group's schema and tables; the database role
must have schema-creation and table-write privileges. This initial design is for
trusted nodes with direct database access. Group namespaces prevent accidental
mixing; they are not an authorization boundary against a database user who can
access other groups' schemas.

Local indexing commits first. Changes mark a durable local publication revision;
a changed node publishes its complete snapshot in one PostgreSQL transaction,
using batches of up to 128 records or about 1 MiB. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally and does not contact PostgreSQL.

`forget` deletes a locally owned conversation and publishes a persistent
shared tombstone. Check its `shared.state`: an offline deletion remains pending
until a later successful `index` or `forget`. Copies owned by other nodes remain.
Node identity is tied to the local database instance; a recreated database must
use a new node ID. Stale snapshots cannot replace newer accepted revisions.

Shared search uses PostgreSQL full-text retrieval, exact cosine over uploaded
quantized vectors, RRF, and at most ten reranked candidates. The querying node
runs the same installed FastEmbed models locally. CPU and Apple Core ML vectors
share a model-space identifier; outdated or missing embeddings cause an explicit
shared error requiring publishing nodes to reindex. Shared hits include `node_id`
for the matching `view` call. A shared outage does not trigger a silent local
fallback. Local and shared result sets are selected explicitly and are not merged.

Full snapshot replacement favors correctness and simplicity over upload
throughput. Large histories incur a full transfer on each changed revision, and
exact cosine scans the shared corpus. There is no background synchronization,
ANN index, HTTP service, or automatic shared deployment.

## MCP

Run `mash-mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir` at startup and exposes `search`, `view`, `status`, `index`, `forget`,
and `models_install`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`. Progress goes to stderr, leaving
stdout for the SDK's protocol messages.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash-mcp",
      "args": ["--config", "/absolute/path/to/config.json"]
    }
  }
}
```

For shared operations, provide the configured database environment variable to
the server through your MCP client's environment mechanism. `search` accepts a
`shared` boolean; `view` accepts a `shared` publishing node ID. Search, view, and
status are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. Synchronous storage/model work runs
on a blocking worker, serialized per MCP session.

## Verification

For faster development code generation, optionally install the nightly Cranelift
component and use the Cargo aliases:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
cargo build-clif
cargo run-clif -- status
cargo test-clif
```

These aliases select Cranelift for mash in the development profile. Dependencies
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

The installed August 2026 nightly can hit a [Clippy compiler crash](https://github.com/rust-lang/rust/issues/161495)
when checking async code. On that toolchain, use
`RUSTFLAGS=-Znext-solver=no cargo clippy --all-targets -- -D warnings`.

The PostgreSQL integration tests are ignored by default and require a disposable
database in `MASH_TEST_PG_URL`. It creates and removes isolated test-group schemas:

```bash
MASH_TEST_PG_URL='postgresql://USER:PASSWORD@localhost/DATABASE?sslmode=disable' \
  cargo nextest run --run-ignored only postgres_publication
```

The real-model integration test is ignored by default and requires
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.

The Veritas test annotations come from the private `jamesqo/veritas`
repository at the exact revision recorded in `Cargo.toml`. Contributors running
tests need repository access and a GitHub SSH identity that can clone Veritas.

## Automatic deployment

Every push to `main` builds the Linux x86-64 and Apple Silicon binaries in
GitHub Actions, then atomically deploys `mash` and `mash-mcp` to `~/.local/bin` on Xenia,
dev-macbook, and personal-macbook over Tailscale. The macOS binary is
cross-compiled on the Linux runner with `cargo-zigbuild`; destination machines
need neither this repository nor a Rust toolchain.

Deployment requires `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`,
`DEPLOY_SSH_PRIVATE_KEY`, and `DEPLOY_SSH_KNOWN_HOSTS` repository secrets.

## License

See [LICENSE](LICENSE).
