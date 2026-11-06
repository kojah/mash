# Agent history architecture

Mash searches chats with coding agents across providers and machines.
Its domain is chats, messages, turns, tool activity, and attachments. The
supported provider adapters interpret each agent's retained history without
writing to the agent's files or databases.

## Canonical history

A chat has a provider, stable identity, title, project, repository, and activity
times. A message belongs to a chat and has a stable identity, dialogue role,
canonical content, interpreted searchable text, tool activity, and attachments.
Display order is separate from identity. Explicit branch ancestry refers to
message IDs, so inserting earlier history does not redirect existing branches.

Turns group related dialogue within a chat. They are derived from canonical
messages after writes; they are not an independently published or archived data
source. Provider-specific evidence, including Claude's retained branch
contributions, remains explicit where needed to reconstruct the history.

Silver stores chats, messages, and turns in separate tables. Chat provenance
and message fingerprints live on their canonical rows; ingestion checkpoints and
publication bookkeeping remain separate. Message roles, positions, and content
are required. Deleting a derived turn clears its message references; deleting a
chat removes its messages and turns. A configurable hierarchy of arbitrary
content types is outside the product boundary.

## Search and synchronization

SQLite silver is canonical. Qdrant holds rebuildable lexical and vector search
views, using the same engine and collection schema locally and on the shared
server. Search results identify the provider, chat, message, and publishing
node needed to retrieve the original context.

Local indexing reads provider histories and commits silver before refreshing
views. Shared publication sends that local history over the authenticated
WebSocket protocol. Baseline backfill, incremental publication, retained
checkpoints, deletion intent, and acknowledgement guards preserve recoverability.
Views may be certified only against history already committed to silver.

Offline local replay and server replay rebuild views from silver. Embeddings may
be reused only when the actual prepared text, model, dimensions, and recipe
match. Search availability must not imply that an unfinished backfill is complete.

## Portable history and releases

Exports preserve sensitive canonical agent history and retained attachments,
without credentials, provider files, models, or retrieval indexes. Archive
envelope `format_version = 2` and history `record_schema_version = 2` are separate
contracts. Chat records describe history; archived parent positions are resolved
to stable message IDs within the import transaction before commit. Earlier
record schemas are rejected rather than translated.

Live silver and views schemas are strict per release. There are no in-place
schema migrations. Local history crosses releases through export/import only
when archive schemas are compatible; shared history is republished and views
are rebuilt. Shared synchronization requires protocol 3 and sync catalog schema
1; existing unversioned catalogs are rejected untouched.
