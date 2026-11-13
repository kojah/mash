# Record what you worked out

Records keep decisions, preferences and facts, each with a quote from the chat
that supports it. With the Mash plugin installed, your agent saves and recalls
them for you:

- **Recall:** at session start the agent sees up to three project records, and
  matching records once per file when it reads a file.
- **Save:** the agent saves a record when you state a lasting preference, make
  or confirm a decision, correct how it works, or settle a non-obvious fact.

Install the current Mash binary and plugin, then restart the agent. Codex also
requires reviewing changed plugin hooks in `/hooks` before they run.

Records are interpretations. A matching quote shows the words were said, not
that the interpretation is correct, and an agent can miss or misread a
conclusion. Inspect important records; if one is wrong, ask the agent to
correct, withdraw or forget it.

## How saving works

The agent cites its own session and your exact words. Mash checks the quote
against that session's transcript at once, so the record is usable before the
chat is indexed. If a later index of that chat lacks the quoted message, the
evidence is marked stale and default recall leaves the record out.

- Decisions and preferences are established only by your own words. Other
  conclusions are saved as candidates, which recall shows after established
  records, labeled as unconfirmed.
- A choice the agent makes of its own accord, such as how to structure the
  code, is saved as an agent decision citing the agent's own message. It never
  replaces one of your decisions; yours can replace it.
- Reading never creates or confirms records.

The plugin also asks the agent to save anything it missed: never in a
session's first fifteen minutes, and at most once every fifteen minutes after
that. The reminder travels with your next message, so something settled in a
session's very last message is not swept.

Saving uses the agent you are already running, with its normal permissions and
token budget. It does not start another agent or download models.

| Setting | Effect |
| --- | --- |
| `MASH_RECORD_CAPTURE=off` | No saving notes or safety net; recall stays on |
| `[records] enabled = false` in the node configuration | Also disables capture and backfill |
| `MASH_RECORD_RECALL=off` | No record context; chat hints stay on |
| `MASH_FILE_RECALL=off` | Disables the entire file-read hook |

## Backfill earlier memory

The `mash init` wizard offers to backfill memory from chats active in the last
3 days, using a signed-in Codex or Claude Code CLI that you select. **Backfill
uses agent tokens and sends the selected chat text to that agent's service.**
It is optional and separate from history indexing. Unattended `mash init --yes`
does not request it, and normal conversations never perform it.

To backfill already indexed chats later, without reindexing or changing agent
integrations:

```sh
mash init --yes --no-index --no-agents --backfill-memory codex
# Or: --backfill-memory claude-code
```

A run stops after 100 chats or an unfinished review; each chat has a
ten-minute limit. The JSON result reports completed reviews and whether the
scan finished. Rerun the command to continue: reviewed chats are skipped unless
they changed, and failed or partially read chats remain pending.

Setup saves its cutoff as `[records] backfill_since` (Unix milliseconds), so a
later run resumes the same window. Without a saved cutoff, a run uses the last
3 days.

## Recall records

Ask your agent to call `recall_records` with an exact `scope`:

```json
{"scope": {"kind": "repository", "identity": "YOUR_REPOSITORY_ID"}, "query": "search engine"}
```

The optional `query` ranks records by matching words and needs no embedding
model. Established current records come first, then candidates with
`status: "candidate"`.

- Recall returns at most eight records and 6,000 bytes by default. Adjust with
  `limit` (1–20) and `byte_budget` (1,024–32,768).
- `truncated` means a relevant result did not fit. `scan_limited` means the
  scope held more than 256 recent records. Use `list_records` for a complete
  inspection.
- `include_review` adds records labeled `stale_evidence` or `inactive`, and
  records with `conflicted: true`. Default recall and the plugin hooks omit
  them. These labels are not approval.
- `include_personal` adds personal records alongside another scope.
- `file: "src/main.rs"` with a repository or local-project scope matches that
  exact relative path in the record text or quotes.

## Inspect existing records

