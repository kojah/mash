# The source model

This document is the design reference for how Mash represents *everything it
ingests*: native agent histories and external connectors. It states both what
exists today and what the target model is; those are marked explicitly, because
several parts of the target are not implemented yet.

The goal is one vocabulary — **source, source key, claim, publisher instance,
capture scope, object role** — so adding a feed does not add a storage branch, a
search path or a special case in retrieval.

## 0. Implementation status

| Element | Current | Target |
|---|---|---|
| Unified object storage (`objects`, roles, schema validation) | **implemented** | — |
| Read-time copy collapse (native and same-source connectors) | **implemented**, window-bounded; merges duplicates for display, not their ranking pressure | — |
| Duplicate detection (`/v1/fingerprints`, status) | **implemented** (native, same conversation id) | add divergence |
| Progressive baseline publication | **implemented** | automatic scheduling beyond recency |
| Native publication transport (`/v1/sync`) | **implemented** | — |
| Connector publication transport | both `/v1/sync` `BeginConnectorBatch` **and** legacy `POST /v1/connector-batches` are mounted | `/v1/sync` only, after the SDK migrates |
| Connector source ownership | **implemented**: durable group-wide claim checked in `ingest_connector`; `POST /v1/source-transfer` | — |
| Principal-scoped connector ids | **implemented for Slack** (`slack:<team>:<principal>`, `principal` required) | `provider:tenant:principal` for the other blueprints |
| Connector omission deletion | **not implemented** (`CompleteSnapshot` records a timestamp only) | capture-scope validated deletion |
| Owner liveness (stale claims) | **not implemented**; a claim blocks the source until a manual transfer | staleness in status, `transfer --force` |
| Restore under the same node id | **not implemented**; a regressed revision is applied | destination rejects a revision below its head |
| Best-effort device dedup skip | **not implemented** | optional |

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
node is a writer, not a source. "Source" is also the name of the wire type
`crate::source::Source::{Provider, Connector}`; see the glossary for the
distinction.

## 2. Two scopes

The single important asymmetry:

| | Device-scoped (native) | Account-scoped (connector) |
|---|---|---|
| Data lives | a filesystem on one machine | an external system behind a credential |
| Identity includes the node | **yes** | no |
| Owner | implicit (that node) | claimed (target) |
| Same source on two machines | two independent sources | a collision |
| Duplicate data from copying | possible, benign | possible, wasteful and divergent |

Device data belongs to a machine because that is where it lives. Two machines
with the same history are not a conflict; they each own their copy. Account data
belongs to a credential because visibility, rate limits and cursors are
properties of that credential, not of a laptop. Running the same credential in
two places produces two pollers over one dataset with two cursors that can
drift.

**Decision D1.** Device-scoped identity includes the node. Account-scoped
identity does not (target; native already behaves this way, connectors do not
yet).

## 3. Ownership

Target: every account-scoped source has exactly one owner node in a group:

```
(group_id, source_key) -> owner_node
```

- The claim is enforced inside `ingest_connector`'s transaction, so it covers
  both transports. `BeginConnectorBatch` is an advisory fast-fail, not the
  enforcement point: the legacy HTTP route never sends it.
- First writer wins; the loser is rejected with `source_owned_elsewhere` before
  any silver writes, and the connector reports it and stops.
- Device-scoped sources need no claim: the node is the owner by construction.
  A **restored or copied** silver database is a new device source, not a move.
- Ownership transfers explicitly (`transfer` command), including the connector
  revision/schema/activity state, so replacing a laptop does not orphan a source
  and does not reset `expected_revision`.

**Owner liveness.** A claim is a correctness record, not a lease. If the
owning node disappears, the source must not stay blocked forever: `status`
reports the claim's last known revision and the last time the owner was seen,
and marks the owner `stale` after a configurable idle period. `transfer
--force` moves the claim without cooperation from the old owner. A stale
claim never expires on its own — two live pollers is exactly the bug D2
exists to prevent, so recovery stays explicit and logged.

**Restore guard.** Restoring a silver backup under the *same* node id is the
one case where D5's absence rule is destructive. The destination still holds
the newer history; the restored node reports a lower revision; its next
replace snapshot deletes the newer rows by absence. The destination
therefore records the epoch of each node's published history and rejects a
publication whose revision is below its head with `revision_regressed`
instead of applying it. Publishing the restored database under a new node id
is the supported restore path; an explicit epoch reset is the escape hatch.

**Decision D2.** One owner per account source; claims are atomic, transactional
and transferable. Never inferred from content or channel overlap.

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

- `parent` / `parent_storage_id` — the container or segment an object sits
  in. An object can have more than one ancestor (a thread inside a channel),
  so the model is a set of edges — D3's `parents` — even though the current
  storage column holds a single parent.
- `reply_to` — a thread reply's parent entry.
- `ordinal` + `context_parent` — native ordering (linear or tree).
- `author` (connector entries) or `role` (native entries) — who spoke.
- `kind` — a namespaced label (`codex:message`, `slack:message`,
  `github:review-comment`) shown to users and usable in filters.

