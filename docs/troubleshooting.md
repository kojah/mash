# Troubleshooting

Start with the check that matches the problem:

```sh
mash status                  # index and search readiness
mash status --installation   # history locations and agent connections
mash status --shared         # the index shared with your other machines
```

`mash status --installation` works even when the index is not ready. Errors are
JSON; find yours below by its `error.kind` or by a phrase from `error.message`.

## `mash: command not found`

The installer does not edit your shell profile. Add the install directory to
`PATH`, then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"   # release installer
export PATH="$HOME/.cargo/bin:$PATH"   # cargo install
```

Put the line in your shell profile to keep it. For PowerShell and custom
install directories, see [install the CLI](installation.md#install-the-cli).

## Qdrant connection failed

**You see:** `qdrant-unavailable`, “Could not reach Qdrant at …”, or
`mash status` reporting `local_qdrant` as `unreachable` with
`recommended_action` `start-qdrant`.

Indexing and every search need the local Qdrant search service. Start it the
way you installed it:

- **`mash qdrant install`:** run it again. It restarts a stopped service and
  repairs missing files. `local_qdrant.installed` in `mash status` shows the
  service's state and where its output is, such as
  `journalctl --user --unit mash-qdrant.service` on Linux.
- **Docker:** start the existing container and read its logs.

  ```sh
  docker start mash-local-qdrant
  docker logs mash-local-qdrant
  ```

- **Your own Qdrant:** restart it from its original folder and read its
  terminal output.

If Qdrant was never set up, follow
[installation](installation.md#start-the-local-search-service). It is ready
when [the readiness check](http://127.0.0.1:6333/readyz) reports
“all shards are ready.”

`local_qdrant` `rejected` means something answered but refused the request or
is not a supported version; the status gives the reason. The HTTP and gRPC
endpoints must point to the same Qdrant 1.20+ instance. See
[custom endpoints](configuration.md#local-qdrant).

`qdrant-failure` means Qdrant answered with an error. Read its output (above)
for storage, credential or schema errors.

Without Qdrant you can still list chats, read messages and export a backup.
`mash index` saves your history before it needs Qdrant: it exits with
`qdrant-unavailable` and says how many chats it saved, and `mash list-chats`
and `mash view` read them. `mash init` changes nothing when Qdrant is down; to
read chats first, run `mash init --no-index`, then `mash index`. Once Qdrant
runs, run `mash index` again to make search current.

On native Windows, a Qdrant log mentioning `gridstore`, “path specified,” and
`os error 3` can mean its data path is too long. Use the short absolute paths in
the [native Windows setup](installation.md#native-windows-without-docker).
For an existing installation, stop Qdrant before moving its data and keep a
backup. Do not delete storage as a first step. Changing only the configured
path starts a separate empty store; it does not move your data.

## Nothing is indexed yet

**You see:** `database-missing`.

| Message | Fix |
| --- | --- |
| “Mash is not set up on this machine” | `mash init` |
| “Mash is configured, but nothing is indexed yet” | `mash index` |

Both need Qdrant running to build search.

## My agent cannot find Mash

`mash init` connects only Claude Code and Codex, and only when their CLI
(`claude`, `codex`) is on `PATH`. Check each agent's `connection`:

```sh
mash status --installation
```

| `connection` | Meaning | Fix |
| --- | --- | --- |
| `none` | The agent cannot reach Mash. | Run the report's `next_action`, usually `mash init`. If the agent's CLI is missing, install it first. |
| `mcp-only` | The Mash tools work; only the plugin is missing. | Run the report's `next_action`. Plugin installation needs network access, and its hooks need Python 3.9+. |
| `full` | Connected. | Restart the agent. |

Restart the agent after setup, a repair, or a binary or plugin upgrade; an open
chat keeps its old connection. To test a fresh connection:

```sh
mash status --installation --agent claude-code --probe
```

A successful probe checks a new MCP process, not a chat that is already open.
See [agent setup](agents.md).

## Search returns no results

An empty result does not prove the conversation never happened. Work through
these:

1. Try one distinctive word with `--intent exact-mention`. A multiword query
   matches a phrase; those words may not appear together in that order.
2. Remove filters, then add them back one at a time. Project paths and repository
   URLs must exactly match recorded metadata.
3. Run `mash index` for new conversations, or `mash index --all-history` for
   chats older than the default 90 days.
4. Check `mash status --installation` for missing history locations or failed
   scans.

Search covers user and assistant messages only. Tool output and reasoning are
kept for reading: search for a nearby message, then open the transcript.

To find a chat by title instead, which needs no models:

```sh
mash list-chats --title-contains 'part of the title'
```

Read it from its first message with `mash view` and the row's
`lookup.view.id`, or search inside its chat ID.

## Models are missing

**You see:** “semantic models are not installed” (`model`), or
“related-discussion needs semantic search, which this configuration turns off”
(`usage`).

Search by meaning is optional and needs downloaded models. Word and phrase
search works without them:

```sh
mash search 'auth_token' --intent exact-mention
```

To search by meaning, follow
[model setup](indexing.md#enable-semantic-search): download the models, turn
semantic search on in your configuration, then index. Only
`mash models install` downloads models, and installing them does not change
the configuration by itself. Edit the existing settings; do not replace a
configuration that has custom history or sharing settings.

## New chats are missing or search is behind

**You see:** recent conversations missing, or `search-not-ready`.

Mash reads new history only when it indexes:

```sh
mash index
```

- “search is catching up with new history”: an index pass is running. Retry in
  a moment.
- “search is behind the stored history and nothing is refreshing it”: run
  `mash index`.

The agent plugin starts a catch-up index when a session begins, at most once
every ten minutes. For continuous updates, see
[keep history up to date](synchronization.md).

## A chat still shows what an older release read

Mash skips a chat whose files have not changed since it last read them. When
an upgrade changes how an agent's history is read, for example which folder a
chat is filed under, chats indexed before the upgrade keep the old result
until they change. To apply it to everything now:

```sh
mash index --reread
```

Chats that come out the same keep their search data, so this is quicker than a
first index. Add `--provider codex` to limit it to one agent.

## Search views are missing or stale

If the database is compatible and its stored history is ready, rebuild the
local search data from it:

```sh
mash index --rebuild-local-views --full
```

This does not read agent history files, fix incomplete source coverage, or
migrate schemas. For new or previously failed sources, run `mash index` and
check its final result.

## The index schema is older or newer than Mash

**You see:** `schema-incompatible`.

Stop and follow [upgrades and backups](upgrades.md). Keep the old executable
and data. Export with the release that wrote the index, and validate an import
with the new release at a separate path.

Do not delete the database, change schema numbers, or expect `index --full` to
migrate it. If an upgrade marker is present, recover or finish that upgrade
before creating a new index.

## Indexing is busy, incomplete, or shared-pending

- **Busy** (`index-busy`, “another Mash operation is updating this local
  index”): another writer is active. Let it finish, or stop your
  `mash index --watch` process, then retry. Do not delete lock files.
- **Incomplete:** read the reported source error, correct it, and run
  `mash index` again. History already stored is kept.
- **Shared-pending:** the local index may have succeeded. Check connectivity,
  the token configuration and `mash status --shared`, then run `mash index`
  again.

A search across machines can return local results with a `shared_error`. Use
`--local` for a local-only search, or repair the shared connection before
relying on results from all machines.
