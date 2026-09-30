# mash

Agent work is tracked with [Beads](https://github.com/steveyegge/beads). With `bd`
and Dolt installed, run `bd bootstrap` after cloning, then `bd prime` and `bd ready`.
Issue history lives on a separate Dolt Git remote in this GitHub repository;
`bd dolt commit` and `bd dolt push` persist tracking updates. Configuration is
versioned; database files and credentials remain local.

mash (Multi-Agent Search History) searches Claude Code and Codex JSONL histories
through a JSON CLI and an MCP server built with the official Rust SDK. Nodes can
index locally, into a shared service over HTTP, or both, with independent provider allowlists.
SQLite FTS5, local FastEmbed embeddings, reciprocal-rank fusion, and bounded
cross-encoder reranking provide
hybrid retrieval. Nodes optionally publish to a joint shared index, one per
node group, so shared history remains searchable while source machines are offline.
The service uses Redis Search by default, with PostgreSQL + pgvector as an alternative.

## Commands

Database query models are generated with Cornucopia 1.0.1 for PostgreSQL and
sqlc 1.31.1 / sqlc-gen-rust 0.1.12 for Rusqlite. Generated output is checked in;
normal Cargo builds require neither generators nor a running database.
To regenerate as part of a build, install these generator versions and run
`make build CODEGEN=1` (add `CARGO_ARGS='--release --locked'` for release).
`make generate-queries` regenerates without compiling; `make check-generated`
checks for stale output. PostgreSQL validation uses a disposable pgvector Docker
container, or a temporary database on `MASH_CODEGEN_POSTGRES_URL` (server URL
without a database path). SQLite generation reads the same schema SQL used at
runtime. CI checks regeneration against the committed output.

Generated typed reads cover retrieval, viewing, status, and shared freshness/state.
Runtime filter predicates and FTS ranking retain custom SQL; dynamic PostgreSQL
rows use column names and the generated result model, while SQLite reuses the
generated projection and decoder. Schema migrations and low-level vector/storage
writes remain explicit driver operations.

```text
mash index [--full|--watch [--debounce-ms N]] [--provider claude-code|codex]... [--since-days N|--all-history]
mash search <query> [--unit threads|messages] [--local | --shared] [--no-worker] [--limit N] [--provider PROVIDER] [--days N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status [--shared]
mash forget <conversation-id>
mash models install
mash mcp [--watch [--debounce-ms N]]
mash serve --server-config PATH
```

One-shot data commands emit one JSON value; watch mode emits newline-delimited
results, and `mash mcp` serves the stdio protocol. `mash serve` runs the shared
index HTTP API; it reports its listening address as JSON on stderr and keeps
stdout empty. Bare `mash` prints help. Global
options are `--config PATH`, `--db PATH`, and
`--models-dir PATH`.

On Linux and macOS, CLI searches automatically share an on-demand local worker
for the same configuration, database, model directory, and credential environment.
The first search loads models; subsequent commands reuse the models and shared
HTTP connections, even for different queries. The worker exits after 60 seconds
without requests and releases its memory. Concurrent searches queue through the
same worker. It accepts only searches through a private per-user Unix socket;
no system service is installed.

Use `mash search "query" --no-worker` to run entirely within that CLI process.
If the worker cannot be reached, search runs directly. Configurations and index
data are read for every request; results are not cached. MCP sessions continue
to reuse models within their own process and do not launch CLI workers.

## Models and indexing

Search is semantic by default and has no production lexical-only mode. Install
models explicitly, then build the local index:

```bash
mash models install
mash index
```

Initial indexing is explicit. CLI and MCP search never ingest history, build an
index, or migrate its schema. A missing or incomplete local index returns a typed
error recommending `mash index`, before loading inference models or contacting
the shared search service. An explicitly completed empty index is valid.

`mash index` reports live progress on stderr, including file/vector counts,
elapsed time, and an estimated remaining time for the current phase. Estimates
use recent throughput and adjust as the workload changes; they are approximate,
especially when message lengths vary. Ingestion estimates cover the current
provider; independently routed shared embeddings estimate the current conversation.
Model loading, search rebuilding, snapshot preparation, and server
commit show activity with an unknown ETA until work can be measured. Shared
uploads report bytes and an upload ETA; completion is reported only after the
server acknowledges publication (or `shared-pending` if publication fails).
Interactive terminals show an indicatif progress bar or spinner; redirected stderr retains
newline-delimited JSON with `eta_seconds` (`null` when unknown) and `eta_scope`.
The final command response stays JSON on stdout. Watching remains opt-in with
`index --watch` or `mcp --watch`.

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, updates FTS incrementally for small changes, embeds only
new or changed searchable messages, and excludes raw tool output from FTS and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Batches select a 64-, 128-,
256-, or 512-token session using the model's tokenizer, retaining the 512-token
limit for long messages. Each shape compiles on first use and is cached on disk;
at most four sessions are retained during indexing. Other targets use the
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

mash reads `config.toml` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned TOML. Provider presence enables that provider;
roots must be absolute. `index.since_days` defaults to 90. Set
`index.all_history = true` instead for all history; these settings cannot be
combined. CLI indexing flags override these settings.

```toml
version = 1

[providers.claude-code]
roots = ["/home/james/.claude/projects"]

[providers.codex]
roots = ["/home/james/.codex/sessions"]

[index]
since_days = 90
destination = "both"

[index.local]
providers = ["claude-code", "codex"]

[index.shared]
providers = ["codex"]

[shared]
group_id = "personal"
node_id = "xenia"
api_url = "https://mash.example.net"
token_env = "MASH_NODE_TOKEN"
```

This example keeps both providers in SQLite and forwards only Codex to the shared service.
SQLite is required on every node. `index.destination` accepts `local` or `both`;
`both` requires the top-level `shared` connection settings. Without an explicit
destination, mash selects both when shared settings exist, and local otherwise.
The local provider allowlist must contain every provider in the shared allowlist.
Shared-only configurations are rejected.

Each destination's optional `providers` list is an allowlist of enabled providers.
Omit a destination filter to include all enabled providers; `providers = []` sends
none to that destination. `index --provider` further restricts these lists and
cannot bypass them. Filters control subsequent ingestion; narrowing them does
not automatically erase previously indexed history. Use `forget` for deletion.

Omit the top-level `shared` for a fully local installation. Search defaults to SQLite plus other nodes in the shared group when shared settings exist; otherwise it uses SQLite. Configure the same
group ID and API URL on every member, with a distinct node ID and token for each
publisher. The service authorizes each token for a fixed group and node. Group IDs select
separate PostgreSQL schemas; every node within a group contributes to one joint
index. The database can live wherever you choose.

The former `nodes`, `local_node`, SSH destinations, `--local-node`,
and hidden federation workers are removed. To migrate a configuration, move this
machine's `providers` and `index` settings to the top level and optionally add
`shared`. Old configuration shapes are rejected rather than guessed.

## Search filters

CLI search accepts repeatable `--provider`, `--node`, `--role`, `--conversation`,
`--project`, and `--repository`. Repeated values for one field are alternatives;
different fields are combined with AND. `--node` filters publishing identity and
does not connect directly to another node. A standalone local installation uses
the node label `local`. `--days` is a rolling message-age window. `--after` is
inclusive and `--before` exclusive; both accept `YYYY-MM-DD` (UTC midnight) or
RFC3339 timestamps with an explicit offset.

```bash
mash search 'database migration' --provider codex --provider claude-code \
  --role user --role assistant --after 2026-09-01 --before 2026-10-01
mash search 'deployment' --repository https://github.com/kojah/mash.git \
  --node dev-macbook --node xenia
```

For Boolean combinations, use `--filter` with a structured JSON expression, or
the MCP search tool's `filter` argument. This is a query expression; configuration
files remain TOML. Each expression contains exactly one operator:

- `all`: AND of expressions; an empty list matches everything.
- `any`: OR of expressions; an empty list matches nothing.
- `not`: negation of one expression.
- `node`, `provider`, `role`, `conversation`, `project`, `repository`: lists of exact, case-sensitive values.
- `after`, `before`: inclusive/exclusive Unix timestamps **in seconds**.

An MCP request can select either repository while excluding tool messages:

```json
{
  "query": "database migration",
  "filter": {
    "all": [
      {"provider": ["codex", "claude-code"]},
      {"any": [
        {"repository": ["https://github.com/kojah/mash.git"]},
        {"project": ["/home/james/scratch/mash"]}
      ]},
      {"not": {"role": ["tool"]}}
    ]
  }
}
```

The CLI accepts the same expression through `--filter '...JSON...'`. Convenience
flags and the structured expression are combined with AND. Filters apply before
lexical/semantic candidate limits on SQLite, PostgreSQL, and Redis, then the
combined results are reranked. Raw SQL, regular expressions, and glob patterns
are not supported. Limits are eight levels of nesting, 128 expressions, and 100
values per field list.

Project means the session's first recorded working directory. Repository means
the session's logged Git repository URL, or the locally available Git origin for
that directory at ingestion time. They are conversation metadata; the first
directory applies to the whole session. Unavailable metadata remains unknown,
so a positive filter does not match it and a negated filter does. Date filters do
not match messages without timestamps. Schema upgrades preserve history and
embeddings and invalidate local ingestion checkpoints, allowing the next
`mash index` to capture metadata from source files. Independently routed shared
ingestion also refreshes its source checkpoints when upgrading metadata support.

## Shared PostgreSQL index

Nodes access the shared index through an authenticated Axum HTTP service. Only
that service connects to PostgreSQL, using async `tokio-postgres` and a bounded
`deadpool-postgres` connection pool (16 connections by default). There is one
joint index per group, with node ownership enforced by the service.

Provision PostgreSQL with the pgvector extension installed. For local development:

```bash
docker run --rm --name mash-postgres \
  -e POSTGRES_PASSWORD=dev-password -e POSTGRES_DB=mash \
  -p 127.0.0.1:5432:5432 pgvector/pgvector:pg17
```

The HTTP server can run on any host with database connectivity; mash does not
choose or provision a host. Create a separate server configuration:

```toml
version = 1
listen = "127.0.0.1:8787"
backend = "postgres"
database_url_env = "MASH_DATABASE_URL"
max_connections = 16

[[nodes]]
group_id = "personal"
node_id = "xenia"
token_env = "MASH_XENIA_TOKEN"

[[nodes]]
group_id = "personal"
node_id = "macbook"
token_env = "MASH_MACBOOK_TOKEN"
```

Use a different randomly generated token for each node (at least 32 printable
ASCII bytes). Supply the same token to that node through its configured
`shared.token_env`. Tokens and database credentials are environment variables,
not values in TOML configuration or status output. Group membership comes from
the server's `nodes` list; restart the service to change membership or rotate a
token. A node can search and view its whole group, but can only publish or forget
its own data. One service can host multiple groups with separate schemas.

```bash
# On the service host; set the node token variables from your secret store too.
export MASH_DATABASE_URL='postgresql://USER:PASSWORD@HOST/DATABASE?sslmode=require'
mash serve --server-config /absolute/path/to/server.toml

# On a node, set MASH_NODE_TOKEN and configure shared.api_url before running:
mash index
mash search 'sqlite migration' --shared
mash view MESSAGE_ID --shared xenia --context 2
```

The service enables `CREATE EXTENSION vector` and migrates each configured group
at startup. Its database role needs extension/schema creation and table-write
privileges; alternatively an administrator can preinstall pgvector. Existing
array-based shared indexes migrate to `vector(384)` without dropping history.
Incomplete old vectors become NULL and require reindexing by their source nodes.
Old node configuration containing `database_url_env` is rejected: replace it
with `api_url` and `token_env`; retain database credentials only on the service.

PostgreSQL TLS verifies certificates and hostnames using public CA roots by
default. Explicit `sslmode=disable` supports trusted local development instances.
The service binds to `127.0.0.1:8787` by default. Use an HTTPS reverse proxy for
remote access, or explicitly configure an HTTP address on a trusted private
network. Nodes accept HTTP(S) URLs, with optional path prefixes, and disable
redirects to keep credentials at the configured endpoint.

The API exposes `/v1/index` (NDJSON batches), `/v1/state`, `/v1/status`,
`/v1/freshness`, `/v1/search`, `/v1/view`, and `/v1/forget`. Requests use
`Authorization: Bearer TOKEN`; mash also sends its expected group/node headers.
These are domain operations rather than an arbitrary SQL endpoint. Search
requests carry a query embedding; responses contain lexical/vector candidates
for node-side fusion and reranking. The HTTP server needs no model assets.

Uploads are spooled into temporary files with restrictive permissions and
deleted when the request finishes. Nodes finish parsing and inference before
uploading; the service receives the entire bounded stream before opening its
transaction. Interrupted or invalid uploads leave data and freshness unchanged.
Uploads are limited to 128 records per batch, 8 MiB per event and 8 GiB per
request; ordinary JSON requests are limited to 64 KiB. Request timeouts and pool
limits bound work. Source checkpoints use revisions to reject concurrent stale
uploads; retry indexing after a conflict. Full snapshot retries are idempotent.

### Redis backend (default)

Redis is the default HTTP service backend. Set `redis_url_env` and omit `backend`,
or select `backend = "redis"` explicitly. Select `backend = "postgres"` for
PostgreSQL/pgvector. Node configuration and MCP tools
stay the same; nodes still use an API URL and their node token. One service uses
one storage backend for all its groups. Changing the backend creates a separate
index: reindex from node histories to populate it; mash does not automatically
copy data between PostgreSQL and Redis.

```toml
version = 1
listen = "127.0.0.1:8787"
backend = "redis"
redis_url_env = "MASH_REDIS_URL"

[[nodes]]
group_id = "personal"
node_id = "xenia"
token_env = "MASH_XENIA_TOKEN"
```

Use Redis with Search support, such as Redis 8 or Redis Stack, rather than a
key-value-only Redis installation. A local development instance can run with:

```bash
docker run --rm --name mash-redis -p 127.0.0.1:6379:6379 \
  redis:8.2 redis-server --appendonly yes --maxmemory-policy noeviction
export MASH_REDIS_URL='redis://127.0.0.1:6379/'
mash serve --server-config /absolute/path/to/redis-server.toml
```

The service uses the `redis-rs` (`redis`) crate with async multiplexed connections
and reconnection support. Use `rediss://` for TLS outside a trusted private
network. Keep database credentials on the service. Configure persistence and
`noeviction`: the Redis backend is a shared index, rather than an evictable cache.
This implementation targets a single Redis instance; it does not configure a
Redis Cluster deployment.

Each group gets a separate full-text/vector index. Redis uses exact `FLAT` cosine
search over the same 384-dimensional quantized embeddings, followed by node-side
RRF and reranking. Provider/age filters, context-only tool output, per-node
ownership, checkpoints, tombstones, and freshness are supported. Its lexical
retrieval uses Redis token matching and ranking; PostgreSQL web-search operators
are not interpreted as operators by the Redis backend. Consequently lexical
candidates can differ between backends.

For atomic publication, the service stages a complete changed node revision and
then switches its visible metadata with a compare-and-swap Lua operation. Search
and view use atomic Lua reads restricted to current revisions; partially staged
or retired data cannot appear in results. Failed uploads leave the visible data
and freshness unchanged. Superseded records are unlinked at commit; abandoned
stages expire after ten minutes. Redis supplies synchronization timestamps.

Each upload and each node's serialized canonical snapshot is limited to
**64 MiB by default**. Set `redis_max_node_mib` in the server configuration
to raise that bound (1–4096 MiB). Size Redis memory for all nodes' canonical
data, searchable documents, vectors, and temporary staging copies; this limit
does not reserve memory or change Redis's `maxmemory` setting.
Canonical snapshots use bounded 8 MiB Redis values with a checksummed manifest.
Set `max_concurrent_uploads` (default 4, range 1–16) to bound concurrent staging
work; large histories should use 1. Staging changed revisions uses extra memory and writes,
and the service loads canonical node data when applying changes; unchanged data
can reuse the existing stage. These are deliberate limits on the initial
backend. Benchmark actual search latency, update cost, memory use, and retrieval
quality for a group. See [the measured comparison](benchmarks/2026-09-30-shared-index.md).

In both mode without destination filters, local indexing commits first. Changes
mark a durable local publication revision;
a changed node publishes its complete snapshot in one PostgreSQL transaction,
using batches of up to 128 records or about 1 MiB. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally; `status --shared` contacts the API.

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

Default search queries SQLite and shared records belonging to other nodes. The
shared service excludes the authenticated node before candidate limits and
embedding-readiness checks. After computing one query embedding, local candidate
retrieval and the shared HTTP request run concurrently. Both sources use independent
lexical/semantic reciprocal-rank fusion, followed by bounded combined reranking.
Local message hits have no `node_id`; shared hits include their
publishing node for `view --shared NODE_ID`. Plain `view` always reads SQLite.
Interactive shared reads have a two-second HTTP deadline. Fetching nearby shared
context has a one-second total budget, preserving completed responses and using
available message evidence for the rest. Transport failures appear in shared
search status; combined search retains local results. Streaming publication has
a separate ten-minute budget.

Search defaults to `unit = "threads"` in CLI and MCP. Each thread result includes
its conversation ID, provider, representative node, available title/project/repository
and conversation timestamps, a ranking score, and up to three distinct matching
excerpts in relevance order. Each excerpt has a `message_id`, its own `node_id`,
role, timestamp, and relevance scores. Open an excerpt with `view MESSAGE_ID
--context N`, supplying `--shared NODE_ID` for a shared excerpt. Timestamps are
Unix milliseconds; unknown metadata is null. Search never returns an entire
conversation automatically.

Local lexical search treats query text as literal words, including URLs, dotted
names and hyphenated identifiers; double quotes group a lexical phrase. Punctuation
does not become SQLite FTS syntax. Queries must contain a letter or number and
fit within 16,384 bytes. Explicit PR/issue references require that exact reference
in each returned excerpt/message, so `PR 180` cannot be replaced by `PR 183`
or an incidental numeric value of 180. Use the metadata filters for Boolean scopes.
See [the search quality audit](docs/search-audit-2026-09-30.md) for measured failures
and remaining relevance limitations.

Thread retrieval considers up to 200 lexical and 200 semantic candidates per
index. It keeps up to six distinct candidate messages per conversation. A first
reranking pass evaluates up to 20 grouped thread documents, then individual
evidence is reranked for five to ten promising threads (at most 60 messages).
Each evidence document includes bounded text from its two nearest searchable
neighbours. Threads are grouped before applying `limit`;
their score is reciprocal rank of the best qualifying message plus a bounded
bonus for up to two additional qualifying excerpts. The bonus is smaller than
the gap to the next message rank, so weaker matches cannot push a thread above
one with stronger primary evidence. Scores express relative relevance, not probabilities.
Lexical matches and verified exact PR/issue references remain eligible outside
the reranking budget. Other semantic-only
matches qualify only after being evaluated by the reranker; unreranked neighbours
do not fill out thread results. Raw model logits are not calibrated probabilities,
so a negative score alone does not disqualify a semantic paraphrase. A query may
return fewer threads than requested. Message mode applies the same evidence gate.
Recency does not change ranking; use date filters to restrict history.
Copies with the same provider and conversation ID are grouped across nodes;
identical role and whitespace-normalized message content earns no additional credit.
Replicas are collapsed before fusion and thread scoring; each excerpt's `copies`
lists alternate message/node references for viewing the same evidence. Independent conversations
with different IDs remain separate.

Use `mash search QUERY --unit messages` or MCP `unit = "messages"` for full
individual message results with the existing 50-candidate and ten-message
reranking budgets. `limit` counts the selected unit, and filters apply equally
to both modes before retrieval. Thread excerpts are bounded to 800 characters
plus ellipses, with a window around a query term when present. Results describe
the bounded candidate pool, not an exhaustive count of matches in each chat.
On server startup, older Redis snapshots receive a resumable metadata repair from
their canonical records without changing their revision or last-updated timestamp.
Unknown upstream metadata remains null.

Reranking uses batches of up to four documents grouped by length, reducing
inference overhead and padding between short and long candidates.
For messages longer than 2,400 characters, reranking uses a bounded window around
the first query term (or the start for semantic-only matches), keeping late lexical
evidence visible instead of always truncating the message's opening. `view` still
returns the canonical message.
Search projections exclude system/developer/tool messages, dedicated skill bodies,
and known injected instruction/context blocks. Ordinary Markdown examples remain
searchable. `view` retains canonical provider history. Existing candidates are
also filtered at query time; the next explicit `mash index` rescans source files
to refresh stored projections and titles. Shared-only indexing similarly refreshes
its source checkpoints once. Search never performs that refresh itself.

Use `search --local` to avoid the shared API, or `search --shared` for the complete
shared index, including this node. MCP search exposes equivalent `local` and
`shared` booleans. An API failure during default search returns local results
with `realized_mode = "local-hybrid"` and `shared_error`; explicit shared search
returns an error. Incomplete local embeddings still require `mash index`.
Models remain local and are installed only by `models install`.

For unfiltered both mode, full snapshot replacement favors correctness and
simplicity over upload throughput. Large histories incur a full transfer on each changed revision, and
exact cosine scans the shared corpus. There is no ANN index or automatic shared service deployment. The API keeps the
current exact retrieval behavior; add approximate indexing only after measuring
the corpus size, latency, and recall tradeoffs.

## Remote embeddings

Local FastEmbed inference remains the default. Add `embeddings` to the node
configuration to use Voyage-4 for both document and query embeddings:

```toml
[embeddings]
api = "voyage"
url = "https://api.voyageai.com/v1/embeddings"
model = "voyage-4"
dimensions = 1024
token_env = "VOYAGE_API_KEY"
```

Set `VOYAGE_API_KEY` in the node's environment. It is separate from the shared
index token; the index service never receives it. History search projections
and search queries are sent to the configured inference endpoint. There is no
automatic fallback to another model. `model` is configurable, including
`voyage-code-4`; Voyage dimensions can be 256, 512, 1024, or 2048. Requests use
`input_type: document` for indexing and `input_type: query` for searching.
Overlong inputs fail explicitly rather than being silently truncated.

For an OpenAI-compatible endpoint, set `api` to `openai-compatible` and provide
its complete embeddings URL, model, dimensions, and optional `token_env`.
Set `request_dimensions = true` if the endpoint should receive the `dimensions`
parameter. Fixed-dimension self-hosted endpoints can omit it. Mash supports
1–2048 dimensions and validates vector count, ordering, dimensions, and values.
HTTP requests are asynchronous, bounded, and time out after 60 seconds. Local
SQLite pipelines remain on blocking workers while remote requests run on Tokio.

Remote indexing needs no local model assets. Reranking remains local; run
`mash models install` with remote embeddings configured to install only the
reranker before searching. No models are downloaded by indexing or searching.

For shared indexing, configure the service's embedding space as well:

```toml
[embeddings]
model = "voyage-4"
dimensions = 1024
```

The service configuration applies to every group it serves. Publishers and
searchers must use the matching model and dimensions. Changing them rebuilds
local embeddings on the next index pass; reindex all shared publishers before
searching. PostgreSQL startup migrates existing vectors without deleting history.
Redis indexes have fixed dimensions: use a new group ID when changing dimensions,
then reindex its members. Model identifiers and dimensions prevent mismatched
queries from being compared with the stored embedding space.

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
missed events. No models are downloaded automatically; every node retains SQLite.

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
read-only; the service adds missing metadata tables during startup migration.

## MCP

CLI and MCP are wrappers around the same typed application core in `app/core.rs`.
The core owns indexing, search, view, status, forgetting, and model installation.
MCP calls it directly without constructing CLI arguments or invoking CLI handlers.
The cached search worker and watcher also use typed core requests.

Run `mash mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir`, `--watch`, and `--debounce-ms` at startup and exposes `search`, `view`, `status`, `index`, `forget`,
and `models_install`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`.

To stream an `index` tool call's progress, include a string or numeric
`params._meta.progressToken` in the `tools/call` request. The server sends
standard `notifications/progress` with the same token while the job runs,
including when it waits for another mutation. `message` contains the phase,
counts, elapsed time, and phase ETA. Structured progress fields are also available
under `params._meta["dev.jamesqo.mash/index-progress"]` in each notification.
The numeric `progress` is an increasing update counter across all phases;
`total` is omitted because the whole job's work is unknown. Phase-specific
totals and `eta_seconds` are in the structured metadata. Unknown ETAs are `null`.

Progress delivery uses a bounded queue and skips intermediate updates if a
client is slow; a disconnected client does not block indexing. The normal
tool result follows progress and retains the CLI's success/error structure.
Without a progress token, the tool returns only its final result. CLI progress
still goes to stderr; MCP stdout remains exclusively SDK protocol messages.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.toml"]
    }
  }
}
```

For shared operations, provide the configured node token environment variable to
`mash mcp` through your MCP client's environment mechanism. `search` accepts a
`shared` boolean and a `unit` of `threads` (default) or `messages`; `view` accepts a `shared` publishing node ID. Search, view, and
status are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. Shared node I/O uses async HTTP;
service-side PostgreSQL I/O uses `tokio-postgres` and async/await. SQLite, parsing, and local FastEmbed inference run on blocking
workers and are awaited by callers. MCP mutations serialize per session; status
and view may run concurrently, while model-heavy operations use a single permit
to bound memory and CPU use. Remote embeddings use the same node configuration
for CLI and MCP operations.

## Code organization

CLI and MCP translate inputs into typed application-core requests. Implementation
modules are grouped by responsibility:

- `app/ingestion/`: history discovery, typed provider records, and normalization;
  `app/ingestion.rs` coordinates ingestion and checkpoints.
- `app/semantic/`: model installation/cache, embedding batches, Core ML, remote
  embeddings, and hybrid retrieval.
- `app/shared/redis_index/`: canonical upload validation, atomic publication,
  and read queries; the parent module handles connections and schema setup.
- `app/tests/`: CLI contract tests grouped by behavior, with common setup in
  `support.rs`. CLI and MCP share marker fixtures from `app/test_support.rs`.

Keep assertions in individual tests. Share setup for repeated fixtures, and
split independent cases so each failure names the behavior that broke.

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

The storage integration tests are ignored by default. PostgreSQL tests require a disposable
database with pgvector installed in `MASH_TEST_PG_URL`. Tests start real HTTP
services and create/remove isolated test-group schemas:

```bash
MASH_TEST_PG_URL='postgresql://USER:PASSWORD@localhost/DATABASE?sslmode=disable' \
  cargo nextest run --run-ignored only postgres_publication

MASH_TEST_REDIS_URL='redis://localhost:6379/' \
  cargo nextest run --run-ignored only redis_publication
```

The real-model integration test is ignored by default and requires
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.


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
