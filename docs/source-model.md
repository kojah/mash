# The source model

This document is the design reference for how Mash represents *everything it
ingests*: native agent histories and external connectors. It records the model
that exists today, the decisions behind it, and the gaps that are still open.

The goal is one vocabulary — **source, scope, owner, object role, revision** —
so adding a feed does not add a storage branch, a search path or a special case
in retrieval.

## 1. What a source is

A **source** is one dataset with a stable identity. Everything that produces
history is a source:

- a native provider on a machine (`codex`, `claude-code`, …);
- a connector export (`slack:T0123:U0ALICE`, `github:12345`, `google:me@…`).

A source identity has three parts:

```
provider : tenant : principal
```

| Part | Native example | Connector example | Determines |
|---|---|---|---|
| provider | `codex` | `slack` | which parser/schema applies |
| tenant | — (the machine) | workspace `T0123` | which organization's data exists |
| principal | the local OS user | token identity `U0ALICE` | **what data is visible** |

A **node** is a machine plus its silver database, publishing into a group. A
node is a writer, not a source.

## 2. Two scopes

The single important asymmetry:

| | Device-scoped (native) | Account-scoped (connector) |
|---|---|---|
| Data lives | a filesystem on one machine | an external system behind a credential |
| Identity includes the node | **yes** | no |
| Owner | implicit (that node) | claimed |
| Same source on two machines | two independent sources | a collision |
| Duplicate data from copying | possible, benign | possible, wasteful and divergent |

Device data belongs to a machine because that is where it lives. Two machines
with the same history are not a conflict; they simply each own their copy.
Account data belongs to a credential because visibility, rate limits and
cursors are properties of that credential, not of a laptop. Running the same
credential in two places produces two pollers over one dataset with two
cursors that can drift.

**Decision D1.** Device-scoped identity includes the node. Account-scoped
identity does not.

## 3. Ownership

Every account-scoped source has exactly one owner node in a group:

```
(group_id, source_id) -> owner_node
```

- Claimed atomically at the first `BeginConnectorBatch` for that source.
  `first writer wins`; the loser is rejected with `source_owned_elsewhere`
  before any bytes or silver writes, and the connector reports it and stops.
- Device-scoped sources need no claim: the node is the owner by construction.
- Ownership transfers explicitly (`transfer` command) so replacing a laptop
  does not orphan a source.

**Decision D2.** One owner per account source; claims are first-writer-wins and
transferable. Never inferred from content or channel overlap.

## 4. The object model

Every ingested record is an object with one of four roles
(`app/connectors.rs::Role`):

| Role | Meaning | Examples |
|---|---|---|
| `container` | groups results and owns context | chat, channel, thread, pull request |
| `segment` | structural part of a container; provides ancestry, does not own results | agent turn, review |
| `entry` | searchable text with an author | message, comment, review comment |
| `document` | standalone searchable text; never a parent | note, doc |

Edges and ordering:

- `parent` / `parent_storage_id` — the container or segment an object sits in.
- `reply_to` — a thread reply's parent entry.
- `ordinal` + `context_parent` — native ordering (linear or tree).
- `author` (connector entries) or `role` (native entries) — who spoke.
- `kind` — a namespaced label (`codex:message`, `slack:message`,
  `github:review-comment`) shown to users and usable in filters.

**Decision D3.** Structure variation is expressed as a per-source *schema*, not
as storage branches. Connectors declare `Schema { kinds: [{kind, role, root,
parents}] }`; Mash validates at ingest (declared kinds, role/parent agreement,
acyclic parent graph). Native providers are a built-in schema with the same
roles.

## 5. Ingestion runtimes

There are two runtimes behind one model:

| | Native | Connector |
|---|---|---|
| Runs | in-process parser | external program |
| Reads | local files | a remote API |
| Emits | `ConversationRecord`/`MessageRecord` events | `connectors::Batch` JSON |
| Delivered by | the node over `/v1/sync` | the owning node over `/v1/sync` (`BeginConnectorBatch`) |
| Revision | node publication revision | source revision (`expected_revision`) |
| Deletion | replace snapshot; absence = deletion for that node | explicit tombstones / complete snapshot |

Both end up in the same silver tables (`objects` with `origin` and `source`),
the same view builder, and the same retrieval.

**Decision D4.** Two runtimes, one storage model. No source-specific tables,
columns or query paths.

## 6. Revisions and delivery

