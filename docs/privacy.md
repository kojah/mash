# Your history and privacy

## What stays on your machine

The default setup indexes and searches locally without changing your agents'
original history files. The public documentation website receives none of
your indexed chats.

Sharing across machines uploads history to your configured Mash server.
Remote embedding services, if you choose to use one, receive text from your
history and your search queries.

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

Find its `chat_id` with `mash list-chats` or search, then run:

```sh
mash forget CHAT_ID
```

Mash removes the local chat and remembers not to import it again. With sharing
enabled, it also attempts to publish the removal; check the command's result
for any pending work.

This does not delete the agent's original files, earlier backups, or copies
independently indexed on another machine. There is no undo command. Make
[a backup](upgrades.md) first if you may need the chat later.
