# Discord → Mash

A runnable, read-only connector for allowlisted Discord channels and threads.
Each channel becomes a `discord:channel` container and each thread a
`discord:thread` container; their messages are `discord:message` entries. It uses
the [Python SDK](../../sdk/python/README.md) and [hikari](https://github.com/hikari-py/hikari)'s
REST-only client, which never opens the gateway and handles Discord's rate
limits. This package supersedes the polled part of the
[`discord.py` blueprint](../discord.py); gateway live events are not implemented.

## Bot (once)

Only **bot** tokens are supported. Automating a user account ("self-bots",
user tokens) violates Discord's Terms of Service and is not supported.

1. In the [Developer Portal](https://discord.com/developers/applications),
   create an application and open **Bot**. Reset and copy the token.
2. On the same page, enable the **Message Content Intent** (a privileged
   intent). Without it Discord returns empty message text even over REST; the
   connector checks the application's flags each poll and reads nothing until
   it is enabled.
3. Under **OAuth2 → URL Generator**, select the `bot` scope and the **View
   Channels** and **Read Message History** permissions, then open the URL to
   add the bot to your server. Private channels and threads also need the bot
   (or one of its roles) added to them.
4. Enable **Developer Mode** in Discord's settings (Advanced) and use **Copy
   Channel ID** on each channel and thread to select.

Export the token in the variable named by `token_env` (default
`DISCORD_BOT_TOKEN`). It is never written to state or output.

## Configure and run

```sh
uv sync --project connectors/discord --locked
```

Copy [example.toml](example.toml) outside Git, e.g. to
`~/.config/mash/discord.toml`, and set the account ID, channels and destination.
Paths are relative to the configuration file, with `~` expansion.

```sh
alias dc-mash='uv run --project connectors/discord --locked mash-discord --config ~/.config/mash/discord.toml'

dc-mash --check      # Offline setup report; no ingestion, state or network.
dc-mash sync         # One poll and delivery.
dc-mash sync --watch # Keep polling in this process; Ctrl-C stops it.

mash index --rebuild-local-views
mash search 'your message text' --source discord:personal --local
```

Each poll prints JSON counts and content-free issues, never text or tokens.
Exit codes: 1 delivery failure, 2 configuration or state (including a failed
`--check`), 3 no token, 9 partial poll (the token was rejected, the intent is
disabled or a channel failed; its cursor stays put and the next poll resumes).
`--check` follows the [prerequisite declaration](src/mash_discord/prerequisites.toml).
It makes no network requests, so Discord's acceptance of the token, the intent
and channel permissions stay `unverified` until `sync`.

## Capture behavior

- `channels` is an allowlist of channel and thread IDs; an empty list selects
  nothing. A thread is a separate chat: selecting a channel never selects its
  threads. A thread's container carries its `parent_channel_id`, and every
  container its `guild_id`, as metadata. Forum channels hold no messages
  themselves; select their posts (threads).
- Each poll reads each channel's messages after a cursor kept in `state_dir`:
  the ID of the newest message delivered. Message IDs are snowflakes, ordered
  by creation time, so `history_days` is a snowflake floor and filters by
  creation time. Narrowing the window or the allowlist never deletes earlier
  history; changing destinations or the window restarts from the window start.
- **Edits are not tracked.** A message is read once, when it is new; an edit
  made after that is not seen (`updated_at` records edits made before). Edits
  need gateway events or periodic rescans; neither is implemented.
- **Deletions are not observed.** Polling cannot see them, so nothing is ever
  deleted from Mash; deliveries are incomplete snapshots.
- Messages map to entries with the author's Discord user ID, display name and
  kind (`bot` for bots, webhooks and Discord's system user), creation and
  edit times and a `https://discord.com/channels/<guild>/<channel>/<message>`
  link. A reply in the same channel links to its target (`reply_to`); forwards
  and cross-channel references are `native_reference` metadata. Attachments are
  kept as names and URLs only and are never downloaded; Discord's CDN URLs
  expire, so the message link is the durable reference.
- Joins, pins, boosts, thread-creation notices and other system messages are
  skipped, as are messages with neither text nor attachments (stickers, or
  embeds alone; embeds are not ingested).
- hikari follows Discord's per-route rate-limit buckets and the global limit,
  waits out a 429's `retry_after` up to 60 seconds (longer waits fail that
  channel for this poll), and retries 5xx responses and connection errors
  three times with backoff. hikari does not limit how often it retries a 429,
  so each request, retries included, gets 180 seconds; a channel that keeps
  being rate limited is reported for this poll. Other failures are reported
  per channel.

## Verify

```sh
uv run --project connectors/discord --locked ruff check connectors/discord
uv run --project connectors/discord --locked ruff format --check connectors/discord
uv run --project connectors/discord --locked ty check connectors/discord
MASH_CONNECTOR_MASH_BINARY=/absolute/path/to/mash \
  uv run --project connectors/discord --locked python -m unittest discover \
  -s connectors/discord/tests
```

Tests point hikari at an in-memory Discord API on a local HTTP server; the
integration test delivers into a disposable Mash database and finds channel
and thread messages with `mash search`/`view`.
