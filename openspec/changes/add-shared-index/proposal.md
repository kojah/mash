# Shared search index

## Intent

Keep a searchable local index on every machine and publish indexed history to
one shared index. Cross-machine search and message context must remain available
when a source machine is offline. Replace SSH query fan-out and remote CLI
execution with shared storage access.

The shared host is intentionally undecided. No deployment, credentials, or
production upload is authorized by this proposal.

## Proposed behavior

- Local indexing, search, and view remain usable without shared configuration or
  network access.
- Indexing commits locally before publishing changes. Failed uploads remain
  pending and retry on a subsequent indexing run, including a no-change run.
- Upload canonical messages and their searchable projections, installed-model
  embeddings, conversation metadata, and deletion records. Include context-only
  messages for view, but exclude them from search.
- Shared search queries one joint index rather than executing a search on every
  node. Shared view reads stored context rather than contacting the source.
- Forget deletes locally and durably queues shared deletion. Its result must
  distinguish local completion from pending shared deletion.
- Remove SSH destinations, node inventories, remote worker flags, subprocess
  transport, and per-node query outcomes when the replacement is usable.

## Decisions still open

1. Access boundary: a small HTTP service backed by PostgreSQL (recommended), or
   direct PostgreSQL connections from each CLI.
2. Search selection: explicit local/shared scope first, or a merged default.
3. Shared deployment location and credential provisioning.

PostgreSQL is a candidate, not a finalized implementation choice. Hosting should
be configurable and need not be selected before implementing the agreed access
boundary.

## Boundaries

Keep the existing six public commands and the two history parsers. Do not add a
transport registry, automatic model downloads, a background client daemon, or
unrequested deployment. Synchronization happens through explicit indexing and
forget operations. A shared service, if selected, is a separate executable.
