# Introduction to Mash search

Mash searches the conversations you have had with coding agents (Claude Code,
Codex, GitHub Copilot, Cursor, OpenCode, and Antigravity) on this machine, and
on your other machines once you set up a [shared index](shared-index.md). It
searches what you and the agent wrote; tool calls and their output are kept
for reading but are not searched.

There are two ways to search, chosen with `--intent`:

- **Word search** (`--intent exact-mention`, the default) finds messages
  containing every word you type, in any order. It needs no models.
- **[Search by meaning](#search-by-meaning)** (`--intent related-discussion`)
  finds messages about the same idea even when the wording differs. It needs
  the optional semantic models and takes about a second longer.

Without `--intent`, Mash searches by word unless your configuration sets
another [default](configuration.md#search-defaults).

## Your first search

Search for the distinctive words, filename, or identifier you remember:

```bash
mash search 'authentication token'
```

Results are JSON, grouped by chat, best match first. Abbreviated:

```json
{
  "results": [
    {
      "chat_id": "01a1a77a-8105-7cf1-849b-37524f39e223",
      "provider": "codex",
      "title": "Add token authentication to the dashboard",
      "project": "/home/you/project",
      "matches": [
        {
          "lookup": { "view": { "id": "709e029507a46e56c1e1cf40" } },
          "role": "assistant",
          "snippet": "It's an authentication gate: the dashboard asks for a token…"
        }
      ]
    }
  ],
  "next_offset": 20
}
```

Each result is one chat: `title`, `provider` (the agent), and `project` (the
directory it ran in) identify it. Each entry in `matches` is a message in that
chat, with a `snippet` of the matching text and the message ID at
`lookup.view.id`. Pass that ID to `mash view` to read the full message and
its neighbours:

```bash
mash view 709e029507a46e56c1e1cf40 --context 3
```

Search reads the existing index and does not refresh it. To find a
conversation you just had, run `mash index` first.

Prefer a visual view? Use the [dashboard](dashboard.md). Inside an agent, ask
“Mash, find our earlier discussion about authentication”; see
[Use Mash with your agent](agents.md).

## Read the original messages

```bash
mash view MESSAGE_ID --context 3
```

`MESSAGE_ID` is a `lookup.view.id` from a search or `list-chats` result.
`--context 3` adds up to three messages before and after it. An ID that
matches no message fails with `not-found`. A chat ID is not accepted either;
that error names the chat's first message ID to use instead.

For local history, the output gives the message's `chat_id`, the chat's
`message_count`, the `messages` themselves, and how many messages come before
and after them (`omitted_before`, `omitted_after`). When some are left out, a
`notice` says so; raise `--context` to read more.

A result from another machine also has `lookup.view.shared`, the ID of the
node (machine) that published it. Pass it with `--shared`:

```bash
mash view MESSAGE_ID --shared NODE_ID --context 3
```

## Find a chat by title

```bash
mash list-chats --title-contains 'authentication' --limit 10
```

This lists chats without searching their messages, so it works without models
or a working search index. A chat's title is its first message, or for a named
Claude Code session, that name. A new name appears the next time the session
is indexed.

Each chat carries `lookup.view.id` for its first message, so you can start
reading from the top:

```bash
mash view FIRST_MESSAGE_ID --context 10
```

To search within one chat, pass its `chat_id` and ask for individual messages
rather than chats:

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
| The exact spelling | `mash search 'auth_token' --match-mode strict` |
| The exact phrase | `mash search '"rotate the logs"'` |

Different filters combine: a result must satisfy all of them. Repeat one flag
to accept alternatives, such as `--provider codex --provider claude-code`.

`--project` and `--repository` must match the recorded value exactly. Copy it
from the `project` or `repository` field of a `list-chats` or search result.

`--after` and `--before` accept a date (taken as UTC midnight) or an RFC3339
timestamp. `--after` includes the boundary; `--before` excludes it. On
`list-chats`, the same two flags instead take Unix seconds and filter by chat
activity.

## Search by meaning

Once [semantic search is enabled](indexing.md#enable-semantic-search), describe
what you remember in your own words:

```bash
mash search 'why did we choose a separate database' --intent related-discussion
```

Semantic queries currently need English.

Choose the intent by what you remember:

| | `exact-mention` | `related-discussion` |
| --- | --- | --- |
| Use for | Words, identifiers, filenames, error text | A question or idea whose wording may differ |
| Needs models | No | Yes |
| Several words | Each must appear in the message, in any order | Need not appear at all |

Word search matches the words themselves, so “log rotation” won't find “rotate
the application logs”; try “rotate”, or search by meaning. A single term such
as `auth_token` or `voyage-code-4` matches as written, not as separate words.
By default a word also matches as a prefix and tolerates small typos;
`--match-mode strict` requires the exact spelling.

## Search for an exact phrase

Put double quotes around the query, inside the shell's own quotes:

```bash
mash search '"rotate the logs"'
```

A quoted query matches as one phrase with exact spelling, not as independent
words scattered through a message. If it finds nothing, remove the quotes.

## More results and local-only searches

A page holds 20 results unless you set `--limit`. For the next page, repeat the
same command with `--offset` set to the returned `next_offset`; stop when it is
null.

`--sort recent` puts the newest results first within each page of relevant
results. For a chronological list of chats, use `list-chats` instead.

With a [shared index](shared-index.md), search combines this machine with the
history other machines published. `--local` searches only this machine;
`--shared` searches only published history. `--node NODE_ID` keeps results
recorded by that machine; it does not connect to it.

If results are missing or stale, start with [troubleshooting](troubleshooting.md).
For other commands, see [useful commands](cli.md).
