# mash

mash (Multi-Agent Search History) searches Claude Code and Codex JSONL histories
through a JSON CLI and an MCP server built with the official Rust SDK. Nodes can
index locally, into PostgreSQL, or both, with independent provider allowlists.
SQLite FTS5, local FastEmbed embeddings, reciprocal-rank fusion, and bounded
cross-encoder reranking provide
hybrid retrieval. Nodes optionally publish to a joint PostgreSQL index, one per
node group, so shared history remains searchable while source machines are offline.

## Commands

```text
mash index [--full|--watch [--debounce-ms N]] [--provider claude-code|codex]... [--since-days N|--all-history]
mash search <query> [--shared] [--limit N] [--provider PROVIDER] [--days N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status [--shared]
mash forget <conversation-id>
mash models install
mash mcp [--watch [--debounce-ms N]]
```

One-shot data commands emit one JSON value; watch mode emits newline-delimited
results, and `mash mcp` serves the stdio protocol. Bare `mash` prints help. Global
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
  "index": {
    "since_days": 90,
    "destination": "both",
    "local": {"providers": ["claude-code"]},
    "shared": {"providers": ["codex"]}
  },
  "shared": {
    "group_id": "personal",
    "node_id": "xenia",
    "database_url_env": "MASH_DATABASE_URL"
  }
}
```

This example keeps Claude Code in SQLite and sends Codex directly to PostgreSQL.
`index.destination` accepts `local`, `shared`, or `both`. Shared and both require
the top-level `shared` connection settings. Without an explicit destination,
mash selects both when shared settings exist, and local otherwise.

Each destination's optional `providers` list is an allowlist of enabled providers.
Omit a destination filter to include all enabled providers; `providers: []` sends
none to that destination. `index --provider` further restricts these lists and
cannot bypass them. Filters control subsequent ingestion; narrowing them does
not automatically erase previously indexed history. Use `forget` for deletion.

Omit the top-level `shared` for a fully local installation. Search defaults to
local in local/both modes and to PostgreSQL in shared-only mode. Configure the same
group ID and database on every member, with a distinct node ID for each publisher. Group IDs select
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

In both mode without destination filters, local indexing commits first. Changes
mark a durable local publication revision;
a changed node publishes its complete snapshot in one PostgreSQL transaction,
using batches of up to 128 records or about 1 MiB. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally and does not contact PostgreSQL.

`forget` deletes a locally owned conversation and publishes a persistent
shared tombstone. Check its `shared.state`: an offline deletion remains pending
until a later successful `index` or `forget`. Copies owned by other nodes remain.
In local/both modes, node identity is tied to the local database instance; a
recreated database must use a new node ID. Stale snapshots cannot replace newer
accepted revisions.

With per-index filters in both mode, SQLite ingests only the local allowlist and
PostgreSQL independently ingests the shared allowlist from the original history
files. PostgreSQL stores its own source checkpoints. Local work still commits
first and shared failures return pending; retry with `index` while retaining the
original shared history files. Shared forget intents are durably queued in SQLite
when offline and applied before the next shared ingestion. A successful forget
can leave index status pending until `index` catches up other changes.

Shared-only mode never opens or creates SQLite, including for bookkeeping or
retries. PostgreSQL holds source checkpoints and persistent tombstones. Indexing
and deletion require connectivity; failures return typed errors and indexing
rolls back atomically. Retry indexing from the original history files. Shared-only
`status` contacts PostgreSQL and returns `database_path: null`; plain `view`
addresses this node, while `view --shared NODE_ID` can read another group member.
Changing between SQLite-backed and shared-only identity requires a new node ID.
Models still live locally and are installed only by `models install`.

Shared search uses PostgreSQL full-text retrieval, exact cosine over uploaded
quantized vectors, RRF, and at most ten reranked candidates. The querying node
runs the same installed FastEmbed models locally. CPU and Apple Core ML vectors
share a model-space identifier; outdated or missing embeddings cause an explicit
shared error requiring publishing nodes to reindex. Shared hits include `node_id`
for the matching `view` call. A shared outage does not trigger a silent local
fallback. Local and shared result sets are selected explicitly and are not merged.

For unfiltered both mode, full snapshot replacement favors correctness and
simplicity over upload throughput. Large histories incur a full transfer on each changed revision, and
exact cosine scans the shared corpus. There is no ANN index, HTTP service, or
automatic shared deployment.

## Automatic synchronization

Run `mash index --watch` for opt-in synchronization while the process remains
running. It indexes once at startup, then waits for one second of quiet after
history changes before refreshing. Set `--debounce-ms 500` to change that delay
(50–60000 ms). Continuous writes trigger a pass within ten seconds, or the
configured delay if longer. Passes run sequentially; changes during indexing
queue another pass. Existing provider, age, destination, and per-index allowlist
settings still apply. `--full` cannot be combined with `--watch`.

The watcher uses native `notify` filesystem events, ignores file reads and
unrelated files, and notices JSONL creation, modification, replacement, and
deletion. It reloads configuration on each pass and watches for config/model
marker changes. Missing roots are watched through their nearest existing parent.
Removing an authoritative configured root reports an error rather than purging
its history; removing individual sources from an existing root is reconciled.
Failed indexing and pending PostgreSQL publication retry after 30 seconds without
requiring another file change. A reconciliation pass every five minutes recovers
missed events. No models are downloaded automatically, and shared-only still
creates no SQLite database.

Watch mode writes one JSON result per completed pass to stdout (newline-delimited
JSON); progress and typed errors go to stderr. Ctrl-C finishes the active pass
and exits. For MCP, use `mash mcp --watch` with optional `--debounce-ms`; watching
lasts for the MCP session and shares the tool mutation/model concurrency guards.
Its refresh results go to stderr so stdout remains MCP protocol only.

Run `mash status --shared`, or call the MCP `status` tool with `{"shared":true}`,
to inspect `shared_index.nodes` for every publishing node in the configured
group. Each node includes its revision, conversation/message counts, and UTC
timestamps:

- `last_synced_at`: last successfully committed indexing/publication check,
  including a check with no new data.
- `last_updated_at`: last committed data update or deletion. A check with no
  changed data preserves this timestamp.

PostgreSQL supplies these timestamps, which commit atomically with the node's
data. Failed or rolled-back uploads leave them unchanged. They describe received
index data, not the newest source message's timestamp or proof that a node is
currently online. Older nodes can have null timestamps until they publish with
this version. Shared-only status includes this information automatically;
ordinary local/both status remains usable offline. Freshness inspection is
read-only; older shared schemas need one indexing pass to add the metadata table.

## MCP

Run `mash mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir`, `--watch`, and `--debounce-ms` at startup and exposes `search`, `view`, `status`, `index`, `forget`,
and `models_install`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`. Progress goes to stderr, leaving
stdout for the SDK's protocol messages.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.json"]
    }
  }
}
```

For shared operations, provide the configured database environment variable to
the server through your MCP client's environment mechanism. `search` accepts a
`shared` boolean; `view` accepts a `shared` publishing node ID. Search, view, and
status are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. PostgreSQL I/O uses `tokio-postgres`
and async/await. SQLite, parsing, and local FastEmbed inference run on blocking
workers and are awaited by callers. MCP mutations serialize per session; status
and view may run concurrently, while model-heavy operations use a single permit
to bound memory and CPU use. No remote inference backend is currently configured.

## Verification

Use Cranelift for development builds and tests. Install the nightly component
and use the Cargo aliases:

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
GitHub Actions, then atomically deploys `mash` to `~/.local/bin` on Xenia,
dev-macbook, and personal-macbook over Tailscale. The macOS binary is
cross-compiled on the Linux runner with `cargo-zigbuild`; destination machines
need neither this repository nor a Rust toolchain.

Deployment requires `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`,
`DEPLOY_SSH_PRIVATE_KEY`, and `DEPLOY_SSH_KNOWN_HOSTS` repository secrets.

## License

See [LICENSE](LICENSE).
