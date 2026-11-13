# Your history and privacy

## What stays on your machine

The default setup indexes and searches locally without changing your agents'
original history files. The public documentation website receives none of
your indexed chats.

An agent that keeps history in a SQLite database, such as OpenCode, may gain
`-wal` and `-shm` files beside that database when Mash reads it. SQLite
creates them for any reader; the database's contents are unchanged.

History leaves the machine only through features you turn on:

- **Sharing across machines** uploads history to your configured Mash server.
- **A remote embedding worker** receives prepared searchable text, which may
  contain sensitive history. Embedding inference runs locally by default, and
  query embeddings always stay local.
- **A remote reranking worker** receives each query and the excerpts being
  ranked. Reranking stays local unless you opt in.
- **The dashboard assistant** sends your questions, and the chat text and
  memories it reads to answer them, to the
  [model service you configure](configuration.md#the-dashboard-assistant).
  There is no assistant unless you configure one.

The local dashboard has no login. Keep its default address or restrict access
to a trusted private network.

## Sensitive information

Mash masks recognizable credentials in results and outgoing shared history by
default. Detection is best effort: review anything you intend to share.

Local history and portable backups can still contain credentials. Store
backups privately, just as you would the original conversations.

Text enclosed in `<private>…</private>` within a single message is omitted
from Mash's stored message text. Both tags must be present. This does not
remove text from the agent's own files or hide content inside attachments.

## Remove a chat

Find its `chat_id` with `mash list-chats` or `mash search`, then run:

```sh
mash forget CHAT_ID
```

Mash removes the local chat and remembers not to import it again. With sharing
enabled, it also attempts to publish the removal; check the command's result
for any pending work.

This does not delete the agent's original files, earlier backups, or copies
independently indexed on another machine. There is no undo command. Make
[a backup](upgrades.md) first if you may need the chat later.
