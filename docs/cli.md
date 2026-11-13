# Useful commands

Find a conversation, then read the message behind a result:

```sh
mash search 'a word you remember'
mash view MESSAGE_ID --context 3
```

Each result carries a `lookup.view.id`; pass it to `view` as `MESSAGE_ID`.
`list-chats` rows carry one too. `view` does not accept chat IDs.

Every command prints JSON. To read conversations directly, use the
[dashboard](dashboard.md) or a [connected agent](agents.md).

## Commands

| Command | What it does | Most useful options |
| --- | --- | --- |
| `mash qdrant install` | Downloads, verifies and starts the local search service. Rerun it to repair or upgrade; its data is kept. | |
| `mash init` | Sets up Mash: writes configuration for the histories it finds, connects Claude Code and Codex, and indexes. | `--yes` accepts the defaults without prompts. `--no-index` configures without the search service running. `--no-agents` leaves agent settings unchanged. `--force` replaces an existing configuration, kept as `config.toml.previous`. |
| `mash search 'QUERY'` | Searches indexed chats or messages. | `--intent exact-mention` or `--intent related-discussion`, `--unit messages`, `--days N`, `--provider PROVIDER`, `--limit N`. See [search filters](search.md). |
| `mash list-chats` | Lists chats, most recently active first. | `--title-contains 'part of the title'`, `--provider PROVIDER`, `--limit N`. |
| `mash view MESSAGE_ID` | Returns one message. | `--context N` adds up to N messages before and after. |
| `mash dash` | Serves the [dashboard](dashboard.md) at `http://127.0.0.1:8790`. | `--listen ADDRESS`. `--services-config PATH` enables [service management](services.md). |
| `mash index` | Reads new or changed history into the index. See [keep history up to date](synchronization.md). | `--watch` keeps refreshing until interrupted. `--all-history` or `--since-days DAYS` includes older history. `--provider PROVIDER` limits the run to one agent. `--reread` reads every chat again after an upgrade that reads an agent's history differently. `--full` recreates all derived search data. |
| `mash status` | Reports whether search is ready, including whether the local search service answers. | `--installation` checks agent connections and the search service without opening the index. `--shared` shows each machine's freshness in the shared index. |
| `mash models install` | Downloads the optional models for [semantic search](indexing.md#enable-semantic-search), about 177 MB. | |
| `mash forget CHAT_ID` | [Removes one chat](privacy.md#remove-a-chat) from the index and does not import it again. | |
| `mash export DIRECTORY` | Writes a [portable backup](upgrades.md) of your indexed history and retained attachments to a new directory. Keep it private. | |
| `mash import DIRECTORY --into NEW_DATABASE` | Restores a backup into a new database. | `--keep-identity` is for [upgrades](upgrades.md) only. |
| `mash mcp` | Serves Mash's tools to an agent over stdio. Agents start it themselves; see [connect another MCP client](mcp.md). | |
| `mash qdrant uninstall` | Stops and removes the search service that `mash qdrant install` set up. | `--delete-storage` also deletes its data, which `mash index` rebuilds. |

Every command also accepts `--config PATH`, `--db PATH` and `--models-dir PATH`
for files outside their default locations.

`mash init` uses an interactive wizard when stdin and stderr are terminals;
its prompts go to stderr and its final result remains JSON on stdout.

For sharing between machines, see [multi-machine setup](shared-index.md).

## Read the built-in help

Run `mash --help` or `mash COMMAND --help` for every option. Help and version
output are JSON too. To display the help text with line breaks:

```sh
mash --help | python3 -c 'import json,sys; print(json.load(sys.stdin)["help"])'
```

In PowerShell, use `(mash --help | ConvertFrom-Json).help`.
