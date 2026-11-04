# Slack → Mash

> Experimental blueprint. Not yet supported for public use; retained tests do not establish production readiness.

A runnable, read-only connector for allowlisted Slack channels. Each channel
becomes a `slack:channel` container; its messages and thread replies are
`slack:message` entries in it, and a reply links to its thread's parent. It
uses the [Python SDK](../../../sdk/python/README.md) and Slack's official
[`slack_sdk`](https://github.com/slackapi/python-slack-sdk) async Web API
client, which retries rate-limited calls after `Retry-After`. This package
supersedes the [`slack.py` blueprint](../slack.py).

## Slack app and token (once)

Create an app **from scratch** in your workspace at
<https://api.slack.com/apps>. Apps made in and for your own workspace keep
Slack's normal Web API rate limits; unlisted apps distributed to other
workspaces get a much lower `conversations.history` limit. Under **OAuth &
Permissions**, add read scopes, install the app and copy a token:

- **User token** (`xoxp-`, User Token Scopes): reads what you can read,
  including private channels you are in. No invitations needed.
- **Bot token** (`xoxb-`, Bot Token Scopes): reads only channels the bot was
  invited to (`/invite @your-app` in each channel).

Scopes, for either token type:

- `channels:read`, `channels:history`: public channels.
- `groups:read`, `groups:history`: private channels.
- `users:read`: author and mention names.
- `im:read`, `im:history`, `mpim:read`, `mpim:history`: only if you list
  direct-message (`D…`) or group DM IDs.

`auth.test` needs no scope; it supplies the workspace name and address used
for message links (no `chat.getPermalink` calls) and must match `team_id`.
Export the token in the variable named by `token_env` (default `SLACK_TOKEN`).
Tokens are never written to state or output.

## Configure and run

```sh
uv sync --project connectors/blueprints/slack --locked
```

Copy [example.toml](example.toml) outside Git, e.g. to
`~/.config/mash/slack.toml`, and set the team ID, your user ID (`principal`),
channel IDs and destination. A channel's ID is at the bottom of its **View
channel details** dialog; the team ID is in `auth.test` output or the
workspace's URL in Slack's web app. `principal` is the user the token acts as:
`auth.test`'s `user_id`, or **Copy member ID** in your Slack profile. `sync`
rejects a token for another user. Paths are
relative to the configuration file, with `~` expansion.

```sh
alias slack-mash='uv run --project connectors/blueprints/slack --locked mash-slack --config ~/.config/mash/slack.toml'

slack-mash --check      # Offline setup report; no ingestion, state or network.
slack-mash sync         # One poll and delivery.
slack-mash sync --watch # Keep polling in this process; Ctrl-C stops it.

mash index --rebuild-local-views
mash search 'your message text' --source slack:T0123456789:U0123456789 --local
```

Each poll prints JSON counts and content-free issues, never text or tokens.
Exit codes: 1 delivery failure, 2 configuration or state (including a failed
`--check` and a token for another workspace), 3 no token, 9 partial poll (a
channel or `auth.test` failed; that channel's cursor stays put and the next poll
resumes). `--check` follows the
[prerequisite declaration](src/mash_slack/prerequisites.toml). It makes no
network requests, so Slack's acceptance of the token stays `unverified` until
`sync` calls `auth.test` and reads the channels.

## Capture behavior

- `channels` is a channel ID allowlist; an empty list selects nothing.
  Messages are `<channel ID>:<ts>`, with Slack's timestamp string unchanged.
- Polling only; the Events API needs a public endpoint and Socket Mode is not
  implemented. Each poll calls `conversations.info` and lists top-level
  messages (`conversations.history`) from `thread_days` ago, or from the
  channel's cursor if that is older (after downtime). Thread parents carry
  `latest_reply`; a thread is fetched with `conversations.replies` the first
  time it is seen and whenever that changed since the last poll. The SDK sends
  only what changed.
- Limits of polling: a new reply to a thread whose parent is older than
  `thread_days` is not found, nor is an edit to a reply in a thread without new
  replies. Edits to top-level messages are found within `thread_days`.
  Deletions are never detected (Slack reports them only as events), so deleted
  messages stay in Mash. Snapshots are incomplete and never delete by omission.
  A larger `thread_days` finds more at the cost of more history pages per poll.
- `history_days` uses message creation time: replies in the window are kept
  even if their parent is older (the reply link then points outside the
  window). Narrowing the window or the allowlist never deletes earlier history.
  Changing destinations or the window restarts the cursor from the window start.
- Ordinary, bot, `/me`, file-share and thread-broadcast messages are kept;
  joins, leaves, topic and name changes, pins and other channel events are
  skipped. Edited messages carry `updated_at` from Slack's `edited` stamp.
- Text is Slack's mrkdwn with `<@U…>` mentions shown as `@name`, channel links
  as `#name` and links as `label (url)`. Authors are `slack:<user or bot ID>`
  with names from `users.info`, looked up once per poll. Files are attachment
  references (title, type, size, Slack permalink); nothing is downloaded. A
  message with only files reads `(file: title)`.
- Channels carry the workspace, kind (channel, im, mpim), private and archived
  flags. Archiving a channel or losing access is never a deletion.
- slack_sdk retries HTTP 429 up to three times after `Retry-After`, and
  connection errors once. Other failures, such as `not_in_channel` or
  `missing_scope`, are reported per channel.

## Verify

```sh
uv run --project connectors/blueprints/slack --locked ruff check connectors/blueprints/slack
uv run --project connectors/blueprints/slack --locked ruff format --check connectors/blueprints/slack
uv run --project connectors/blueprints/slack --locked ty check connectors/blueprints/slack
MASH_CONNECTOR_MASH_BINARY=/absolute/path/to/mash \
  uv run --project connectors/blueprints/slack --locked python -m unittest discover \
  -s connectors/blueprints/slack/tests
```

Tests serve an in-memory Slack Web API over local HTTP to the real client; the
integration test delivers into a disposable Mash database and finds a message
and a thread reply with `mash search`/`view`.
