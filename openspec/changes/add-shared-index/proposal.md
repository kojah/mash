# Local and shared history search

Keep a local SQLite search index on each node. Publish indexed history directly
to PostgreSQL, with one joint index per node group. Shared search and view remain
available while source nodes are offline. SSH federation is removed.

Nodes use optional `shared` configuration with `group_id`, `node_id`, and
`database_url_env`. Hosting is deployment-supplied and remains undecided.
Local search stays the default; shared search and view are explicit.

Expose the same operations through a stdio MCP executable using the official
Rust MCP SDK. Preserve the six public CLI commands and two JSONL parsers.

Publication follows local commits. Persistent revisions, atomic snapshots,
acknowledgements, and tombstones support retry and deletion without an HTTP
service or background synchronization. Production database provisioning and
history upload remain deployment steps outside implementation validation.
