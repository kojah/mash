# Direct PostgreSQL shared index

## Configuration and groups

Strict local configuration contains providers and indexing settings. Optional
shared configuration supplies a group ID, node ID, and connection-string
environment variable name. No hosting location or credentials are hardcoded.
Old SSH inventory configuration and flags are rejected.

`index.destination` selects local, shared, or both. Defaults preserve local-only
without shared settings and both with them. `index.local.providers` and
`index.shared.providers` independently select enabled providers; omitted filters
include all, and empty lists include none. Command provider flags intersect them.
Filters govern future ingestion, not retroactive deletion of previously indexed history.

A deterministic hash of the group ID selects a PostgreSQL schema. All members
of that group use one joint index; records are owned by node ID within the group.
Namespaces prevent accidental group mixing, not access by privileged database
users. Direct access assumes trusted nodes. TLS verifies public-root certificates
and hostnames by default; plaintext requires explicit sslmode=disable.

## Publication and deletion

SQLite schema 14 stores a random local database instance ID, monotonically
increasing revision, acknowledged revision, and destination fingerprint.
Triggers record canonical message/conversation, embedding, and tombstone changes
in their existing transactions, including checkpointed indexing changes.

After local indexing commits, publication opens a consistent SQLite read snapshot
and locks the PostgreSQL publisher row. A different database instance cannot
reuse an existing node ID. Newer accepted revisions reject stale snapshots;
equal revisions acknowledge retries without rewriting records.

Changed snapshots upload bounded batches inside one PostgreSQL transaction.
Only the publishing node's records are replaced. Cumulative tombstones filter
restored conversations. Edits, removed messages, and purged sources therefore
reconcile without a separate outbox protocol. The acknowledgement is recorded
only after PostgreSQL commits and only while the local revision still matches.

Failures preserve local success and return pending publication. A no-change
index retries pending snapshots. Forget also attempts publication after its local
deletion. Copies owned by other nodes remain. Status reads local acknowledgement
state without a network request; it does not promise current server health.

Full replacement transfers the complete node history whenever its revision
changes in unfiltered both mode. This is an intentional throughput limitation.

Filtered both mode ingests the local allowlist into SQLite and the shared allowlist
directly into PostgreSQL, using the same parsers. PostgreSQL keeps hashed source
paths and modification/size checkpoints for incremental reconciliation. SQLite's
instance/revision binds these remote writes to the same publisher identity and
queues offline shared forget intents. Intent delivery precedes shared ingestion
in one transaction. Retries require the original shared source files when those
files were never retained in SQLite. A forget acknowledgement clears delivered
intents but leaves any unindexed revision pending until indexing catches up.

Shared-only uses the PostgreSQL ingestion path with all checkpoints, revisions,
and tombstones in PostgreSQL. No SQLite connection or retry database is created.
Shared-only failures return typed errors; a failed ingest rolls back its changes.
Its status is a network operation, its database_path is null, and its default
view addresses its own publishing node. Switching from SQLite-backed identity
requires a new node ID. No models are downloaded implicitly.

## Search and context

Default search/view remain local in local/both modes. Search --shared queries only the joint index;
view --shared NODE_ID retrieves a shared owner's message context. Source machines
need not be reachable. Cross-owner copies remain distinct and hits include owner
identity. There is no implicit merge or local fallback.

PostgreSQL full-text search and exact cosine over stored signed quantized vectors
produce bounded candidates. Local FastEmbed query embeddings, RRF, and bounded
reranking provide hybrid retrieval. Both CPU and Core ML use the same model-space
identifier, independent of local chunk layout and batching. Missing or outdated
vectors require publishing nodes to reindex. Tool output remains context-only.

## MCP

mash-mcp uses the official rmcp SDK's stdio transport, initialization, tool routing,
and schemas. Search, view, status, index, forget, and models_install invoke the
same async CLI operations. PostgreSQL uses native async tokio-postgres calls;
SQLite snapshots stream bounded batches from a blocking worker to the async
publisher. Parsing and local model inference also run on blocking workers and
are awaited. Mutations serialize per MCP session, model-heavy work has a single
permit, and status/view can proceed concurrently. This async boundary can later
await remote inference without running local CPU work on the executor.
Tool arguments cannot inject
CLI flags. Read-only and destructive annotations describe tool effects; typed
application errors become MCP error tool results. Only explicit models_install
can download models. No separate HTTP endpoint or application server is added.

## Verification and deployment

Default tests cover local behavior, strict configuration, removed SSH options,
durable publication state, and SDK initialization/discovery/tool calls. A
PostgreSQL integration tests use disposable schemas for multiple nodes/groups,
idempotent retries, exact candidates, stored context, stale revision rejection,
node identity conflicts, deletion propagation, provider routing, shared-only
checkpointing without SQLite, malformed-source rollback, and offline forget retries.
Development builds and tests use the Cranelift aliases.

Releases and the existing deployment workflow include both executables.
Production PostgreSQL provisioning, credentials, and shared-index configuration
remain deployment choices. No production history is uploaded during validation.
