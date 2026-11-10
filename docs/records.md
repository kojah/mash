# Record what you worked out

Native records preserve decisions, preferences, facts and open questions with
citations to your indexed chats. Ask your agent to inspect the source discussion
before recording a conclusion. Records are interpretations; a matching quote
does not prove that an interpretation is correct.

The plugin recalls existing memory at session start and before file reads.
It also tells the agent how to save memory as it happens: when you state a
lasting preference, make or confirm a decision, correct how the agent works, or
settle a non-obvious fact. A prompt that reads like a standing preference or a
decision gets a reminder to save it.

The agent cites its own session and your exact words. Mash checks the quote
against that session's transcript at once, so saving does not wait for
indexing and the record is recalled straight away. If a later index of that
chat does not contain the quoted message, the evidence is marked stale and
default recall leaves the record out. Decisions and
preferences are established only by your own words; other conclusions are
saved as candidates. The exception is a choice the agent makes of its own
accord, such as how to structure the code: it is saved as the agent's decision,
citing the agent's message that made and explained it, and recall labels it an
agent decision. An agent's decision never replaces one of yours; yours can
replace it. Reading alone never creates or confirms records.

Recall shows candidates after established records, labeled as unconfirmed. If
one is wrong, ask the agent to correct, withdraw or forget it.

As a safety net, the plugin occasionally asks the agent to save anything it
missed: never in a session's first fifteen minutes, and at most once every
fifteen minutes after that. Saving uses the agent you are already running,
including its normal permissions and token budget. It does not start another
agent, download models or run an independent background model. Install the
current Mash binary and plugin, then restart the agent. Codex also requires
reviewing changed plugin hooks in `/hooks` before they run.

Set `MASH_RECORD_CAPTURE=off` to disable the saving notes and the safety net
while retaining recall. `[records] enabled = false` in the node configuration
also disables capture and backfill. An agent can still miss or misinterpret a
conclusion; inspect important memory and its supporting messages.

## Backfill earlier memory

The `mash init` wizard offers to backfill memory from chats active in the last
3 days. This runs during setup, after indexing, using a selected signed-in
Codex or Claude Code CLI. It uses agent tokens and sends the selected chat text
to that agent's service. It is optional and separate from history indexing.
Unattended `mash init --yes` does not request historical record backfill.

Normal conversations never consume historical backfill. They recall existing
memory and capture their own new outcomes only.

To explicitly backfill already indexed chats later, without reindexing or
changing agent integrations:

```sh
mash init --yes --no-index --no-agents --backfill-memory codex
# Or: --backfill-memory claude-code
```

Setup saves its fixed cutoff as `[records] backfill_since` (Unix milliseconds),
so another explicit run can resume that window. Without a saved cutoff, an
explicit run uses the last 3 days. Completed source versions are skipped using
durable extraction checkpoints. Changed sources become eligible again. Failed
or partially read chats remain pending; nothing is delegated to later conversations.
Progress appears on stderr and the JSON result reports completed reviews and
whether the scan finished. A run stops after 100 chats or an unfinished review;
each chat has a ten-minute limit. Rerun explicitly to continue. Agents can still
miss conclusions, so inspect important saved memory.

For a small context packet, ask your agent to call `recall_records` with an exact
`scope`, such as `{"kind":"repository","identity":"YOUR_REPOSITORY_ID"}`.
An optional `query` ranks matching words; this is lexical retrieval and needs no
embedding model. Current established records rank first, with bounded boosts for
distinct confirmations and recent recording dates. Reading never confirms them.
Use `get_record` on an item's ID to inspect its supporting evidence.

Recall returns at most eight records by default and limits its structured JSON
result to 6,000 bytes. Set `limit` (1–20) and `byte_budget` (1,024–32,768) to adjust
those limits. `truncated` means a relevant result did not fit; `scan_limited`
means more than 256 recent records existed in the selected scope. Use
`list_records` and its pagination for a complete inspection.

Candidates follow current records with `status: "candidate"`. `include_review`
adds records labeled `stale_evidence` or `inactive`, and records with
`conflicted: true`; default recall and automatic hooks omit these. Conflict is separate from evidence freshness,
so a record can have both stale evidence and a known disagreement. These labels are not approval. Personal records are only
included alongside another scope when explicitly requested with `include_personal`.
For file context, supply `file: "src/main.rs"` with a repository or local-project
scope. It matches that exact relative path in the record text or retained quotes,
not `other/src/main.rs` or `src/main.rs.bak`; personal scope expansion is disabled
for file requests.

The plugin's session-start hook shows up to three project records before
recent chats, marking candidates as unconfirmed. Its file-read hook can show matching records once per file/session,
alongside earlier chats. Both call the same local recall API with a short deadline
and continue without record context if it fails. `MASH_RECORD_RECALL=off` disables
record context while retaining chat hints; `MASH_FILE_RECALL=off` disables the
entire file-read hook. These hooks never create or confirm records.

