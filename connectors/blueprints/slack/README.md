# Slack → Mash

> Experimental blueprint. Not yet supported for public use; retained tests do not establish production readiness.

A runnable, read-only connector for allowlisted Slack channels. Each channel
becomes a `slack:channel` container; its messages and thread replies are
`slack:message` entries in it, and a reply links to its thread's parent. It
uses the [Python SDK](../../../sdk/python/README.md) and Slack's official
[`slack_sdk`](https://github.com/slackapi/python-slack-sdk) async Web API
client, which retries rate-limited calls after `Retry-After`. This package
supersedes the [`slack.py` blueprint](../slack.py).

## Sign in

```sh
uv sync --project connectors/blueprints/slack --locked
alias slack-mash='uv run --project connectors/blueprints/slack --locked mash-slack --config ~/.config/mash/slack.toml'

slack-mash login
```

`login` does the whole setup in the browser:

1. **Create Mash's Slack app (once per workspace).** It opens Slack's app
   creation page with Mash's settings already filled in: read-only user
   permissions, browser sign-in for desktop tools (PKCE) and a return address on
   this machine (`http://localhost:47643/slack/callback`). Pick the workspace and
   click **Create**, then paste the app's **Client ID** from **Basic
   Information**. Pass `--client-id` next time to skip this step.
2. **Approve Mash.** It opens Slack's usual permission page and waits for the
   reply on this machine. No client secret is involved.
3. **Save.** It asks Slack which workspace and user you signed in as, saves the
   token to a file only you can read, and, if `--config` does not exist yet,
   writes a configuration with `team_id`, `principal` and `token_file` filled in.
   An existing configuration must be for the same workspace and user; the new
   token replaces its old one.

Then add the channel IDs to index to `channels` in the configuration (an empty
list indexes nothing). A channel's ID is at the bottom of its **View channel
details** dialog. See [example.toml](example.toml) for every setting; paths are
relative to the configuration file, with `~` expansion.

Why your own app: Slack gives apps made for one workspace its normal Web API
limits, while apps distributed to other workspaces get a much lower
`conversations.history` limit unless Slack approves them for its directory. The
app is yours, used only by you.

What the sign-in allows and keeps:

- **You read as yourself.** Browser sign-in for desktop tools cannot request bot
  permissions, so Mash sees exactly the channels and DMs you can, and only those
  you list. Permissions: `channels:read`/`history` (public channels),
  `groups:read`/`history` (private channels), `im:` and `mpim:` `read`/`history`
  (DMs you list), and `users:read` (names).
- **The token lasts until revoked.** Token rotation stays off, so the token does
  not expire; remove the app or its authorization in Slack to revoke it, and run
  `login` again to replace it. Turning on browser sign-in marks the app as a
  public client in Slack, which only Slack support can undo; that is fine for an
  app only you use.
- **The token stays in its file.** It is never printed, logged or copied into
  connector state.

## Run

```sh
slack-mash --check      # Offline setup report; no ingestion, state or network.
slack-mash sync         # One poll and delivery.
slack-mash sync --watch # Keep polling in this process; Ctrl-C stops it.

mash index --rebuild-local-views
mash search 'your message text' --source slack:T0123456789:U0123456789 --local
```

Each poll prints JSON counts and content-free issues, never text or tokens.
Exit codes: 1 delivery failure, 2 configuration or state (including a failed
`--check`, a failed `login` and a token for another workspace or user), 3 no
saved token, 9 partial poll (a channel or `auth.test` failed; that channel's
cursor stays put and the next poll resumes). `--check` follows the
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

Tests serve an in-memory Slack Web API over local HTTP to the real client,
including the secret-free sign-in exchange; the
integration test delivers into a disposable Mash database and finds a message
and a thread reply with `mash search`/`view`.
