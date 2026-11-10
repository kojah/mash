# Native conversation records

Implementation contract for the native records restored from the improved
November 2 mash-recall behavior (`a4fa36279`). It defines the record model and
the requirements for storage, public tools, publication, archives, privacy and
plugin recall.

## Purpose and ownership

Records interpret Mash's agent history as decisions, preferences, facts and open
questions. They are not external content connectors or arbitrary objects.
Canonical chat messages remain the supporting evidence. SQLite owns record
state; retrieval never establishes acceptance or updates confirmations.

Each record has an opaque stable ID and an authoritative owner. Ownership is
independent of the physical database identity and survives identity-preserving
restoration. Only that owner's local database mutates a record. Shared reads
aggregate records without choosing a winner by timestamp. Different owners may
hold contradictory records. Supersession and resolution initially require the
same owner and scope.

Scopes are typed: personal (within the owner namespace), repository (explicit
identity, not basename), local project (exact absolute path, not shared as a fleet
identity), or chat (owner plus chat ID). Evidence must belong to the requested
scope. Broadening scope is explicit. Supplied actor labels are attribution,
not authentication or proof of approval.

## Record state

Every revision has a statement, rationale where supplied, effective time where
known, recording time, attribution and quoted evidence. Unknown times stay
unknown. Candidate/established describes evidence review, separately from the
kind's lifecycle:

| Kind | Lifecycle | Additional information |
| --- | --- | --- |
| Decision | proposed, accepted, superseded, withdrawn | alternatives and consequences |
| Preference | active, superseded, withdrawn | conditions and exceptions |
| Fact | current, superseded, retracted | reported-by attribution and optional validity interval |
| Open question | open, resolved, dropped | answer and resolution links |

Only established accepted decisions count as settled choices. Candidate records
never silently become established through reads, repeated extraction or
similarity. Current facts are recorded claims, not independent verification.
An open question may reopen with a new revision. A decision to implement work
does not prove implementation; implementation evidence is recorded separately.

Corrections create revisions of the same record. Changed choices create new
identities with explicit supersession: A to B to A is three records. Corrections
do not inherit confirmations of the previous interpretation. Superseding creates
the successor and retires the predecessor in one transaction; conflicting
successors cannot both win. Similar statements are suggestions, never automatic
merges. Exact duplicates are reported/reused explicitly without manufacturing a
confirmation or resurrecting a retired choice.

## Evidence, privacy and retry

Evidence carries chat/message identity, exact quote and content fingerprint.
Short approvals cite both the proposal and the approval. Matching a quote proves
its presence, not semantic endorsement. User-role messages can contain injected
agent reports; agents must distinguish these and uncertain interpretations remain
candidates. Decisions/preferences presented as established need human support.

A citation names a retained message, or a session plus a quote. A session
citation resolves inside the mutation transaction to the latest retained
message containing the quote; an endorsement matches only visible user dialogue
(`COALESCE(search_projection, content)`). When retained history lacks the
quote, `records::live` reads that session's provider source without writing
silver, because a silver write for one chat would leave local views uncertified
until the next index. It selects the source as indexing does (Claude Code
`<session>.jsonl` under the configured roots; Codex through the thread store's
own metadata), so the cited message ID and content fingerprint are the ones
indexing will retain.

Such evidence counts as current while retained history for its chat ends before
the record was written, judged by the chat's `updated_at` against the
revision's `recorded_at`, and while the chat is not tombstoned. Once history is
retained past that point, the usual rule applies: a missing or changed message
is stale evidence.

Ordinary source replacement preserves records and exposes changed/missing
evidence as stale. Evidence therefore has no cascading foreign key to messages.
Explicitly forgetting any cited chat removes the entire dependent record's text,
all revisions, evidence and confirmations. A content-free tombstone prevents
resurrection. Publication and archives must respect these removals. Private spans
are removed before storage using the current ingestion policy; output and
publication also apply their configured credential redaction.

Each mutation supplies an operation ID and expected revisions. The operation
receipt, request digest, mutation, links and any extraction checkpoint commit
atomically. Reusing an operation ID with a different request fails. A retry with
the same request returns the durable result. Receipts contain no copied prose.
All source checks and revision guards run inside the writer transaction.

Confirmations require distinct supporting source messages, attached to a specific
record revision. Retries, rereading and re-extracting the same evidence cannot
inflate confirmation history or recall rank. Historical extraction tracks source
fingerprints, not timestamps alone, and commits coverage with successful writes.
Old material imported later cannot supersede newer choices through ingestion order.

## Storage and transport

The common local/shared schema contains records, typed revision documents,
evidence, links, confirmations, operation receipts, extraction checkpoints and a
record publication journal. Record-only edits do not advance the chat projection
certificate or require Qdrant embeddings. Shared uploads must explicitly carry
record-only changes, snapshots, and deletion intent under the existing publication
revision guards. References are checked against the final transaction state.

Shared record reads select eligible IDs in SQLite before decoding their history;
incremental uploads select from the record journal. They never visit owner-local
operation receipts, extraction receipts or checkpoints. Canonical fingerprints
also exclude those tables at query time; portable archives retain them. These
paths preserve the same canonical table ordering and row encoding.

Archive import validates and remaps record ownership, citation identities and
links. A normal copy import gets new ownership; identity-preserving restoration
retains ownership. Import cannot claim successful portability while silently
dropping records. Payload sizes and history walks are bounded.

Migration baseline is schema 62; schema 54 and other older layouts remain
unsupported. `rusqlite_migration` 2.6 is compatible with Rusqlite 0.40 and owns
the numbered upgrade sequence. Its first 62 entries reserve existing version
numbers, not legacy upgrade paths. The initializer rejects unsupported versions
before invoking it. Upgrade runs under writer admission and a SQLite transaction;
read-only opens never migrate. Qdrant schema and existing chat certificates stay
unchanged by this additive migration.

## Agent interface and recall

Typed MCP operations call the Rust application core to record, revise, transition,
confirm, search/list and inspect history. Plugin hooks are bounded clients, never
an alternative record store. Session recall selects relevant established records
with a text budget and bounded confirmation/recency ranking. Candidates, conflicts
and stale evidence are labeled separately.

Preserve the improved file recall behavior: at most once per file/session, exact
project-relative file context, bounded results, a fail-open deadline and opt-out.
Preserve non-destructive duplicate suggestions and explicit supersession history.
Historical extraction is explicit, bounded and checkpointed; it does not silently
promote inferred decisions.

## Acceptance evidence

Required tests cover all four kinds and illegal states; scope/owner rejection;
quotes, stale evidence and short approval context; confirmation replay; competing
supersessions; correction versus changed choice; A to B to A; forget across all
revisions; record-only shared publication and snapshots; portable archives;
migration rollback and retained identities; stale runtime rejection; file recall
bounds and private spans. A bounded protocol model covers receipt atomicity,
competing revisions and publication of deletion, with source mappings and explicit
failure boundaries. Passing storage-only tests does not certify the whole feature.
