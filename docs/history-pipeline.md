# Canonical history and retrieval views

Mash's local application core separates successful indexing into two stages:

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

## Remaining boundaries

Some provider adapters still prepare the stored `search_projection` alongside
canonical records. Gold rebuilds retrieval structures from that stored
projection; it cannot yet reinterpret every projection policy solely from raw
canonical text. Moving those policies requires durable provider interpretation
metadata (tracked in `mash-c0d.3.9`).

The ingestion outcome used for readiness is run metadata, not a new persisted
completion certificate. An independently exposed gold operation would need a
durable, versioned completion contract before it could safely publish readiness
after arbitrary restarts.

Shared-only ingestion and independently routed shared publication still use
their existing paths. Applying this boundary to those paths, without requiring
local SQLite, is tracked separately in `mash-c0d.3.10`.
