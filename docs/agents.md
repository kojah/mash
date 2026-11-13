# Use Mash with your agent

Once connected, Claude Code and Codex can search your past chats themselves
when you ask about earlier work. Mash connects to **Claude Code and Codex**;
from either one you can search the history of every supported agent (Claude
Code, Codex, GitHub Copilot, Cursor, OpenCode, and Antigravity). For any other
agent, see [other MCP clients](#other-mcp-clients-and-manual-setup).

## Connect Claude Code or Codex

After [installing Mash and starting Qdrant](installation.md), run:

```bash
mash init
```

Then restart Claude Code or Codex so it loads the connection, and
[try it](#try-it).

For each of the two agent CLIs found on your `PATH`, `mash init` installs two
things, using the agent's own commands:

- an **MCP server**: the Mash search tools the agent can call (MCP, the Model
  Context Protocol, is how agents run external tools);
- the **Mash plugin**: a `mash-history` skill that tells the agent when and
  how to search, and hooks that [keep new chats searchable](#keep-new-chats-searchable).

Those commands edit the agent's user settings: `~/.claude.json`,
`~/.claude/settings.json` and `~/.claude/plugins` for Claude Code, and
`~/.codex/config.toml` and `~/.codex/plugins` for Codex. Mash also records
the connected agents in its own `config.toml`, so that
[`mash status --installation`](#confirm-the-connection) can report one that
later loses its connection.

Requirements for the plugin:

- Installing it needs network access.
- Its hooks need Python 3.9+ (`python3`, `python`, or the Windows `py`
  launcher).

If the plugin fails to install, the agent can still search through the MCP
server when you ask it to; it lacks only the skill and the hooks.

Running `mash init` again repairs a registration that points at a missing or
different Mash executable.

## Try it

Ask your agent:

- “Use Mash to find what we decided about authentication.”
- “Find the chat where we discussed this repository's deployment.”
- “What did we work on last week? Show me the messages behind the answer.”
- “Find the earlier conversation about this error.”

The agent searches, reads the messages behind the results, and answers with
the chat's title, date, or a link as its source. It gives raw session and
message IDs only if you ask for them.

To invoke the skill explicitly, use `/mash:mash-history` in Claude Code. In
Codex, select the plugin's `mash-history` skill or ask the agent to use Mash.

## Confirm the connection

```bash
mash status --installation
```

In the JSON report, `summary.clients` lists each agent with a `connection`:

| `connection` | Meaning |
| --- | --- |
| `full` | Nothing needs attention. |
| `mcp-only` | The Mash tools work, but the plugin (skill and hooks) is missing. |
| `none` | The agent cannot reach Mash. |

When something needs repair, `next_action` gives the exact command. Run it
rather than reindexing; indexing does not fix a connection.

The command exits with status 1, still printing the report, when an agent
needs attention and is one that `mash init` connected, one configured for
Mash, or one named with `--agent`. Agents you never connected are fine to
leave unused.

To check one agent and also launch its MCP server in a fresh process:

```bash
mash status --installation --agent codex --probe
mash status --installation --agent claude-code --probe
```

A successful probe verifies a new process, not the connection in an
already-open chat. Restart the agent after setup, repairs, or upgrades.

The report distinguishes an agent's data directory from its CLI being on
`PATH`: old histories stay searchable after you uninstall the agent. To
connect an agent whose CLI is absent, install the CLI, then rerun `mash init`.

## Keep new chats searchable

The plugin's hook indexes new history in the background when an agent session
starts, at most once every ten minutes. It does not follow the current
conversation as it happens. Set `MASH_AUTO_INDEX=off` in the agent's
environment to turn it off.

To index now, run `mash index`. To index continuously, keep
`mash index --watch` running in another terminal.
See [keeping history current](synchronization.md).

## Disconnect an agent

Run the agent's own removal commands (`mash init` lists them as `undo`):

```bash
claude mcp remove --scope user mash
claude plugin uninstall mash@mash
codex mcp remove mash
codex plugin remove mash@mash
```

## Other MCP clients and manual setup

Any other client that supports stdio MCP servers can run `mash mcp`. Use the
[MCP configuration example](mcp.md#connect-another-mcp-client), including any
environment variables your configuration needs.

For common connection problems, see [troubleshooting](troubleshooting.md).
