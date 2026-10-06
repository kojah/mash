# Shared storage: SQLite silver, Qdrant gold

As of 2026-10-06 the shared HTTP service keeps two stores with one direction of
dependency:

| Layer | Store | Contents | Recovery |
| --- | --- | --- | --- |
| Shared silver | SQLite, one database per node under `silver_dir` | Node heads and revisions, conversations, messages, source checkpoints, tombstones, scan and coverage reports | Back up the file; nodes can republish snapshots |
| Shared gold | Qdrant (`mash_gold_*` collections) | Group gold configuration, one certificate per node, one search document per message with sparse and dense vectors | Rebuilt from silver: `mash serve --replay-gold`, or node republication |

Silver has no gold tables and gold holds no history that silver lacks. Each
node's silver database uses the same schema as a local index
(`app/storage/silver.sql`); the server leaves local-only ingestion tables empty
and keeps its publication head in `shared_head`. Published records map onto
the typed columns losslessly, so thread fingerprints and silver revisions are
the same whichever store computes them.

## Publication

1. The upload is spooled to a temporary file and checked against the group's
   active gold configuration.
2. It is validated and applied to the node's silver database in one SQLite
   `IMMEDIATE` transaction:
   revision checks, snapshot/delta semantics, tombstones, thread fingerprints,
   coverage and the new silver revision. Any failure rolls back.
3. Gold synchronization clears the node's certificate, replaces changed
   documents in place (point IDs derive from node and message ID), deletes
   removed threads, and finally certifies the node for the new silver revision.

A failure after step 2 leaves gold stale. Stale gold is never reported current
and is excluded from dense search; lexical search and `view` keep working. The
node's next snapshot (sent because its gold is not current) or a server replay
repairs it. A duplicate upload of the same revision changes no silver; it
records the sync time and resynchronizes gold when gold is not current.

## Removed machinery

The previous design stored canonical snapshots in Qdrant and recreated database
transactions on top of it: immutable staged versions, a conditional group
catalogue, head compare-and-swap with read-back verification, ambiguous-write
retention, snapshot work budgets (`max_mutation_snapshot_mib`) and cleanup of
obsolete versions. All of it is gone. `qdrant_max_node_mib` became
`max_upload_mib`, and per-node mutations are serialized by an in-process lock
plus SQLite's writer lock on that node's database.

## Deploying over an existing Qdrant-only server

The new service creates empty silver and new `mash_gold_*` collections; the old
`mash_q_*` collections are untouched and can be deleted after cutover. Every
publishing node must run `mash index` once to republish its snapshot. Remove
`qdrant_max_node_mib` and `max_mutation_snapshot_mib` from the server TOML and
optionally set `silver_dir`.

## Local silver and gold

A node's index is the same split: `mash.sqlite3` is silver and
`mash.gold.sqlite3` (attached as schema `gold`) holds embeddings, semantic
chunks, the FTS index, manifests and readiness. Gold records the silver
database instance it was built for; a missing, unreadable or foreign gold file
is replaced with empty, unready gold by the next writer, and readers treat it
as not ready without creating anything. Deleting messages removes their gold
rows and publication-journal entries through per-connection triggers, since
SQLite foreign keys cannot cross databases. `mash index --rebuild-local-gold
--full` discards all gold and replays it from silver; without `--full` valid
vectors are reused. Upgrading a single-file index copies its gold into the new
file (seconds for a 1.1 GB index, no re-embedding); the freed pages stay in
the silver file until a manual `VACUUM`.
