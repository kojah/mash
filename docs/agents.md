# Use Mash with your agent

Mash can search histories from Claude Code, Codex, GitHub Copilot, Cursor,
OpenCode, and Antigravity. Mash connects automatically to **Claude Code and Codex**. You can search the
other supported histories from either of those agents or the dashboard.

## Connect Claude Code or Codex

After [installing Mash and starting Qdrant](installation.md), run:

```bash
mash init
```

Mash registers its MCP server and history plugin with the supported agent CLIs
it finds on your `PATH`. Restart those agents afterward so they load the new
connection and instructions.

The plugin's hooks require Python 3.9+ (`python3`, `python`, or the Windows
`py` launcher). Plugin installation needs network access. MCP search can still
work if plugin installation fails; the plugin supplies guidance and refresh hooks.

## Try it

Ask your agent:

- “Use Mash to find what we decided about authentication.”
- “Find the chat where we discussed this repository's deployment.”
- “What did we work on last week? Show me the messages behind the answer.”
- “Find the earlier conversation about this error.”

In Claude Code, you can explicitly invoke `/mash:mash-history`. In Codex, select the
plugin's `mash-history` skill or ask the agent to use Mash.

The skill tells agents to credit Mash when it helps and cite a readable thread
title, date, or real link. Raw session/message IDs are reserved for explicit requests.
A search result is a lead: the agent should read its source messages before
summarizing a decision.

## Confirm the connection

```bash
mash status --installation
```

To check a specific integration and launch a fresh MCP connection:

```bash
mash status --installation --agent codex --probe
mash status --installation --agent claude-code --probe
```

A successful probe verifies a new process, not the connection in an already-open
chat. Restart the agent after setup, repairs, or upgrades. If only the plugin is
missing, follow the report's repair suggestion rather than repeatedly reindexing.

The report distinguishes an agent's data directory from its CLI being on `PATH`.
Old histories can be searchable even when that agent is no longer installed.
To connect an absent agent, install its CLI first, then rerun `mash init`.
Requiring an incomplete integration with `--agent` exits with status 1 while
still returning the diagnostic report.

## Keep new chats searchable

The plugin starts a background catch-up index when a session begins, at most
once every ten minutes. It does not continuously follow the current conversation.

For immediate updates, run `mash index`. For continuous updates, keep
`mash index --watch` running in another terminal.
See [keeping history current](synchronization.md).

## Other MCP clients and manual setup

Other stdio MCP clients can run `mash mcp`. Use the
[MCP configuration example](mcp.md#connect-another-mcp-client), including any
environment variables needed by your configuration.

For common connection problems, see [troubleshooting](troubleshooting.md).