**Decision D3.** Structure variation is expressed as a per-source *schema*, not
as storage branches. Connectors declare `Schema { kinds: [{kind, role, root,
parents}] }`; Mash validates at ingest (declared kinds, role/parent agreement,
acyclic parent graph). Native providers are conceptually a built-in schema with
the same roles; native ingestion does not run that validator today.

## 5. Ingestion runtimes

There are two runtimes behind one model:

| | Native | Connector |
|---|---|---|
| Runs | in-process parser | external program |
| Reads | local files | a remote API |
| Emits | `ConversationRecord`/`MessageRecord` events | `connectors::Batch` JSON |
| Delivered by | the node over `/v1/sync` | the writing node: `/v1/sync` `BeginConnectorBatch`, or the still-mounted `POST /v1/connector-batches` |
| Revision | node publication revision | recorded per **(node, source)** destination today |
| Deletion | replace snapshot; absence = deletion for that node | explicit `DeleteObject` only |

Both end up in the same silver tables (`objects` with `origin` and `source`),
the same view builder, and the same retrieval.

**Decision D4.** Two runtimes, one storage model. No source-specific tables,
columns or query paths.

## 6. Revisions and delivery

- A **node publication revision** orders a node's own history. It is what
  `/v1/sync` commits and what freshness reports.
- A **connector revision** (`expected_revision` / `source_revision`) is stored
  in `connector_sources` inside the receiving node's silver, and `Receipt` says
  "at this ingestion destination". It is therefore scoped to `(node, source)`
  today, not to the account source. Moving a source to another node must
  migrate this state; otherwise the connector's next batch conflicts.

  Read the split as: **identity is global, delivery state is a per-destination
  cache keyed by node.** `expected_revision` is not part of what the source
  *is*; it is what one destination last acknowledged. Ownership transfer moves
  the cache, never the identity.
- Delivery is typed, compressed, resumable and acknowledged:
  - `SilverCommitted` — silver and the receipt are durable.
  - `Published` — views are certified **for native publications**. For
    connectors the frame currently carries `views_ready`, which is false because
    connector receipts do not certify views synchronously.
- Progressive baselines deliver a first history in batches that commit
  independently, with a manifest that proves coverage. Baseline batches never
  delete by absence.

## 7. Deletion

Only explicit evidence deletes:

- A replace snapshot from a node means "this is everything that node has"; chats
  absent from it are deleted **for that node**.
- A connector batch deletes only through explicit `Change::DeleteObject`.
  `Trigger::CompleteSnapshot` records a timestamp, not a deletion set; nothing
  stores the capture scope that would define "complete". Clients must emit
  deletes for objects that vanish from their capture scope.
- A device-scoped baseline batch never deletes by absence.
- `forget` is local to the node that issues it. It does not remove identical
  copies on other nodes; a copied/restored device source keeps resurfacing until
  forgotten there too.

**Decision D5.** Absence means deletion only inside a complete replacement from
the writer that owns the scope. Connector omission deletion is a target, not a
current behaviour.

## 8. Duplication

Duplication happens for two reasons: device histories copied across machines,
and account sources configured twice.

**Read-time collapse (implemented, conditional).** Search keys evidence by
`(source, conversation_id, (role, author_kind), blake3(ws-normalized text))`
(plus the message id for message results) and merges copies into `hit.copies`.
Chat listing dedups by provider/chat id. Collapse requires:

- the same `Source` value — for native that is the provider, shared across
  nodes, so device copies collapse; for connectors it is the `source_id`, so two
  principals over one channel do **not** collapse;
- the same conversation/message ids — restored or re-imported histories with new
  ids do not match;
- the same normalized text — divergent copies stay separate;
- both copies reaching the same bounded retrieval window; dedup runs inside
  fusion/diversification, not over the whole corpus.

Collapse is a **display** guarantee, not a ranking guarantee. Copies are
merged after fusion and diversification have already spent candidate slots,
so a conversation present on three nodes can still push distinct evidence out
of a bounded window before it collapses to one hit. The contract in §9
("copies are references, not extra evidence") therefore holds for what the
user sees, not for what ranking selected. Collapsing before fusion, or
choosing one canonical node per conversation earlier in the pipeline, is what
would make the two agree; it is not implemented.

**Duplicate detection (implemented).** `GET /v1/fingerprints` returns a
`FingerprintReport`: duplicated conversations (same conversation id and
thread fingerprint on more than one node), redundant copies, redundant messages,
estimated duplicated vector bytes, and per-node summaries. It cannot see copies
with different ids and does not report divergence (same id, different
fingerprint) separately.

**Write-time avoidance.**

- Account sources: the target is ownership (D2), not dedup.
- Device sources: **not deduped in silver** (D6). Two machines may legitimately
  hold different histories, and per-node silver must remain exportable. An
  optional best-effort skip could avoid most of the cost, at the price of
  incomplete node silver; it is not implemented.

**Decision D6.** No write-time dedup for device-scoped sources. Read-time
collapse (under the conditions above) is the contract; ownership is the target
contract for account sources; detection is a status feature.

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

Current:

- `mash status --shared` reports per-node freshness (revision, counts,
  readiness) and the most recent baseline coverage for the node
  (`baseline: {id, state, covered, total}`).
