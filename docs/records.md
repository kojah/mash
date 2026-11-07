# Record what you worked out

Native records preserve decisions, preferences, facts and open questions with
citations to your indexed chats. Ask your agent to inspect the source discussion
before recording a conclusion. Records are interpretations; a matching quote
does not prove that an interpretation is correct.

The current implementation provides local recording, shared reads, portable
backups and bounded record context in the plugin's session and file-read hooks.

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

`include_review` adds records labeled `candidate`, `stale_evidence` or `inactive`
after current records. These labels are not approval. Personal records are only
included alongside another scope when explicitly requested with `include_personal`.
For file context, supply `file: "src/main.rs"` with a repository or local-project
scope. It matches that exact relative path in the record text or retained quotes,
not `other/src/main.rs` or `src/main.rs.bak`; personal scope expansion is disabled
for file requests.

The plugin's session-start hook shows up to three current project records before
recent chats. Its file-read hook can show matching records once per file/session,
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

Ask your agent to call `list_records`. By default it returns established current
records whose evidence still matches the saved messages. Use `include_inactive`
to inspect candidates, older choices and records with changed or missing sources.
Follow `next_offset` to continue, even if a page contains no current records.

Use `get_record` with the returned ID. Set `history` to `true` to inspect prior
revisions, evidence, confirmations and links to replacement records. Reading does
not confirm or change anything. Each revision's `confirmation_history` lists the
distinct supporting message IDs and the dates those confirmations were recorded
(Unix milliseconds). These are confirmation dates, not the dates of the original
messages. A correction starts a new revision with its own confirmations; earlier
confirmation history remains attached to the earlier interpretation.

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

Replace IDs and the quote with actual evidence. A short approval may cite the
preceding proposal as `context` alongside the approval as `endorsement`.
User-role messages sometimes contain automated reports: do not classify those
as the person's endorsement. Keep uncertain interpretations as `candidate`
(the default review state). Established decisions and preferences require a
user-message endorsement citation. `recorded_by` is caller-supplied attribution,
not authenticated human identity.

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

Ordinary source updates retain records but mark changed evidence stale.
**Forgetting a cited chat also removes every dependent record**, including records
that cited that chat only in an older revision. A small content-free tombstone
prevents an old successful request from recreating the forgotten text.

Record writes reject text containing private spans. Normal output applies your
configured credential masking. Records are stored locally without downloading
models or running inference; record-only edits do not rebuild chat search.
