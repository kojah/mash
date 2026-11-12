# Shared index API and operations reference

[Documentation](../README.md) · [Project home](../../README.md)

Nodes access the shared index through an authenticated Axum HTTP service. That
service owns access to shared Qdrant; local CLI/MCP accesses its local Qdrant directly. There is one
joint index per group, with node ownership enforced by the service.

`POST /v1/chats` lists chat metadata without model inference, message bodies
or vectors. Use the same bearer token as other shared API calls. Its typed JSON
request accepts `provider` and `node` arrays, literal ASCII-case-insensitive
`title_contains`, inclusive `after` and exclusive `before` activity bounds in
Unix seconds, and `exclude_self`. Activity refers to recorded chat `updated_at`;
unknown activity does not match a time bound. Omitted or empty arrays impose no
restriction. Sort by `updated_at` (default), `created_at` or `title`, with
`direction` set to `asc` or `desc` (default). Title ordering uses binary string
order. Missing sort values always come last; provider, chat ID and node break
ties deterministically.

The response contains `chats` with canonical `chat_id`, `provider`,
representative `node_id`, title/project/repository and timestamps in Unix
milliseconds, plus `next_offset`. Copies with the same provider/chat ID are
deduplicated before paging; the best matching copy for the requested sort order
supplies metadata. `limit` defaults to 50 (1–100); `offset` defaults to 0
(0–10000). Pagination stops before advertising an offset beyond 10000; narrow
filters to browse larger collections. Pages are stable while the index is unchanged; updates between page
requests can shift boundaries.

Use `mash list-chats` or the MCP `list_chats` tool for the same metadata
discovery. These thin wrappers call the shared application core. The default
scope combines local SQLite and other nodes in the configured shared index;
`--local`/`local=true` stays offline, and `--shared`/`shared=true` reads only the
shared index including this node. Local and remote reads run concurrently, with
deduplication and ordering applied before the final page. A shared read has a
two-second total deadline; combined discovery returns local metadata with
`shared_error` on failure, while shared-only discovery fails. `shared` in the
response indicates a successful shared read.

Use `list-chats --diagnostics` or MCP `list_chats` with `diagnostics=true`
to include metadata-only artifact selection, requested/completed sources,
shared fallback and listing/merge counts and timings. Diagnostics make no extra
HTTP calls and require no models. `local-metadata` and `shared-metadata` measure
the complete source listing (including pagination); `metadata-merge` measures
deduplication, ordering and final paging. Candidate counts reflect the filtered
source windows, not the entire corpus. Concurrent timings overlap and must not
be summed as wall time. Listing elapsed time excludes startup/configuration
loading. Ordinary responses omit this optional report.

SQLite metadata reads require an existing current schema and never create or
migrate the database. They require no installed models or completed embedding
generation, so metadata already recorded during indexing can be inspected.
This does not make semantic search ready. MCP sort values use `updated_at` and
`created_at`; CLI values use `updated-at` and `created-at`.

For example, orient first and then search within a returned chat:

```bash
mash list-chats --provider codex --title-contains cbioportal --limit 10
mash search 'database migration' --unit messages --provider codex --chat CHAT_ID
```

### Tool calls and results

Claude Code and Codex messages record their tool activity as structured silver
metadata beside the unchanged flattened text. `view` (CLI and MCP, local and
shared) returns it as each message's `tools`; messages without activity omit it:

```json
{"tool_calls":[{"name":"Bash","call_id":"toolu_01A","input":{"command":"ls fixtures","description":"List fixtures"}}]}
{"tool_results":[{"call_id":"toolu_01A","is_error":false},{"call_id":"toolu_01B","is_error":true}]}
```

A message can carry several calls or results. `input` is the provider's arguments
kept verbatim as JSON: Claude `tool_use.input`, or Codex `arguments`/`input`,
which Codex records as a string. Results link to calls by `call_id` within the
chat and keep their output in the message text. `is_error` is present only where
the provider records it (Claude `tool_result.is_error`); Codex results carry
none. Other providers' tool activity is not yet structured. Tool text and
metadata stay out of search projections and embeddings, and output and
publication redaction mask credentials in every string of tool inputs.

