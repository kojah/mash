# Canonical history and retrieval views

See [silver enrichment and gold configuration](silver-gold-configuration.md)
for the current enrichment audit, implemented query defaults and remaining
build-configuration contracts.

## Layer ownership

| Area | Layer | Responsibility |
| --- | --- | --- |
| Provider-owned session files/databases | Bronze | Original source records |
| Native adapters | Silver | Interpret formats, branches, tool records and visible dialogue |
| Canonical threads/messages | Silver | Stable identities, text, roles, timestamps, relationships and project metadata |
| Reconciliation | Silver | Deduplication, provenance, edits/deletions, checkpoints and ingestion completeness |
| Search views | Gold | Query-oriented projections of interpreted silver dialogue |
| Retrieval | Gold | FTS, embeddings, vector indexes, ranking and retrieval caches |
| Thread search | Gold | Searchable thread views and ranked excerpts |
| Topics | Gold | Derived memberships, labels and topic-filtering views |

The intended invariant is that gold can be rebuilt from silver without reading
bronze. Silver retains the native interpretation required to distinguish visible
dialogue from context-only records; gold decides how that interpreted history is
represented for retrieval. Changing an embedding model or topic algorithm must
not rewrite canonical history.

CLI/MCP adapters, HTTP transport, configuration and database connections support
the pipeline. They are not additional data layers. A storage module can hold both
silver records and gold tables while exposing distinct responsibilities.

## Current local stages

Mash's local application core separates successful indexing into two stages:

Canonical models, provider adapters, discovery and reconciliation live in
`app/silver.rs` and `app/silver/`; callers import `crate::silver`. The application
orchestrator in `app/core/indexing.rs` invokes `app/silver/local.rs` and
`app/gold/local.rs`. Those builders do not depend on the application core.
Visible dialogue interpretation lives in `app/silver/dialogue.rs`, and canonical
messages expose it through `NormalizedMessage::dialogue()`. A message retains
its complete `content` for context/viewing and an optional `dialogue_text`
override for native visible prose. An empty override is context-only; no override
means its canonical text is dialogue. Native hidden turns, injected instructions,
reasoning blocks and tool records are interpreted by silver, not by a query's
ranking or embedding policy.

Gold model execution and retrieval live in `app/semantic/`; the `app/search_*`
modules own gold query models, ranking and results, and `app/thread_catalog.rs`
owns metadata discovery views over silver. Experimental topics are also gold.
The module groups in `app/lib.rs` identify silver, gold and supporting
infrastructure. `app/shared/` owns HTTP transport, publication coordination and
backend adapters supporting both layers.

1. **Silver ingestion** discovers provider history, interprets native records,
   reconciles identities and deletions, and commits canonical conversations,
   messages, provenance and checkpoints. It also commits invalidation of stale
   retrieval state. It does not load models or refresh FTS/vector caches.
2. **Gold building** refreshes lexical search, semantic caches and embeddings
   from stored history, then publishes readiness for the configured model
   generation. It receives storage, model configuration and the ingestion
   outcome; it does not discover or read provider files.

The existing SQLite schema stores both layers. This separation adds no second
copy of message bodies and requires no schema migration. Provider-owned source
files remain the raw (bronze) source; Mash does not archive another raw copy.

Silver commits canonical changes and invalidation together before gold begins.
Readers can continue to see the previous complete snapshot before that commit;
after it, an incomplete index fails the existing readiness checks. A failed gold
build does not roll back committed canonical history. Context-only records remain
available through viewing without becoming search results.

During an uninterrupted run, gold uses the writer's temporary delta queues and
retains unchanged embeddings. If a writer exits after silver commits, its delta
queues disappear. The durable dirty marker then requires a full derived-state
rebuild from stored history. This recovery is more expensive than an incremental
refresh, but does not require provider files.

`mash index` still orchestrates both stages, including model-configuration
preflight before opening the database. CLI and MCP use this same core. There is
no new standalone gold command, and search never implicitly runs either stage.
Stage spans are `silver_ingest` and `gold_build`; existing progress events and
response fields remain available.

## Interpretation and publication boundaries

The SQLite column historically named `search_projection` stores silver's
interpreted `dialogue_text`. Its name remains unchanged to preserve the existing
schema and stored data. Gold's FTS, embeddings and vector caches are separate
derived structures. The shared wire field `projection` carries the same visible
dialogue alongside canonical content. These physical names do not make native
visibility a retrieval policy.

