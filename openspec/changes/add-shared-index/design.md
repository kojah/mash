# Direct PostgreSQL shared index

## Configuration and groups

Strict local configuration contains providers and indexing settings. Optional
shared configuration supplies a group ID, node ID, and connection-string
environment variable name. No hosting location or credentials are hardcoded.
Old SSH inventory configuration and flags are rejected.

A deterministic hash of the group ID selects a PostgreSQL schema. All members
of that group use one joint index; records are owned by node ID within the group.
Namespaces prevent accidental group mixing, not access by privileged database
users. Direct access assumes trusted nodes. TLS verifies public-root certificates
and hostnames by default; plaintext requires explicit sslmode=disable.

## Publication and deletion

SQLite schema 13 stores a random local database instance ID, monotonically
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
changes. This is an intentional first-version throughput limitation.

## Search and context

Default search/view remain local. Search --shared queries only the joint index;
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
same CLI operations in a serialized blocking worker. Tool arguments cannot inject
CLI flags. Read-only and destructive annotations describe tool effects; typed
application errors become MCP error tool results. Only explicit models_install
can download models. No separate HTTP endpoint or application server is added.

## Verification and deployment

Default tests cover local behavior, strict configuration, removed SSH options,
durable publication state, and SDK initialization/discovery/tool calls. A
PostgreSQL integration test uses disposable schemas for multiple nodes/groups,
idempotent retries, exact candidates, stored context, stale revision rejection,
node identity conflicts, and deletion propagation.

Releases and the existing deployment workflow include both executables.
Production PostgreSQL provisioning, credentials, and shared-index configuration
remain deployment choices. No production history is uploaded during validation.