Provider interpretation changes require an explicit refresh to capture newly
supported tool activity. History whose source files are gone keeps its retained
text without newly reconstructed metadata. Follow the
[upgrade guide](../upgrades.md) before crossing incompatible database schemas.

The HTTP service stores shared silver in SQLite and shared views in
[Qdrant](#qdrant-shared-storage). Configure `qdrant_url_env` and `qdrant_grpc_url_env`; there is no backend
selector. See [shared storage](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/shared-storage.md)
for the publication sequence, recovery and cutover from Qdrant-only servers.

Each node has a different randomly generated token (at least 32 printable ASCII
bytes), supplied through its configured `shared.token_env`. Server group membership
comes from the `nodes` list; restart to change membership or rotate tokens.
A node can search its whole group and can only publish or forget its own data.
Database credentials belong on the HTTP server, supplied through environment variables.
Use an HTTPS reverse proxy for remote access or HTTP on a trusted private network.
The service binds to `127.0.0.1:8787` by default. Node clients disable redirects.

The API exposes `/v1/sync` (authenticated WebSocket mutation transport),
`/v1/freshness`, `/v1/search`, `/v1/contexts`, `/v1/chats`, `/v1/view` and the
attachment read routes. Nodes plan publication from their `/v1/freshness` entry
and forget through delta uploads. Requests use bearer credentials. `/v1/sync`
carries typed Protobuf envelopes with zstd publication data, durable piece
checkpoints and resume; a first publication of a large history is planned into
independently committed batches, so recent chats become searchable
while the rest uploads; publication, attachment-upload and
progressive-backfill operations share one transfer protocol. The former
`POST /v1/index` (NDJSON) and `/v1/attachment-upload` routes and handlers were
removed in the protocol-matched release. Search carries a query
embedding and returns candidates for node-side fusion and reranking. The HTTP
server needs no model assets. Interrupted or invalid uploads leave the visible
revision and freshness unchanged. Uploads are bounded by 128 records per batch,
64 MiB per event, 8 GiB per request, and the configured `max_upload_mib` (64 MiB
default, range 1–4,096). Ordinary JSON requests are limited to 64 KiB.
Node/revision checks reject stale uploads; full snapshot retries are idempotent.

Set `max_concurrent_uploads` (default 4, range 1–16) to bound concurrent publication
work; large histories should use 1. The same capacity also covers `forget`
mutations. A complete upload or accepted forget operation
keeps its slot until the work succeeds or fails, including if the requester
disconnects or times out. Cancellation does not imply rollback: retry using the
existing node/revision checks. Busy mutations return a retryable HTTP 503;
search and metadata reads do not consume mutation slots. This bounds the number
of mutations, not their total RAM.

Shared silver is the source of truth. A publication is validated and committed
to the silver SQLite database in one transaction, and only then synchronized into
Qdrant: documents are replaced in place per message and the node's views record
is certified for the new silver revision when every chat is written. Until
then the node's views are reported stale and excluded from dense search; there is
no Qdrant staging or conditional head protocol. A node that sees stale views
resends a complete snapshot, which repairs views without changing silver.
Chat lists, `view` and context reads come from shared silver.

If Qdrant data is lost or damaged, start the service with `--replay-views` to
rebuild every configured node's documents and vectors from shared silver before
serving. Replay uses local embedding model assets from `--models-dir`, in the
same 384-dimensional space as publishers and search clients. Replay is the only shared views rebuild; nodes never re-embed
shared history.
Benchmark actual search latency, update cost, memory use, and retrieval
quality for a group. See [the measured comparison](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/benchmarks/2026-09-30-shared-index.md).

In both mode without destination filters, local indexing commits first. Changes
mark a durable local publication revision;
the service commits a changed node's upload to shared silver and then synchronizes its views. Raw tool outputs are uploaded
for context but remain excluded from search. Unchanged revisions are acknowledged
without replacing shared records. A failed upload returns local success with
`shared.state: "pending"`; subsequent `index` runs retry even with no new history.
`status` checks publication state locally; `status --shared` contacts the API.

`forget` deletes a locally owned chat and publishes a persistent
shared tombstone. Check its `shared.state`: an offline deletion remains pending
until a later successful `index` or `forget`. Copies owned by other nodes remain.
In local/both modes, node identity is tied to the local database instance; a
recreated database must use a new node ID. Stale snapshots cannot replace newer
accepted revisions.


Default search queries local Qdrant and shared records belonging to other nodes. The
shared service excludes the authenticated node before candidate limits and
embedding-readiness checks. After computing one query embedding, local candidate
retrieval and the shared HTTP request run concurrently. Both sources use independent
lexical/semantic reciprocal-rank fusion, followed by bounded combined reranking.
Local message hits have no `node_id`; shared hits include their
publishing node for `view --shared NODE_ID`. Plain `view` always reads SQLite.
Shared candidate searches have a five-second HTTP deadline; metadata and context
reads retain a two-second deadline. Fetching nearby shared
context has a one-second total budget, preserving completed responses and using
available message evidence for the rest. Transport failures appear in shared
search status; combined search retains local results. Bulk publication and
retained-history operations have a separate thirty-minute budget, covering
upload, validation and views synchronization. The service uses the same bulk budget. A timed-out
or disconnected requester does not cancel an already admitted mutation: it keeps
its capacity until completion, and views are not reported current until synchronization completes.

Search defaults to `unit = "chats"` in CLI and MCP. Each chat result includes
its chat ID, provider, representative node, available title/project/repository
and chat timestamps, a ranking score, and up to three distinct matching
excerpts in relevance order. Each excerpt has a `message_id`, its own `node_id`,
role, timestamp, and relevance scores. Open an excerpt with `view MESSAGE_ID
--context N`, supplying `--shared NODE_ID` for a shared excerpt. Timestamps are
Unix milliseconds; unknown metadata is null. Search never returns an entire
chat automatically. Every returned message `id` in a `view` response is
itself a view anchor: to keep reading, view the first or last returned message
with the same `--shared` value. `--context` has no upper bound and bodies are
never truncated.

Local lexical search treats query text as literal words, including URLs, dotted
names and hyphenated identifiers; double quotes require contiguous literal words
in each returned message or excerpt. Punctuation
is treated as literal input. Queries must contain a letter or number and
fit within 16,384 bytes. Explicit PR/issue references require that exact reference
in each returned excerpt/message, so `PR 180` cannot be replaced by `PR 183`
or an incidental numeric value of 180. PR and issue references remain distinct;
a complete reference URL also constrains its host and repository.
Use the metadata filters for Boolean scopes.
See [the search quality audit](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/search-audit-2026-09-30.md) for measured failures
and remaining relevance limitations.

Chat retrieval considers up to 200 lexical and 200 semantic candidates per
index. It keeps up to six distinct candidate messages per chat. A first
reranking pass evaluates up to 20 grouped chat documents, then individual
evidence is reranked for the ten most promising chats (at most 60 messages).
Each evidence document includes bounded text from its two nearest searchable
neighbours. Chats are grouped before applying `limit`;
their score is reciprocal rank of the best qualifying message plus a bounded
bonus for up to two additional qualifying excerpts. The bonus is smaller than
the gap to the next message rank, so weaker matches cannot push a chat above
one with stronger primary evidence. Scores express relative relevance, not probabilities.
Lexical matches and verified exact PR/issue references remain eligible outside
the reranking budget. Other semantic-only
matches qualify only after being evaluated by the reranker; unreranked neighbours
do not fill out chat results. Raw model logits are not calibrated probabilities,
so a negative score alone does not disqualify a semantic paraphrase. A query may
return fewer chats than requested. Message mode applies the same evidence gate.
Recency does not change ranking; use date filters to restrict history.
Copies with the same provider and chat ID are grouped across nodes;
identical role and whitespace-normalized message content earns no additional credit.
Replicas are collapsed before fusion and chat scoring; each excerpt's `copies`
lists alternate message/node references for viewing the same evidence. Independent chats
with different IDs remain separate.

Use `mash search QUERY --unit messages` or MCP `unit = "messages"` for full
individual message results with the existing 50-candidate and ten-message
reranking budgets. Copies with the same provider, chat, canonical message
ID, role and whitespace-normalized content occupy one message result, with
alternate view anchors in `copies`. Repeated content with different message IDs
and divergent copies remain separate. Replicas do not earn additional fusion
credit. `limit` counts the selected unit, and filters apply equally
to both modes before retrieval.

Search results are paged. `limit` (1–200) sets the page size and `offset`
(default 0) selects where the page starts in one final ranked list, which holds
at most 200 chats or 50 messages; an offset at or beyond that window is a usage
error. Responses include `next_offset`, the offset of the following page, or
null on the last page. Repeat the same search with that offset to continue.
Candidate retrieval, fusion and both reranking passes use fixed budgets that do
not depend on `limit` or `offset`, and qualification filters apply before the
page is cut, so for an unchanged index the pages of local, shared, combined and
exact-mention searches partition the same ranking that one large page returns.
Boundaries are best effort when the index changes between requests, or when a
combined search's shared source or shared context fetch fails for some pages
and not others (reported by `shared_error` and `shared_context_error`). Chat excerpts are bounded to 800 characters
plus ellipses, with a window around a query term when present. Results describe
the bounded candidate pool, not an exhaustive count of matches in each chat.
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
`shared` booleans. A candidate API failure during default search returns local results
with `realized_mode = "local-hybrid"` and `shared_error`; explicit shared search
returns an error. Later context failures retain the retrieved shared evidence,
`shared = true`, and the merged/shared mode, and report `shared_context_error`
separately. These source flags work independently of opt-in diagnostics.
Incomplete local embeddings still require `mash index`.
Models remain local and are installed only by `models install`.
Installation metadata is checked before loading models: the marker is limited to
1 MiB and 4,096 asset entries, revisions to 128 bytes, and each tokenizer/config
file to 16 MiB. Invalid paths, duplicate entries, wrong local dimensions and
missing or truncated assets return a model error recommending `mash models install`.
Hugging Face snapshot symlinks remain supported. Markers are published atomically
after installation checks, using a unique staging file for each installer.
Only one installer can run against a model directory at a time. A second
`models install` returns a retryable `model-busy` error (exit code 6); retry
after the active installer finishes. The operating system releases the lock
when the installer exits, so an interrupted process does not leave a stale lock.
Installers copy available assets into a separate generation, run SDK checks
there, then atomically select that generation. Failed or interrupted staging
keeps the previous installation available. Searches and indexers retain a lease
on their selected generation; an explicit installation collects abandoned and
retired generations once no reader holds them. The original flat cache is
retained so readers started before the first generation install can finish.
Reinstallation needs disk space for a second copy of the assets. This staging
also isolates mutable SDK revision references from the active cache.
Model downloads always use mash's selected model directory, even when `HF_HOME`
is set for another application. Set `--models-dir` to change mash's model location;
`HF_ENDPOINT` remains available for a Hugging Face mirror.
On Unix, installers flush the asset files and directories before activation,
then flush the marker's directory entry after activation. If that final flush
fails, the retryable error states that the installation is active but its
durability is unconfirmed. Both generations are retained; retry `models install`.
Activation errors also retain prepared assets until a later confirmed cleanup.
Other platforms retain atomic activation without these Unix flush barriers.

For unfiltered both mode, full snapshot replacement favors correctness and
simplicity over upload throughput. Large histories incur a full transfer on each changed revision, and
Qdrant uses HNSW retrieval by default, with explicit exact vector search available.
The API spools each upload to a private temporary file and applies it to shared
silver in one SQLite transaction; temporary disk must hold concurrent uploads.
View synchronization then reads the committed upload and writes Qdrant documents
per chat. Account for concurrent uploads when provisioning the API.
Shared-service provisioning remains explicit. Measure corpus size, latency and
recall tradeoffs before choosing approximate retrieval.

### Qdrant shared storage

Configure `qdrant_url_env` and `qdrant_grpc_url_env` on the HTTP service. Point
publication uses gRPC; reads and collection setup use REST. Both endpoints must
address the same Qdrant instance. Nodes continue using the same
API URL, bearer token, CLI and MCP tools. Qdrant **1.20+ in the 1.x series** is
required; older versions are rejected before creating collections. Shared
silver lives on the API host under `silver_dir` (absolute path; default
`shared-silver` in the platform data directory), one SQLite database per
configured node at `<silver_dir>/<group_id>/<node_id>.sqlite3`, using the same
silver schema as local indexes. Back it up: Qdrant can be rebuilt from it, but
not the reverse. One `mash-server` process owns a `silver_dir`; a second one using
the same directory fails at startup. `mash-server --retire-node GROUP/NODE` takes
that same ownership for a one-off deletion of a node's silver, documents,
transfer receipts and attachments, and exits; see
[Search across your machines](../shared-index.md#remove-a-machine-or-let-a-rebuilt-one-publish-again). Filtered HNSW vector retrieval is the default, with
`ef_runtime = 200`; exact vector retrieval remains an explicit option.

```toml
version = 1
listen = "127.0.0.1:8787"
qdrant_url_env = "MASH_QDRANT_URL"
qdrant_grpc_url_env = "MASH_QDRANT_GRPC_URL"
# Optional for an authenticated Qdrant service:
# qdrant_api_key_env = "MASH_QDRANT_API_KEY"
# silver_dir = "/var/lib/mash/shared-silver"
max_upload_mib = 64
qdrant_memory = "durable" # Default: pinned HNSW/text indexes, cached dense vectors, cold payloads.

[[nodes]]
group_id = "personal"
node_id = "xenia"
token_env = "MASH_XENIA_TOKEN"

[views.query.vector_search]
algorithm = "hnsw"
ef_runtime = 200
# To request exhaustive vector retrieval instead:
# algorithm = "exact" # Omit ef_runtime with exact.

# Optional default for a configured group:
# [views.query.groups.personal]
# algorithm = "hnsw"
# ef_runtime = 80
```

The server's `views.query.vector_search` controls retrieval from the existing
dense index. Defaults are persisted per group. Use `mash-server --server-config
server.toml --activate-query` to intentionally change existing defaults; conflicting
startup settings otherwise fail. Running API processes read the selected policy
on subsequent hybrid searches. This does not rebuild vectors, change views build
identity, or modify silver. The previous
top-level `qdrant_vector_index` table is rejected; move it to the table above.
Groups inherit the server default unless named in `views.query.groups.GROUP`.
Status exposes the selected policy in `group_views.vector_search`. Node-side
`views.query` still controls intent, fusion and result shape independently.

For a single hybrid search, use `--shared-vector-search exact`, or
`--shared-vector-search hnsw --shared-hnsw-ef 80` (effort 1..4096, default 200).
MCP accepts `shared_vector_search: {"algorithm":"hnsw","ef_runtime":80}`.
Overrides affect only shared dense candidates and do not persist or rebuild
views. They require related-discussion intent and configured dense shared views;
local-only and exact-mention requests reject them. Omitting the override uses
the current persisted group default.

```bash
docker run --name mash-qdrant --memory 2g -p 127.0.0.1:6333:6333 -p 127.0.0.1:6334:6334 \
  -v mash-qdrant-data:/qdrant/storage qdrant/qdrant:v1.20.0
export MASH_QDRANT_URL='http://127.0.0.1:6333/'
export MASH_QDRANT_GRPC_URL='http://127.0.0.1:6334/'
# Set MASH_XENIA_TOKEN to the node's existing API token.
mash-server --server-config /absolute/path/to/qdrant-server.toml
```

Shared views settings are persisted per group. Startup with conflicting build/model
settings fails. To deliberately replace the selected settings, update the server
TOML and start with `--activate-views --replay-views`, which re-embeds every node's
shared silver on the API host. Old API settings fail closed, and dense search
requires compatible manifests across the group. This preserves canonical history
but interrupts dense search during replacement. See
[the activation contract](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md#durable-shared-group-configuration)
for publication races and remaining lifecycle gates.
Embedding dimension changes retain the same group and canonical history. Mash
prepares an additional named dense field in the existing collection, then the
same `--activate-views --replay-views` workflow selects and populates it.
The original dense field and lexical data are retained. Preparation alone does
not activate the new model; dense search requires current manifests after the
switch, while compatible metadata and lexical queries remain available.

Use an HTTPS origin and `qdrant_api_key_env` for hosted Qdrant. Endpoints cannot
contain credentials, paths, queries or fragments; redirects are disabled so keys
are not forwarded elsewhere. Shared Qdrant credentials belong on the HTTP server;
local Qdrant credentials are resolved from the node's `qdrant.api_key_env`.
Populating a new shared group requires an explicit `mash index` from each publishing node.
`qdrant_memory` selects a durable Qdrant 1.20+ storage profile. The default,
`durable`, pins the HNSW and word-search text indexes in RAM, preloads dense vectors into the
OS cache and keeps payloads cold on disk; search reads payloads only for returned
hits. The text index takes its placement when it is first built. Qdrant cannot pin dense vectors or payloads. Database files and the
write-ahead log remain on persistent disk. Cached dense vectors can still be
evicted under memory pressure; this is not a volatile RAM filesystem and does
not eliminate ingestion/optimizer disk writes.

The explicit `cached` profile preserves the previous placement: cached dense,
HNSW and text indexes with cold payloads. `cold` uses cold placement for those
components. Payload filter indexes retain Qdrant's own defaults. Pinning requires
enough RAM for the indexes, and optimizer conversion can temporarily increase
resource use.

Changing a profile on an existing group requires explicit activation:

```bash
mash-server --server-config /absolute/path/to/qdrant-server.toml --activate-memory
```

Mash validates the existing vector geometry, graph settings and payload schema
before updating placement. It preserves publications, canonical history, vectors
and retrieval policy; no reindexing is required. Activation can be rerun after
interruption. Subsequent starts with the selected profile omit the flag. Other
API processes must use the same configured profile when restarted.

Qdrant independently applies conversions through its optimizer, so the configured
policy can be visible before conversion finishes. Its
[memory-tier documentation](https://qdrant.tech/documentation/ops-configuration/memory-tiers/)
describes residency and low-memory behavior. Lower peak RAM and search latency
need workload-specific measurement.

Each group has an isolated message collection with named cosine dense vectors
and a BM25-scored full-text index on `unit.text`, plus a state collection for the group's
views configuration and each node's views certificate. Canonical history,
checkpoints, tombstones and chat catalogs live in shared silver. Lexical retrieval is
a Qdrant `text` query on that index (`word` tokenizer, lowercased; k=1.2, b=0.75),
so document frequencies and the average length come from the collection rather
than a fixed 256-word reference. A `text_any` condition per query word requires
the word or one of its accepted prefix/typo alternatives; metadata filters do
not change the statistics. Deleted or rewritten points stay in the statistics
until Qdrant optimizes their segment. The `tokens` keyword field remains the
prefix/typo vocabulary and literal-verification evidence. Local and shared
retrieval use this same lexical ranking function.

Views schema 13 marks a group whose collection has the text index. Explicit
setup (`mash index`, `mash-server` startup) builds the index from stored
payloads and upgrades a schema-12 group in place, keeping its views version and
certificates; readers fail closed until then. Collections created under schema
12 keep an unused `lexical` sparse vector configuration, and their old sparse
vectors disappear as documents are rewritten or views are rebuilt.
The existing application core still fuses, reranks, and groups message candidates.
All expressive filters apply inside Qdrant before candidate limits. HNSW is
approximate; exact mode stays available without rebuilding the collection.
Quantization is disabled. Foreign vector fields, incompatible vector settings,
or payload index types are rejected without deleting data. Dimension-specific
fields are prepared without changing the original vector space.

Each node's views record names the silver revision it was certified against.
Publication clears the certificate, replaces changed documents in place, deletes
removed chats, and then recertifies; a crash between those steps leaves views
stale rather than wrong-and-current. Activation replaces the group configuration
point; API processes with superseded settings fail closed. The 64 MiB upload
limit is configurable. Keep the silver database on persistent, backed-up storage.
