# Record what you worked out

Native records preserve decisions, preferences, facts and open questions with
citations to your indexed chats. Ask your agent to inspect the source discussion
before recording a conclusion. Records are interpretations; a matching quote
does not prove that an interpretation is correct.

The current implementation provides local recording, shared reads and portable
backups. Automatic recall integration is still being added.

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
not confirm or change anything.

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

## Save a conclusion

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
- `forget` removes the entire record, including its prior revisions and quotes.

Ordinary source updates retain records but mark changed evidence stale.
**Forgetting a cited chat also removes every dependent record**, including records
that cited that chat only in an older revision. A small content-free tombstone
prevents an old successful request from recreating the forgotten text.

Record writes reject text containing private spans. Normal output applies your
configured credential masking. Records are stored locally without downloading
models or running inference; record-only edits do not rebuild chat search.
