# MCP and attachments

[Documentation](../README.md) · [Project home](../../README.md)

Run `mash mcp` as a stdio MCP server. It accepts `--config`, `--db`, and
`--models-dir` at startup and exposes `search`, `list_chats`, `view`, `status`, `index`, `forget`,
`models_install`, `list_attachments`, `get_attachment`, `export_silver`, and `import_silver`. The server uses the same operations and typed JSON errors
as the CLI; tool failures set MCP `isError`.

All MCP tools present readable text alongside the
unchanged typed `structuredContent` JSON. Text includes copyable lookup arguments,
source attribution and partial errors; archived excerpts are quoted as evidence.
Clients may expose both channels, so this does not guarantee token savings.
`view` includes complete archived message bodies. Mutation tools
summarize their results and retry/next-action information; attachment retrieval
also retains native MCP image/resource content. Tool errors use readable text
and preserve typed error JSON and `isError`.
`search`, `list_chats`, and `status` accept `detail: "compact"` (default)
or `"extended"`. Compact search bounds message/excerpt text to 600 characters
(plus truncation markers), removes per-stage ranking scores and enrichment
provenance, and preserves result ordering, IDs, publishing node attribution,
copy anchors, pagination, partial-source errors and explicitly requested
diagnostics. Truncated text is marked with `content_truncated` or
`snippet_truncated`; call `view` to read complete messages. Extended returns the
existing full response, including retrieved message text, scores, provenance
and status manifests/configuration. `view` always returns complete message
text and accepts no detail selector. Extended text adds the full JSON response
after its readable summary. CLI output stays extended JSON.

`export_silver({"directory":"/path/to/new-export"})` saves local silver and
retained attachments offline, with a typed JSON manifest and checksums.
`import_silver({"directory":"/path/to/new-export","database":"/path/to/new.sqlite3"})`
imports to a fresh path and leaves views explicitly unready. It does not redirect
the running MCP session or overwrite existing history. See [silver exports](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-exports.md)
for format, CLI commands, import/rebuild steps and data scope.

Attachment discovery uses a message ID from `view` or search:
`list_attachments({"message_id":"…"})`, then
`get_attachment({"message_id":"…","attachment_id":"…"})`.
For shared evidence, pass `shared` with its publishing node ID to both tools.
Retrieval returns metadata plus native MCP image content for PNG/JPEG/GIF/WebP,
or a base64 embedded resource for other files. Local `view` also includes attachment
metadata. No models, URL downloads, OCR, or arbitrary file reads are involved.

The initial adapters capture Claude image/document blocks, Codex raw and completed
[image inputs](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/user_input.rs),
and OpenCode file parts. Other providers' attachment formats are not
yet captured. Run `mash index` to refresh compatible history; incompatible database schemas
require the [upgrade procedure](../upgrades.md). Attachment names/markers stay out of semantic dialogue projections.
Up to 32 attachments per message and 4 MiB per retained file are supported.

Embedded base64 files are retained automatically. Explicit local file references
require allowed roots in node TOML; missing or disallowed files retain metadata:

```toml
[attachments]
allowed_roots = ["/absolute/path/to/provider/attachments"]
share_files = false
```

Changing allowed roots triggers replay during the next explicit index operation.
Local copies live beside SQLite in `<database-path>.attachments`, keyed by content
hash, and work offline even after the provider file disappears. Back up this
directory with SQLite. UTF-8 files are retained and returned as credential-redacted
views; binary images/PDFs are not sanitized. Attachment bytes do not enter Qdrant.

For optional shared files, set `share_files = true` on the node and configure an
absolute `attachments_dir` in server TOML. Nodes upload only files linked to their
published messages, after canonical publication; failed uploads leave indexing's
shared result pending for retry. Metadata without uploaded bytes reports
`not_uploaded`. Storage is isolated by group/node, and every read checks current
published message membership. Removed attachments become inaccessible; orphaned
content-addressed files currently remain on disk until the cache is removed.

To stream an `index` tool call's progress, include a string or numeric
`params._meta.progressToken` in the `tools/call` request. The server sends
standard `notifications/progress` with the same token while the job runs,
including when it waits for another mutation. `message` contains the phase,
counts, elapsed time, and phase ETA. Structured progress fields are also available
under `params._meta["dev.jamesqo.mash/index-progress"]` in each notification.
The numeric `progress` is an increasing update counter across all phases;
`total` is omitted because the whole job's work is unknown. Phase-specific
totals and `eta_seconds` are in the structured metadata. Unknown ETAs are `null`.

Progress delivery uses a bounded queue and skips intermediate updates if a
client is slow; a disconnected client does not block indexing. The normal
tool result follows progress and retains the CLI's success/error structure.
Without a progress token, the tool returns only its final result. CLI progress
still goes to stderr; MCP stdout remains exclusively SDK protocol messages.

## Connect another MCP client

Example client configuration (the client's format is JSON; Mash's own configuration remains TOML):

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.toml"]
    }
  }
}
```

For shared operations, provide the configured node token environment variable to
`mash mcp` through your MCP client's environment mechanism. `search` accepts a
`shared` boolean, a `unit` of `chats` (default) or `messages`, and `limit`/`offset` paging with `next_offset` in the response; `view` accepts a `shared` publishing node ID. Search, view, and
status and `list_chats` are annotated read-only. Index and forget mutate data; `models_install`
is the only operation that downloads models. Shared node I/O uses async HTTP;
local and service-side Qdrant I/O uses async gRPC for publication and HTTP for reads/setup. SQLite, parsing, and local FastEmbed inference run on blocking
workers and are awaited by callers. MCP mutations serialize per session; status
and view may run concurrently, while model-heavy operations use a single permit
to bound memory and CPU use. CLI and MCP use the same installed local models.
