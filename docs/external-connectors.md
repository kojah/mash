# External connector preparation

Status: experimental contract, not an available ingestion feature. External
records cannot yet be stored or searched. The active implementation task is
`mash-e6g`; the Python client and live ingestion endpoints are still pending.

Connectors execute outside Mash. They own authentication, source enumeration,
translation and upstream checkpoints. Mash owns validation, canonical silver,
retrieval artifacts, search, redaction and optional shared publication. No
third-party executable is loaded by Mash.

## Public boundary

`mash::connectors::Batch` is the draft strongly typed contract. Its JSON Schema
is derived from the Rust types through Schemars; Python models should be generated
from that schema, not from SQLite or our provider-specific silver archive.
Generate each schema with `cargo run-clif --example connector-schema -- batch`
or `-- receipt`. On Xenia, use `agent-test` for the build/run workload and write
generated artifacts under `/home/james/scratch/mash-artifacts`.
`Receipt` describes durable silver acceptance independently of gold readiness.
Its `source_revision` becomes the next batch's `expected_revision` at that
destination; `silver_revision` identifies the database-wide canonical revision.
Changes from other sources must not invalidate a connector's ordering token.
The version-1 shape is experimental until local ingestion and conformance tests
exercise it end to end. Do not advertise a stable SDK yet.

```json
{
  "schema_version": 1,
  "source_id": "my-work-chat",
  "batch_id": "batch-104",
  "expected_revision": 12,
  "changes": [
    {"op": "upsert-chat-context", "id": "context-456", "title": "Storage decisions", "metadata": {"native_kind": "discussion"}},
    {
      "op": "upsert-chat-message",
      "id": "message-789",
      "chat_context_id": "context-456",
      "author": {"id": "user-42", "name": "Alice", "kind": "human"},
      "content": "We decided to use Qdrant.",
      "created_at": 1791540000000,
      "url": "https://chat.example/messages/789"
    },
    {"op": "delete-chat-message", "id": "message-123"}
  ]
}
```

IDs are opaque ASCII identifiers of 1–256 characters. The source ID identifies
an account/dataset, not a machine: copies on different machines must use the
same identity. ChatMessage and ChatContext IDs are separate namespaces within a
source. Canonical storage keys must incorporate that source identity. Names
and message text may contain Unicode. Timestamps are Unix milliseconds; unknown
timestamps stay unknown. Native reply relationships use stable message IDs,
not internal SQLite ordinals. Missing parents may exist outside the current
batch; storage must resolve or reject them explicitly.

`ChatContext` is composed of `ChatMessage` records. It has no author or separate
body. A file's text can be one message without a fabricated user/assistant role. Context
boundaries belong to the connector; source-native channels, discussions or agent
sessions can map to chat contexts while native replies remain message relationships.
Containers preserve source organization (workspace/server/channel/repository).

Chat contexts are not limited to two participants or alternating agent/user
turns. Each message may carry its own stable author identity; several humans
and bots can speak in any order and reply along different branches. Bot authors
are not automatically LLM assistants. Participant membership can be derived
from retained authorship, without inventing a user/assistant designation or
requiring a fixed participant list on every chat context. Native agent roles
remain native metadata; external messages require source-appropriate visibility
and retrieval rules. Preserve the native grouping supplied by the connector.

Upserts contain complete record replacements; omitted optional fields become
unknown. Omitted *records* remain untouched. Deletions are explicit. A batch
cannot mutate an identity twice or upsert messages in a chat context it deletes.
The initial budgets are 512 changes, 8 MiB encoded JSON and 1 MiB UTF-8 content
per message. Titles, author names and HTTP(S) source URLs are bounded to 4096 bytes.
Messages may include up to 32 typed attachment references (ID, name, media type,
HTTP(S) URL, optional byte count). Ingestion must not fetch these URLs. Explicit
binary retention remains a separate operation and is not implemented by this contract.
The Rust decoder also rejects URL credentials. Structural schemas do not replace
semantic, byte-budget, ownership or stored-reference validation.

Attachments belong to `ChatMessage`, never to `ChatContext`. A context's attachment
list is derived from its messages. The contract stores references only; a missing
retained binary must not erase the original source reference. Fetching and retaining
attachment bytes requires a separate explicit operation.

Replies stay within a chat context. The decoder rejects cycles and cross-context
reply links visible in a batch, plus links to parents deleted in that batch.
Ingestion must also validate the final stored graph: a batch-local check alone
cannot catch a cycle spanning previously accepted batches.

Chat contexts and messages may carry arbitrary JSON-object metadata, bounded to
256 KiB encoded per object and covered by the total batch budget. Metadata is
opaque source data: it is not executed, fetched, embedded or automatically made
filterable. Known searchable fields remain typed. The contract also permits
bounded metadata on source containers.

## Required ingestion semantics

These are implementation requirements, not behavior supplied by the DTO module:

- Check a previously accepted `(source_id, batch_id)` before comparing source
  revisions. Retrying the same payload returns its existing silver receipt;
  reusing a batch ID with different contents is a conflict.
- Compare `expected_revision` against the durable source revision to reject
  stale or concurrent mutations. Persist silver changes, the new source revision
  and retry receipt atomically. Never acknowledge uncommitted data.
- Once silver commits, preserve a receipt even if gold maintenance fails.
  Report gold readiness separately and retain existing durable gold repair work.
  Keep publication synchronous; do not introduce a broker or shared outbox.
- The connector advances its upstream checkpoint only after a durable receipt.
  Lost responses are resolved by retrying the same batch, not guessing success.
- Validate author/chat-context/source metadata without interpreting human chat as
  unknown agent roles or applying agent-specific exclusion rules indiscriminately.
- Keep ingestion local by default. Optional publication sends the local data
  through Mash's existing authenticated shared path. A future direct shared
  endpoint must enforce source ownership and publication policy.

## Remaining implementation and acceptance gates

1. Separate closed native parser enums from extensible canonical source identities
   throughout silver, shared publication, query filters and provenance. Store
   human authorship and container/reply metadata without provider-specific code.
   Follow the existing release export/import policy; add no schema upgrade code.
2. Implement local batches in the application core. Do not reuse snapshot
   reconciliation that deletes omitted messages. File ingestion and live requests
   must call the same operation. Gold remains reconstructible from silver.
3. Rename Mash's aggregate thread concept to ChatContext across core/CLI/MCP.
   Rename canonical messages to ChatMessage. Add source/author/container filters
   and source links to search and chat-context
   results. Existing native-provider histories and search behavior must continue
   to pass their contracts.
4. Publish generated schema artifacts and Python record models with drift checks;
   add the minimal local/shared client and a service-independent example connector.
   The SDK does not access SQLite internals or execute source-specific code in Mash.
5. Add live local/shared ingestion using the same batch contract, authentication,
   request limits and acknowledgements. No automatic index creation through search.
6. Verify initial backfill, edits, deletions, omitted-record retention, duplicate
   retries, conflicting batch reuse, stale revisions, cross-source ID collisions,
   missing reply parents, interrupted acknowledgement, crash/restart recovery,
   source ownership, local/shared publication and gold replay. Exercise actual
   Python-generated batches against the Rust ingestion path, not just two serializers.

The current tests cover contract decoding, field and batch budgets, discriminators,
unambiguous mutations and content-free errors. They do not prove durable acceptance,
transport behavior, searchability or Python/Rust parity; those gates remain open.
