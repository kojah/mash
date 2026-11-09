# Privacy and stored history

[Documentation](../README.md) · [Project home](../../README.md)

## What stays on your machine

A local-only setup reads your agent history without modifying the original files.
Optional shared indexing publishes history to your configured server.
Embedding inference defaults to local execution. Explicit remote batch inference
sends prepared searchable text to the configured authenticated worker. Query
embeddings and reranking remain local. Remote inference grants no database access.
The public Mash documentation website receives none of your indexed chats.

The dashboard serves readable history to anyone who can reach its listening
address. Its default is loopback. Portable backups preserve sensitive canonical
history; credential masking in search results does not make an export safe to share.

## Remove a chat from Mash

Find its `chat_id` with `mash list-chats` or search, then:

```sh
mash forget CHAT_ID
```

This removes the local chat and records a tombstone so later indexing does not
reimport it. If sharing is configured, Mash also attempts to publish the removal;
check the result for pending shared work. It does not delete the provider's
original files, older backups, or copies independently indexed by another node.
There is no CLI undo command. If you may need the chat later, export a backup first.

## Credential masking

Mash masks recognizable credentials in CLI/MCP search results, chat metadata,
and `view` output, shared HTTP read responses and outgoing publications. Full values use `[REDACTED_SECRET]`; excerpts use
masking characters to retain their source offsets. Original provider histories
and local canonical SQLite data remain intact. Detection is best effort, not a
guarantee that arbitrary or obfuscated secrets are absent.

Text a person wraps in `<private>…</private>` (any case, closed in the same
message) is replaced by `[PRIVATE]` before it is stored: in native message
text, search projections, tool inputs and chat titles. Every index, view, export
and publication derives from silver, so none of them holds it, whatever the
`[redaction]` settings. An
unclosed tag masks nothing. Mash cannot change the provider's own history files,
and retained text attachments are stored as submitted.

Masking is on by default and can be turned off per destination in the node
configuration:

```toml
[redaction]
output = true             # CLI/MCP search, view and chat listing
publication = true        # history uploaded to the shared server
```

Turning `output` off shows canonical text, and lets search match inside
credentials. Changing `publication` republishes this node's whole history on
its next index, because masked and unmasked uploads must not mix. The shared
server has its own `[redaction]` table with `responses` for HTTP read responses.
Text attachments are masked when they are retained, so they stay masked regardless of these
settings. Existing deployments
and stored shared history require an upgrade and explicit re-indexing; rebuilding
alone does not delete previous publications. See the
[redaction evaluation and limits](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/redaction-evaluation.md).
