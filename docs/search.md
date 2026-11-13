# Find a conversation

Start with a distinctive word, filename, or phrase you remember:

```bash
mash search 'authentication' --intent exact-mention
```

Without semantic models, use wording from the conversation. Multiple words
match a phrase, and different word forms may not match: “log rotation” won't
find “rotate the application logs.” Try “rotate,” or use
[search by meaning](#search-by-meaning) when you remember the idea rather than
the wording.

Results are JSON. By default they group matching messages by chat. Search reads
the existing index; it does not refresh history. If you just had the conversation,
run `mash index` first.

Prefer a visual view? Use the [dashboard](dashboard.md). Inside an agent, ask
“Mash, find our earlier discussion about authentication.”

## Read the original messages

Copy `lookup.view.id` from a search result, then read that message with nearby
context:

```bash
mash view MESSAGE_ID --context 3
```

Replace `MESSAGE_ID` with the returned message ID, not the chat ID. A chat ID
or an ID that matches no message is an error (`not-found`); when you pass a chat
ID, the error names the chat's first message ID to use instead.
For a shared result, `lookup.view.shared` names its publishing node:

```bash
mash view MESSAGE_ID --shared NODE_ID --context 3
```

## Find a chat by title

```bash
mash list-chats --title-contains 'authentication' --limit 10
```

A Claude Code session that has a name is listed under that name; other chats
use their first message. Names appear as each session is next indexed.

This lists chat metadata without models or a working search index. Each chat
carries `lookup.view` for its first message, so you can start reading it:

```bash
mash view FIRST_MESSAGE_ID --context 10
```

To search within one of those chats, copy its `chat_id`:

```bash
mash search 'token' --chat CHAT_ID --unit messages
```

## Narrow the results

| I remember… | Example |
| --- | --- |
| The agent | `mash search 'authentication' --provider codex` |
| Roughly when | `mash search 'authentication' --days 7` |
| A date range | `mash search 'authentication' --after 2026-10-01 --before 2026-11-01` |
| The project directory | `mash search 'authentication' --project /absolute/path/to/project` |
| The repository | `mash search 'authentication' --repository https://github.com/owner/repo.git` |
| Something I said | `mash search 'authentication' --role user` |
| The exact spelling | `mash search 'auth_token' --intent exact-mention --match-mode strict` |

Project paths and repository URLs must match the recorded metadata exactly.
Copy them from `list-chats` or search results. Repeat a flag to include alternatives,
such as `--provider codex --provider claude-code`; different filters narrow the
results together.

Search date bounds accept dates (UTC midnight) or RFC3339 timestamps.
`--after` includes the boundary; `--before` excludes it. Unlike search,
`list-chats --after/--before` take Unix seconds and filter chat activity.

## Search by meaning

Once [semantic search is enabled](indexing.md#enable-semantic-search):

```bash
mash search 'why did we choose a separate database' --intent related-discussion
```

Use `exact-mention` for words or identifiers and `related-discussion` for a
question whose wording may differ from the conversation. Semantic queries
currently need English. Exact-mention search works without models.

Exact-mention matches a multiword query as a phrase, not independent words
scattered through a chat. If it finds nothing, try the most distinctive word.
Default matching allows prefixes and conservative typos; `--match-mode strict`
keeps spelling exact.

## More results and local-only searches

Use `--limit 10` for a smaller page. For the next page, repeat the same request
with `--offset` set to the returned `next_offset`; stop when it is null.

`--sort recent` shows the newest qualifying results first within each relevance
page. For a chronological chat list, use `list-chats` instead.

With a [shared index](shared-index.md), normal search combines this machine and
other nodes. `--local` uses only this machine; `--shared` uses only published
history. `--node` filters recorded node identity—it does not connect to that machine.

If results are missing or stale, start with [troubleshooting](troubleshooting.md).
