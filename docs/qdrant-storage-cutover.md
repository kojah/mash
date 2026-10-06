# Qdrant-only storage cutover

The target is one Qdrant persistence implementation for both local and shared
Mash data. The local SQLite backend is being removed, not retained as an option.
This document describes the remaining acceptance work; it does not claim that
the current local runtime has completed the cutover.

## Storage and runtime contract

- Qdrant owns canonical silver, source/checkpoint facts, metadata, tombstones and
  gold projections. CLI and MCP use the same typed core operations.
- Local operations use a locally running Qdrant instance and remain usable
  without internet or access to the shared service. Shared operations continue
  through the authenticated Mash HTTP API, with one index per node group.
- Local namespaces and shared groups have distinct ownership. A local mutation
  cannot overwrite another namespace or silently replace shared authority.
- Mash creates no SQLite database, including temporary staging, caches or
  fallback stores. Read-only parsing of provider-owned SQLite formats remains
  an ingestion adapter, not a Mash persistence backend.
- Search does not ingest, initialize an empty store, build embeddings or migrate
  schema implicitly. Missing/incomplete initial indexing returns a typed error.
- Initial indexing, watch synchronization and explicit refresh preserve their
  progress notifications, readiness certificates and retry/recovery behavior.

## Required behavior before removing the old backend

1. Local canonical mutations preserve provider identities, source ownership,
   deduplication, normalization/enrichment, edits, deletions, checkpoints,
   tombstones, native branch evidence and attachment metadata.
2. Silver becomes durable before projection work. Failed or interrupted gold
   construction cannot claim readiness; retries retain unchanged embeddings and
   cannot overwrite a concurrently published canonical revision.
3. `status`, `list-threads`, `view`, attachment access and `forget` operate on
   Qdrant. Metadata and canonical views do not require embedding models.
4. Local/combined message and thread search preserve provider, node, time,
   project/repository and conversation filters before candidate limits. HNSW is
   the dense default, with explicit exact search available for verification.
5. Export/import use typed, versioned canonical records and verified retained
   attachment files. Exports exclude gold, deployment credentials and models;
   they preserve sensitive canonical history. Restoration on another machine
   must permit explicit gold construction and then search without provider files.
6. Provider ingestion and watching work against Qdrant for every supported
   adapter. SQLite-owned schema, queries, FTS/vector tables, sqlc codegen,
   migrations, build hooks and backend-specific tests are removed only after
   corresponding behavior has native persistence coverage.
7. Existing node data is exported and verified before deployment retires the old
   backend. The current export contains a SQLite silver database, so a new
   canonical archive writer and verified conversion are part of the cutover.
   Existing databases are not automatically deleted.

## Evidence required for completion

- Native local tests cover initial indexing, changed/deleted sources, import,
  export/restore, metadata, attachments, both search units and expressive filters.
- Interrupted publication and process/database restarts retain acknowledged
  canonical data and reject incomplete gold or stale writer acknowledgements.
- Tests assert that selecting/using local Qdrant never creates a Mash SQLite
  database and that missing local indexing fails without initializing data.
- CLI and official-SDK MCP tests demonstrate equivalent results and streamed
  index progress through the common core.
- Fresh deployment checks verify local offline access, shared authentication,
  publication identities and source-specific freshness on reachable nodes.
- Representative first/subsequent local and combined searches are measured with
  default ranking, varied queries and filters. Native telemetry verifies actual
  HNSW execution. Candidate/final-result comparisons document approximation
  changes; lower timings alone do not establish acceptable search behavior.

## First completed step

History counts, canonical message views and ranked hits now live in the
storage-independent `records` module. Qdrant transport/persistence and HTTP
handlers use those types directly. The old local SQLite runtime remains until
the canonical ingestion and application operations above have been ported.
