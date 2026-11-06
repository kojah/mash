# CLI reference

[Documentation](README.md) · [Project home](../README.md)

```text
mash init [--force] [--no-agents] [--no-index] [--marketplace SOURCE]
mash index [--full|--watch [--debounce-ms N]] [--provider claude-code|codex|opencode|copilot|cursor|antigravity]... [--since-days N|--all-history]
mash index --rebuild-local-views [--full]
mash search <query> [--unit chats|messages] [--local | --shared] [--limit N] [--offset N] [--provider PROVIDER] [--days N]
mash list-chats [--local | --shared] [--provider PROVIDER] [--title-contains TEXT] [--after UNIX_SECONDS] [--before UNIX_SECONDS] [--sort updated-at|created-at|title] [--direction asc|desc] [--limit N] [--offset N]
mash view <message-id> [--shared NODE_ID] [--context N]
mash status [--shared]
mash forget <chat-id>
mash export <new-directory>
mash import <export-directory> --into <new-database> [--keep-identity]
mash models install
mash dash
mash mcp
mash serve --server-config PATH
```

Use `list-chats` for chat discovery and `view` for complete message text.

`mash dash` opens a local, read-only dashboard. Recent chats refresh every
30 seconds while the page is visible. An open transcript keeps its displayed
messages until you select **Show updated messages**; **Check for updates**
requests a fresh comparison. Navigating between chats retains the sidebar.
Direct chat links load messages independently of the catalogue.

Tool calls, tool output and reasoning appear in expandable sections. Tool
output is literal text, even when its recorded provider role is `user`.
Ordinary replies render sanitized Markdown; long replies expand without
cutting Markdown syntax. Catalogue titles use the first nonempty line, capped
at 240 characters; canonical history remains unchanged.

The transcript's **Export** menu downloads the entire chat as Markdown or
standalone HTML, including tool output. **Export to PDF** opens a printable
copy in a new tab; select **Save as PDF** in your browser's print dialog.
Exports read all pages from the selected machine, independently of the page
you are reading. You can cancel while they load. A failed read saves no
partial file. Browser exports are limited to 100,000 message positions and
50 million text characters; exceeding either limit reports an error.
These paged reads are not a database snapshot: avoid exporting during an
index update if you need a stable copy. The CLI `export` command remains the
portable canonical-history backup format. Treat downloaded transcripts as
sensitive chat history.

Search results, chat matches and listed chats identify their provider directly,
for example `"provider": "codex"`. Messages and matching excerpts carry the agent's
`role` directly, including provider-defined roles. Messages and matching excerpts
include `turn_id` when they belong to a derived turn; `chat_id` identifies their chat.
For storage compatibility and recovery, see [upgrades](../docs/upgrades.md).

One-shot data commands emit one JSON value; watch mode emits newline-delimited
results, and `mash mcp` serves the stdio protocol. `mash serve` runs the shared
index HTTP API; it reports its listening address as JSON on stderr and keeps
stdout empty. `mash --help` and subcommand `--help` return `{"help":"..."}`;
`mash --version` returns `{"version":"..."}`. These informational requests
exit successfully before loading configuration, models or indexes. Bare `mash`
returns a JSON usage error containing help with exit code 2. Global
options are `--config PATH`, `--db PATH`, and
`--models-dir PATH`.

CLI searches execute directly. MCP sessions reuse loaded models within their
process. Search never starts a background process.