- A **node publication revision** orders a node's own history. It is what
  `/v1/sync` commits and what freshness reports.
- A **source revision** orders a connector's dataset (`expected_revision`,
  `source_revision` in receipts). It is independent of any node.
- Delivery is typed, compressed, resumable and acknowledged:
  `SilverCommitted` means silver and the receipt are durable; `Published` means
  views are certified.
- Progressive baselines deliver a first history in batches that commit
  independently, with a manifest that proves coverage.

## 7. Deletion

Only explicit evidence deletes:

- A replace snapshot from a node means "this is everything that node has"; chats
  absent from it are deleted **for that node**.
- A connector batch deletes through explicit tombstones or a complete snapshot.
  Absence from a partial or polling window never deletes: deleted Slack
  messages stay until a tombstone arrives.
- A device-scoped baseline batch never deletes by absence.

**Decision D5.** Absence means deletion only inside a complete replacement from
the writer that owns the scope.

## 8. Duplication

Duplication happens for two reasons: device histories copied across machines,
and account sources configured twice.

**Read-time collapse (implemented).** Search keys evidence by
`(source, conversation_id, (role, author_kind), blake3(normalized text))` and
merges copies into `hit.copies`; chat listing dedups by provider/chat id. Users
see one result. Documented in `app/search_results.rs::deduplicate`.

Consequences worth stating:

- Device copies collapse because `source` is the provider, shared across nodes.
- Connector copies collapse only within one `source_id`; two principals reading
  the same channel are two sources and show twice.

**Write-time avoidance.**

- Account sources: prevented by ownership (D2), not by dedup.
- Device sources: **not deduped in silver** (D6). Two machines may legitimately
  hold different histories, and per-node silver must remain exportable. An
  optional best-effort skip (comparing per-conversation fingerprints across
  nodes) can avoid most of the cost, at the price of incomplete node silver.
- Detection is cheap and useful: report duplicated and divergent conversation
  counts per group.

**Decision D6.** No write-time dedup for device-scoped sources. Read-time
collapse is the contract; ownership is the contract for account sources;
detection is a status feature.

## 9. Retrieval contract

Retrieval operates on roles and ancestry, never on `kind` labels:

- chat-level results group under a container; a segment contributes ancestry;
- message-level results are entries with an author;
- documents are standalone;
- filters (`provider`, `kind`, `role`, `author`, time, project, repository)
  apply uniformly;
- copies from other nodes are references, not extra evidence.

**Decision D7.** Ranking and filtering must not branch on origin or kind
strings beyond the user-visible filter values.

## 10. Status

`mash status --shared` reports per-node freshness. The unified surface adds a
source list:

```
sources:
  codex:macbook            device   owner=macbook  revision=42   current
  slack:T0123:U0ALICE      account  owner=laptop   revision=981  complete
```

and, for device duplication, duplicated/divergent conversation counts.

## 11. Open gaps

1. **Authorship is split.** Native entries use `role`; connectors use `author`.
   Normalize on `Author { id, kind }` and map native roles into it.
2. **Ordering is split.** Native uses `ordinal`; connectors have parent/reply
   edges but no generic position. Add `sequence` on entries/segments.
3. **Overlapping principals.** Two Slack users in the same channel produce two
   sources and two copies that do not collapse. Either accept (per-principal
   corpora) or extend the dedup key for connector records.
4. **Source catalog** does not exist yet; ownership, revision and capture scope
   are not visible in one place.
5. **Native schema declaration.** Native providers do not declare a schema
   explicitly; doing so would let them share the connector validator.
6. **Principal-scoped Slack ids.** The blueprint currently uses
   `slack:<team>`; it should use `slack:<team>:<principal>`.

## 12. Phased plan

1. `SourceIdentity` type used by both paths (no behaviour change).
2. Source catalog + ownership claims at `BeginConnectorBatch`; `transfer`.
3. Status: sources, ownership, revisions.
4. Principal-scoped connector ids (Slack first).
5. Author and ordering normalization.
6. Optional device dedup detection, then best-effort skip.

Each phase ships alone and preserves D1–D7.

## 13. Invariants

1. A node is a writer; a source is a dataset.
2. Device identity includes the node; account identity does not.
3. One owner per account source; claims are atomic and transferable.
4. Structure is declared, never special-cased in storage or retrieval.
5. Absence deletes only in a complete replacement from the owning scope.
6. Read-time collapse is guaranteed; write-time dedup is not.
7. Ranking never branches on origin or kind.