Ordinary `mash export` backups include records, their revisions, evidence and
confirmations. A normal import creates a new owner and remaps record IDs and
links. An `import --keep-identity` restore retains IDs and retry receipts; retire
the original database before using that restored copy as its replacement.
Backups contain sensitive record text and quoted evidence, as well as chats.

## Inspect existing records

For a visual, read-only view, open `mash dash` and choose **Memory**. The
[dashboard](dashboard.md#inspect-conversation-records) includes filters, revision
history, relationships and links to supporting chat messages.

Ask your agent to call `list_records`. By default it returns established current
records whose evidence still matches the saved messages. Use `include_inactive`
to inspect candidates, older choices and records with changed or missing sources.
Follow `next_offset` to continue, even if a page contains no current records.

Optional `kind`, `state` and `review` filters select the latest revision before
pagination. `chat` selects records citing that chat in any retained revision.
These filters also work on shared lists; keep them unchanged when following a
page cursor.

Use `get_record` with the returned ID. Set `history` to `true` to inspect prior
revisions, evidence, confirmations and links to replacement records. Reading does
not confirm or change anything. Each revision's `confirmation_history` lists the
distinct supporting message IDs and the dates those confirmations were recorded
(Unix milliseconds). These are confirmation dates, not the dates of the original
messages. A correction starts a new revision with its own confirmations; earlier
confirmation history remains attached to the earlier interpretation.

History returns up to eight revisions per page by default, newest first. Set
`limit` from 1 to 20 with `history: true` to change that count. Pass the returned
`next_before_revision` as `before_revision` to continue until it is null. New
revisions do not shift that cursor. Links are returned separately in pages of
128: pass `next_link_offset` as `link_offset` to continue. Link and list offsets
describe the current data; restart their traversal if concurrent edits change
the relationships or list order.

Pages also have size limits, so a page may contain fewer items than requested.
Follow the returned cursor rather than assuming a short page is the last one.
These limits apply to local and shared reads; records and their complete history
remain in the database and in exports.

To read published records, call `list_records` with `shared: true`. Results
include `node_id` and `owner`; identical statements from different owners remain
separate records. You can select one node with `node_id`. Shared pages visit
nodes in order: pass `next_cursor` back as `cursor`, preserving the other filters,
until no cursor remains. A page can be empty when its records are inactive.

Pass the result's `node_id` alongside its `id` to `get_record` to read that shared
copy. Shared reads use your configured group's authentication and need no models.
They do not silently fall back to local records when the server is unavailable.
Edits still happen on the owning node and reach the shared copy through normal
publication. Records scoped to a local project path stay local.

## Relate conclusions

To explicitly record agreement or disagreement, use `record` with
`change.action: "link"`, `expected: {"id": "SOURCE_ID", "revision": 1}`,
`target: {"id": "TARGET_ID", "revision": 1}`, and `relation: "supports"` or
`"contradicts"`, plus a new `operation_id`. Both endpoints must be established,
current records with matching evidence, owned locally in the same scope.
Mash does not infer contradictions from wording or choose a winner.

Links describe the record identities, not just their current wording. A correction
does not silently erase a disagreement. Retire or supersede a changed claim;
once either endpoint is inactive, the conflict is no longer active. Forgetting
either endpoint removes the link. Inspect both directions through `get_record`'s
paginated `links`. Adding or retrying a link never adds confirmations.

## Review an older chat

For an explicit review, call `get_extraction_checkpoint` with the chat ID and
the scope in which you intend to save conclusions. It returns the current
`source_fingerprint`, the last processed fingerprint (if any), and `up_to_date`.
Read the chat after obtaining that fingerprint.

Submit the reviewed fingerprint to `extract_records`, together with a stable
`operation_id`, the same scope and chat ID, and up to 64 record drafts. Each
draft contains `body` and `evidence`, as in the create example below, and must
have `review: "candidate"`. All cited messages must belong to the reviewed chat.
An empty `records` list records that the review found no conclusions to save.

Mash saves the drafts and checkpoint together. If the chat changed during the
review, it rejects the request: reread the chat before submitting a new operation.
If only the response was lost, retry the identical request with its original
operation ID. Review candidate conclusions separately before establishing them.
The checkpoint records your agent's claim to have reviewed that version; it
does not prove that the agent read every message or interpreted it correctly.

## Save a conclusion

Before creating a record, call `find_record_matches` with its proposed `scope`
and `body`. It returns up to eight exact or similar-wording suggestions of the
same kind and scope, scanning at most 256 recent records. Review their current
state, review status and evidence; similar wording does not prove agreement.
Suggestions never merge records or add confirmations.

`create` reports an identical live record by default. To explicitly reuse it,
set `on_duplicate: "reuse"` inside the create change. The receipt then has
`reused: true`; its ID and revision identify the existing record. This adds no
evidence or confirmation and does not change the original attribution. An exact
match requires all interpretation fields to agree, including state, review and
effective time, but ignores which agent recorded it. Retired choices are never
revived through reuse. Use `revise` or `supersede` when the interpretation changed.

The `record` MCP tool accepts an operation ID and a typed change. Reuse the same
operation ID when retrying the same request after a lost response; use a new one
for a different operation. This example records a decision after reading its
supporting chat:

```json
{
  "operation_id": "choose-local-qdrant-1",
  "change": {
    "action": "create",
    "scope": {"kind": "chat", "id": "RETURNED_CHAT_ID"},
    "body": {
      "statement": "Use Qdrant for local search.",
      "recorded_by": "My coding agent",
      "review": "established",
      "details": {"kind": "decision", "state": "accepted"}
    },
    "evidence": [{
      "message_id": "RETURNED_USER_MESSAGE_ID",
      "quote": "Yes, let's use Qdrant.",
      "purpose": "endorsement"
    }]
  }
}
```

Replace IDs and the quote with actual evidence. An agent recording something
from its own running session can cite it without a message ID:

```json
{"session_id": "THE_AGENTS_SESSION_ID", "quote": "Yes, let's use Qdrant.", "purpose": "endorsement"}
```

Mash then cites the latest message in that session containing the quote. For an
endorsement that is the latest message where the person visibly said it. A short approval may cite the
preceding proposal as `context` alongside the approval as `endorsement`.
User-role messages sometimes contain automated reports: do not classify those
as the person's endorsement. Keep uncertain interpretations as `candidate`
(the default review state). Established decisions and preferences require a
user-message endorsement citation. A decision with `"decided_by": "agent"` in
its details instead requires a citation of the agent's own message, and cannot
revise or supersede a decision made by the person. `recorded_by` is caller-supplied attribution,
not authenticated human identity.

Evidence `purpose` accepts `context`, `endorsement`, and `report`. Use `report`
for a source's attributed factual claim; it does not assert the person's approval
of a decision or preference. An omitted purpose defaults to `context`.

Retained evidence includes its stated purpose and the source message's role at
recording time. Older records that did not retain these fields show them as
unknown; Mash does not guess whether an old quote was an endorsement.
Changed or unknown source roles make the evidence stale, even if the quoted text
still matches. Use `include_inactive` to review these records and revise them
with verified sources before relying on them again.

| Kind | States | Additional fields |
| --- | --- | --- |
| `decision` | `proposed`, `accepted`, `superseded`, `withdrawn` | optional alternatives and consequences |
| `preference` | `active`, `superseded`, `withdrawn` | optional conditions |
| `fact` | `current`, `superseded`, `retracted` | required `reported_by`, optional `valid_until` |
| `open_question` | `open`, `resolved`, `dropped` | answer for resolved questions |

A current fact is an attributed claim, not independent verification. Optional
`effective_at` and fact `valid_until` use Unix milliseconds. Unknown dates should
be omitted. Expired facts and choices that do not yet apply are excluded from the
default current list.

Scopes can be `personal` within this record owner's namespace, `repository` with
an exact `identity`, `local_project` with an absolute `path`, or `chat` with an
ID. Repository identity and project path must match the cited chat metadata;
matching project basenames are not enough.

## Correct, replace or resolve

Edits provide `expected: {"id": "RECORD_ID", "revision": 1}`. If another edit
has changed that revision, reread the record before trying again.

To establish a candidate after reviewing its sources, use `revise` with its
expected ID/revision, a complete updated `body` with `review: "established"`,
and verified evidence. Decisions and preferences need a user endorsement.
Reading or confirming a candidate does not establish it.

- `revise` corrects a summary or changes its lifecycle state, with supporting
  evidence. New revisions do not inherit confirmations of the old interpretation.
- `supersede` records a changed choice and retires its predecessor atomically.
  Returning to an earlier choice creates another record, preserving the sequence.
- `confirm` adds new supporting source messages to an established current record.
  Reusing the same evidence or retrying a request does not increase confirmations.
- `resolve` supplies an answer and evidence for an open question. A later revision
  can reopen it by setting its state to `open` and removing the answer.
  Optionally provide `resolved_by: {"id": "ANSWER_RECORD_ID", "revision": 1}`
  to link the decision, preference or fact that answered it. That record must
  belong to the same owner and scope, be established and current, and have
  matching evidence. Both revisions are checked together. The link remains as
  history if the question reopens or the answer later changes; inspect their
  current states before treating the question as settled.
- `forget` removes the entire record, including its prior revisions and quotes.

Resolved and dropped questions are inactive, so default recall and lists omit
them. Their answers and history remain available through `get_record`; use
`recall_records` with `include_review: true` or `list_records` with
`include_inactive: true` to find them. If an answer should remain in current
recall, save the resulting fact or decision separately and link it with
`resolved_by` when resolving the question.

Ordinary source updates retain records but mark changed evidence stale.
**Forgetting a cited chat also removes every dependent record**, including records
that cited that chat only in an older revision. A small content-free tombstone
prevents an old successful request from recreating the forgotten text.

Record writes reject text containing private spans. Normal output applies your
configured credential masking. Records are stored locally without downloading
models or running inference; record-only edits do not rebuild chat search.
