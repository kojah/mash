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
   backend. Version 2 exports contain typed canonical JSON Lines, including
   source/branch evidence; their writer currently reads the SQLite runtime.
   Import must be moved from its current SQLite destination to Qdrant.
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

## Canonical Qdrant persistence component

`shared/qdrant_index/canonical` stores validated canonical JSON Lines in immutable
Qdrant chunks and uses a conditional head update as the visibility boundary.
Its reader never initializes a missing namespace. Staging is batched by encoded
size, includes checksums, and handles UTF-8 boundaries. Captured revisions remain
readable across newer publications. Unchanged data can reuse the active revision;
concurrent writers cannot overwrite the winning pointer. The head records its
publication preparation time and contains no gold readiness claim.

The portable validator now checks identities, ownership, message/source
references, ordinals, scan/checkpoint interpretation, completeness and retained
file counts without a SQL database. CLI/MCP import uses that same validation.

This component is not yet the local CLI/MCP storage path. The existing SQLite
ingestion, projection and query code still needs replacing. Ordinary ingestion
also needs source/thread delta publication and bounded reclamation; the initial
full-archive transfer API alone is not the completed watch/index pipeline.

An isolated disk-backed Qdrant 1.19.1 round trip transferred the Xenia archive:
1,026,902,224 bytes, 817 threads and 345,451 canonical messages, in 3,918 chunks.
Publishing took 61.874 seconds and reading back took 28.814 seconds. The complete
test took 129.56 seconds including file copying, independent validation and
cleanup. Recovered bytes, relationships, completeness/revision and retained-file
counts matched. This is canonical transfer, not search latency or ingestion with
embeddings. Private results are in `canonical-qdrant-corpus-4.log` under the
2026-10-06 performance artifacts directory.

The real corpus caught two setup/transport defects: collection creation could
outlive the client's interactive read deadline, and fixed byte cuts could split
a UTF-8 character. Schema writes now use the awaited bulk-write deadline; reads
retain their bounded deadline. Canonical staging uses the existing UTF-8-safe
chunk reader, with a four-byte boundary regression case.