For a visual, read-only view, open `mash dash` and choose **Memory**. The
[dashboard](dashboard.md#inspect-conversation-records) includes filters, revision
history, relationships and links to supporting chat messages.

Or ask your agent to call `list_records`. By default it returns established
current records whose evidence still matches the saved messages.

- `include_inactive` adds candidates, older choices and records with changed
  or missing sources.
- `kind`, `state` and `review` filter on the latest revision. `chat` selects
  records citing that chat in any retained revision.
- Follow `next_offset` to continue, keeping the filters unchanged. A page can
  be short or hold no current records without being the last one.

Call `get_record` with a returned ID to read one record and its evidence. Set
`history` to `true` for prior revisions, confirmations and links to
replacement records, following the returned cursors until they are null.
Confirmation dates are when each confirmation was recorded, not when the
original message was sent.

### Shared records

To read published records, call `list_records` with `shared: true`. Results
include `node_id` and `owner`; identical statements from different owners stay
separate. Select one node with `node_id`, and pass `next_cursor` back as
`cursor` with the same filters until no cursor remains. Pass a result's
`node_id` alongside its `id` to `get_record` to read that shared copy.

Shared reads use your configured group's authentication and need no models.
They do not fall back to local records when the server is unavailable. Edits
happen on the owning node and reach the shared copy through normal publication.
Records scoped to a local project path stay local.

## Save a conclusion

To save something yourself, tell the agent. In Claude Code, run
`/mash:remember` followed by what to keep, or with nothing after it to save the
latest decision in the conversation. In Codex, pick the plugin's `remember`
skill or say "remember this". The agent checks for an existing record, saves
your words as the evidence, and confirms what it saved.

To do it by hand, read the supporting chat first, then call the `record` MCP
tool with an operation ID and a change:

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

Replace the IDs and the quote with actual evidence. Reuse the same operation
ID only when retrying the same request after a lost response.

**Scope** is one of:

- `personal`, within this record owner's namespace
- `repository` with an exact `identity`
- `local_project` with an absolute `path`
- `chat` with an `id`

Repository identity and project path must match the cited chat's metadata;
a matching project basename is not enough.

**Kind and state** go in `details`:

| Kind | States | Additional fields |
| --- | --- | --- |
| `decision` | `proposed`, `accepted`, `superseded`, `withdrawn` | optional alternatives and consequences |
| `preference` | `active`, `superseded`, `withdrawn` | optional conditions |
| `fact` | `current`, `superseded`, `retracted` | required `reported_by`, optional `valid_until` |

A current fact is an attributed claim, not independent verification. Optional
`effective_at` and `valid_until` use Unix milliseconds; omit unknown dates.
Expired facts and choices not yet in effect are left out of the default list.

**Review** is `candidate` (the default) or `established`. Keep uncertain
interpretations as candidates. An established decision or preference requires
an `endorsement` citation of a user message. A decision with
`"decided_by": "agent"` in its details instead requires a citation of the
agent's own message, and cannot revise or supersede a decision made by the
person. `recorded_by` is caller-supplied attribution, not authenticated
identity.

**Evidence** `purpose` is one of:

- `context` (the default): surrounding material, such as the proposal a short
  approval answers. Cite the proposal as `context` and the approval as
  `endorsement`.
- `endorsement`: the person visibly said it. User-role messages sometimes
  contain automated reports; do not cite those as endorsements.
- `report`: a source's attributed factual claim. It does not assert the
  person's approval.

An agent recording from its own running session can cite it without a message
ID. Mash cites the latest message in that session containing the quote:

```json
{"session_id": "THE_AGENTS_SESSION_ID", "quote": "Yes, let's use Qdrant.", "purpose": "endorsement"}
```

Record writes reject text containing private spans, and normal output applies
your configured credential masking.

### Check for duplicates

Before creating a record, call `find_record_matches` with the proposed `scope`
and `body`. It returns up to eight exact or similar-wording suggestions of the
same kind and scope. Similar wording does not prove agreement, and suggestions
never merge records or add confirmations.

`create` reports an identical live record instead of adding a second one. To
reuse it, set `on_duplicate: "reuse"` inside the create change; the receipt
then has `reused: true` and the existing record's ID and revision. Reuse adds
no evidence or confirmation and never revives a retired choice.

## Correct or replace

Edits provide `expected: {"id": "RECORD_ID", "revision": 1}`. If another edit
has changed that revision, reread the record before trying again.

- `revise` corrects a summary or changes its lifecycle state, with supporting
  evidence. New revisions do not inherit confirmations of the old interpretation.
- `supersede` records a changed choice and retires its predecessor atomically.
  Returning to an earlier choice creates another record, preserving the sequence.
- `confirm` adds new supporting source messages to an established current record.
  Reusing the same evidence or retrying a request does not increase confirmations.
- `forget` removes the entire record, including its prior revisions and quotes.

To establish a candidate, `revise` it with a complete `body` that has
`review: "established"` and verified evidence. Reading or confirming a
candidate does not establish it.

Reindexing a changed chat keeps its records but marks changed evidence stale.
**Forgetting a cited chat also removes every dependent record**, including
records that cited that chat only in an older revision. A forgotten record
cannot be recreated by retrying an old request.

## Relate conclusions

To record that two records agree or disagree, use `record` with a new
`operation_id` and a `link` change:

```json
{
  "operation_id": "link-qdrant-choices-1",
  "change": {
    "action": "link",
    "expected": {"id": "SOURCE_ID", "revision": 1},
    "target": {"id": "TARGET_ID", "revision": 1},
    "relation": "contradicts"
  }
}
```

`relation` is `supports` or `contradicts`. Both records must be established,
current, backed by matching evidence, and owned locally in the same scope. Mash
does not infer contradictions from wording or choose a winner.

A correction does not erase a disagreement. Retire or supersede a changed
claim: once either record is inactive, the conflict is no longer active.
Forgetting either record removes the link. Read links in both directions
through `get_record`'s `links`.

## Review an older chat

To review one chat by hand instead of running a backfill:

1. Call `get_extraction_checkpoint` with the chat ID and the scope in which
   you intend to save conclusions. It returns the current
   `source_fingerprint`, the last processed fingerprint (if any) and
   `up_to_date`.
2. Read the chat.
3. Call `extract_records` with that fingerprint, a stable `operation_id`, the
   same scope and chat ID, and up to 64 record drafts. Each draft has `body`
   and `evidence` as in [Save a conclusion](#save-a-conclusion), must have
   `review: "candidate"`, and may cite only messages from the reviewed chat.
   An empty `records` list records that the review found nothing to save.

If the chat changed during the review, the request is rejected: reread the
chat and submit a new operation. If only the response was lost, retry the
identical request with its original operation ID.

The checkpoint records the agent's claim to have reviewed that version, not
proof that it read every message or interpreted it correctly. Review the
candidates before establishing them.

## Back up records

Ordinary `mash export` backups include records with their revisions, evidence
and confirmations. **Backups contain sensitive record text and quoted
evidence, as well as chats.**

A normal import creates a new owner and remaps record IDs and links. An
`import --keep-identity` restore retains IDs and retry receipts; retire the
original database before using the restored copy as its replacement.
