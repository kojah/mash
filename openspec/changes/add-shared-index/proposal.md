# Local and shared history search

Support local SQLite indexing, direct PostgreSQL indexing without a local database,
or both. Each destination has an independent provider allowlist. PostgreSQL has
one joint index per node group. Shared search and view remain
available while source nodes are offline. SSH federation is removed.

Nodes use optional `shared` configuration with `group_id`, `node_id`, and
`database_url_env`. Hosting is deployment-supplied and remains undecided.
Local search stays the default for local/both modes; shared-only defaults to PostgreSQL.

Expose the same operations through a stdio MCP executable using the official
Rust MCP SDK. Preserve the six public CLI commands and two JSONL parsers.

In both mode, shared publication follows local commits. Persistent revisions, atomic snapshots,
acknowledgements, and tombstones support retry and deletion without an HTTP
service or background synchronization. Production database provisioning and
history upload remain deployment steps outside implementation validation.
Use async tokio-postgres for network I/O, with SQLite, parsing, and local inference
awaited on blocking workers. Cranelift is the development compiler backend.