Gold rebuilds from canonical records plus their stored interpretation. It does
not infer hidden/native visibility from arbitrary raw text. Changing native
interpretation rules requires silver replay; changing retrieval or embedding
policies must use stored silver without provider replay. Query-oriented policy
belongs in gold; provider-native visibility is retained in silver rather than
reconstructed by gold from raw serialized tool bodies.

Local gold now consults the durable, versioned silver certificate in SQLite,
including pending provider scans and rejected sources across runs. Index/status
responses expose this evidence. See `silver-gold-configuration.md` for its scope,
replay and migration semantics. An independently exposed gold operation still
needs its build manifest/configuration contract and shared propagation before it
can safely replace projections across arbitrary restarts.

Local SQLite is required. Configuration supports local-only or local plus shared
indexing, with per-index provider allowlists. Every provider forwarded to the
shared index must also be included locally. Shared publication is optional;
local search remains available while disconnected from the shared service.

Routed shared publication in `app/shared/routed.rs` discovers and reconciles
sources on the node, calling silver adapters and canonical provenance storage.
Its gold
materializer lives in `app/gold/shared.rs` and receives interpreted silver
conversations plus model configuration, without provider roots or checkpoints.
It emits bounded message batches with embeddings into the private upload spool.
The shared server persists canonical records and vectors together and maintains
the backend's retrieval indexes. Publication coordination supports both layers;
it is not another data layer and does not connect directly to shared databases.

## Storage boundary

Both layers may share a database/service, but silver must be independently
readable and gold independently replaceable from silver plus model configuration.
Atomic publication must prevent readers from combining different revisions.

| Backend | Silver storage | Gold storage |
| --- | --- | --- |
| SQLite | `conversations`, `messages`, provenance and checkpoints | `message_embeddings`, `message_fts`, `semantic_chunks` and retrieval caches |
| Postgres | `conversations`, `messages`, `direct_sources`, tombstones and node metadata | `message_search`: vectors, model generation and full-text documents |
| Redis | Immutable snapshot `silver` section, state and catalog keys | Snapshot `gold.embeddings` section and search documents |
| Qdrant | Snapshot `silver` section in state chunks, node heads and catalog records | Snapshot `gold.embeddings` section and searchable message vectors |

The current Postgres definitions in `app/shared/schema.sql` explicitly group
silver and gold tables. Historical bootstrap/upgrades live in
`app/shared/migrations.sql`. Version 16 moves existing generation, vector and
full-text values into `message_search` within the startup transaction, then
removes those columns from canonical messages. Publications write both tables
in the same transaction. Readiness checks use a left join so missing gold fails
search, while viewing continues to use silver alone. Deleting silver cascades
to its gold rows; deleting gold leaves canonical history intact. This adds no
second canonical body copy.

Redis and Qdrant use the typed persisted revision schema in
`app/shared/snapshot/layers.rs`. Its `silver` section contains canonical threads,
messages, source checkpoints and tombstones; canonical messages have no vector
or model-generation fields. The separate `gold.embeddings` map owns those
derived values by message ID. Both sections share one checksummed, chunked
immutable payload and atomic head. This is a section-level schema split within
one published revision, not separate silver/gold keys or collections.

Publication heads name their snapshot schema. Heads from the historical mixed
format are decoded through its typed schema and rewritten into the layered
form on the next successful publication, including an idempotent retry. The
in-memory joined planning model is not serializable in production, preventing
new writes of that old representation. Serialization borrows canonical rows and
vectors rather than cloning a complete node. Retrieval documents are
denormalized gold views: they may copy canonical excerpts/metadata together with
derived search fields, while silver remains the canonical source. The Redis
and Qdrant storage adapters identify these views explicitly in their module
documentation. Their copies are not another silver store.

## Verified boundary contracts

Local gold tests remove provider files before building, check canonical rows
remain unchanged, verify model failure preserves durable silver, and cover
incremental reuse plus recovery after losing temporary queues. Rejected source
records prevent readiness even when stored vectors are complete.

The Postgres storage lifecycle test preserves silver and gold through historical
migration and repeated startup, removes/replaces gold without deleting silver,
and verifies readiness and cascading deletion. Shared snapshot codec and real
Redis/Qdrant migration tests preserve message/provenance/vector data and revision
ownership while rewriting the historical mixed format. Backend contracts cover
filters, all supported providers, branches, authorization and failed publication.