- Duplicate history is reported through the fingerprint report.

Target: a source list with owner and revision, replacing the per-node-only view:

```
sources:
  codex:macbook            device   owner=macbook  revision=42   current
  slack:T0123:U0ALICE      account  owner=laptop   revision=981  complete
```

## 11. Open gaps

1. **Connector transport is not cut over.** `POST /v1/connector-batches` is
   still mounted in the release router, and the Python SDK still calls it.
2. ~~**No ownership.**~~ Closed: `ingest_connector` claims `(group, source)`
   in the transfer catalog before any silver write, rejects a second writer
   with `source_owned_elsewhere`, and `POST /v1/source-transfer` moves the
   claim explicitly. Revisions are still per `(node, source)`.
3. ~~**Principal-scoped connector ids are missing.**~~ Closed for Slack: the
   blueprint's source id is `slack:<team>:<principal>` and `principal` is
   required configuration, so two credentials in one workspace are two
   sources. The other blueprints keep their existing ids.
4. **Connector omission deletion is not implemented** and capture scope is not
   stored.
5. **Connector `Published` does not certify views** (`views_ready` is false).
6. **Restore/copy semantics are undefined.** A copied silver DB is a new device
   source; `forget` does not fan out.
7. ~~**Authorship is split.**~~ Closed: every search hit and chat match
   serializes one `author: { id, kind }`; native roles map into the kinds and
   keep the role string as the source-native id, while `role`/`author_kind`
   remain internal ranking inputs and are no longer serialized. `Entry`
   objects now require an author, and all five blueprint suites pass under
   that rule.
8. **Ordering is split.** Native uses `ordinal`; connectors have parent/reply
   edges but no generic position. Add `sequence` on entries/segments.
9. **Overlapping principals.** Two Slack users in the same channel produce two
   sources and two copies that do not collapse.
10. **Terminology overload.** See the glossary; the document now uses distinct
    names, but the code does not.
11. **Collapse runs after ranking.** Duplicates occupy candidate slots before
    they merge (§8), so "references, not extra evidence" holds for display
    only.
12. **No owner liveness.** A dead owner blocks an account source until someone
    transfers it; nothing distinguishes a stale claim from an active one.
13. **A restored database under the same node id can delete newer shared
    history** by publishing a regressed revision (§3, restore guard).

## 12. Phased plan

1. **Principal-scoped connector ids** (Slack first) and a migration note.
   This changes `source_id`, so it invalidates existing `connector_sources`
   rows and read-collapse keys; plan the migration, do not discover it.
2. **Ownership**: a group-level claim table enforced in `ingest_connector`,
   covering both transports; `transfer` migrates revision/schema/activity;
   owner liveness in `status`; the restore guard on published revisions.
3. **Transport cutover**: port `sdk/python` to `BeginConnectorBatch`, then
   remove the HTTP connector route.
4. **Connector views certification** in the `Published` frame, or a separate
   frame that reports receipt readiness.
5. **Author and ordering normalization.** This is what lets connector results
   rank alongside native ones; it outranks a nicer status command.
6. **Status**: unified source list with owner, revision, state and staleness.
7. **Optional device dedup refinements** (divergence), then pre-fusion
   collapse, then best-effort skip.

Each phase ships alone and preserves D1–D7.

## 13. Invariants

1. A node is a writer; a source is a dataset.
2. Device identity includes the node; account identity does not.
3. One owner per account source; claims are atomic and transferable.
4. Structure is declared, never special-cased in storage or retrieval.
5. Absence deletes only in a complete replacement from the owning scope.
6. Read-time collapse holds only for copies with the same source key,
   conversation/message ids, speaker and normalized text that reach the same
   retrieval window.
7. Ranking never branches on origin or kind.
8. A connector revision is scoped to its destination node until ownership and
   transfer migrate it.
9. `Published` certifies views for native publications; connector publication
   reports receipt readiness instead.
10. A claim is atomic and never self-expires; `status` exposes staleness and
    `transfer --force` is the only way to take it, from a live owner or a dead
    one.
11. A destination never accepts a publication whose revision is below the
    node's published head; restore is a new node id or an explicit epoch reset.
12. Collapse merges duplicates for display; it does not remove their ranking
    pressure.

## 14. Glossary

| Term | Meaning | Not to be confused with |
|---|---|---|
| **source** | a dataset: native provider on a machine, or connector export | `crate::source::Source`, the wire enum |
| **source key** | `provider : tenant : principal` | the connector `source_id`, which is one encoding of it |
| **scope** | device vs account (ownership semantics) | search filters; connector capture allowlists |
| **node** | a machine + its silver DB, the writer | a source |
| **owner** | the node allowed to write an account source | `owner` in publication snapshots (DB instance check) |
| **claim** | the `(group, source key) -> owner` record | a lease or a lock |
| **publisher instance** | the local DB instance that publishes | a node identity |
| **capture scope** | what a source deliberately covers (roots, channels) | query filters |
| **dedup key** | `(source, conversation, speaker, text hash)` | the fingerprint report key `(conversation, thread fingerprint)` |
