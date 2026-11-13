# Troubleshooting

Start with the command that matches the problem:

```sh
mash status
mash status --installation
```

The first checks index/search readiness. The second checks provider locations and
agent setup, even when the index is not ready. Use `mash status --shared` when
the problem concerns another machine.

## Search returns no results

1. Try one distinctive word with `--intent exact-mention`. A multiword query
   matches a phrase; those words may not appear together.
2. Remove filters, then add them back one at a time. Project paths and repository
   URLs must exactly match recorded metadata.
3. Run `mash index` for new conversations, or `mash index --all-history` for
   older sources outside the default 90-day horizon.
4. Check `mash status --installation` for missing provider roots or failed scans.

`mash list-chats --title-contains 'part of the title'` can find a chat without
using search models. If it appears there, read it from its first message with
`mash view` and the row's `lookup.view.id`, or search inside its returned chat ID.

Tool output and reasoning are retained for reading but excluded from dialogue
search. Search for a nearby user/assistant message, then open the transcript.
An empty result does not prove the conversation never happened.

## Qdrant connection failed

`mash status` reports `local_qdrant`: `reachable`, `unreachable` (nothing
answered at the configured URL) or `rejected` (it answered but refused the
request or is not a supported version, with the reason). While Qdrant is
unreachable, `recommended_action` is `start-qdrant`.

For a native install, restart Qdrant from its original folder and inspect its
terminal output. If you chose Docker, start the existing container and inspect
its logs:

```sh
docker start mash-local-qdrant
docker logs mash-local-qdrant
```

If it has not been created, follow [installation](installation.md#start-the-local-search-service).
Wait for [the readiness check](http://127.0.0.1:6333/readyz) to report
“all shards are ready.”
The HTTP and gRPC endpoints must point to the same Qdrant 1.20+ instance.
See [custom endpoints](configuration.md#local-qdrant).

You can still list chats, read messages, and export a backup while the search
service is unavailable. `mash index` saves your history before it needs Qdrant:
when Qdrant does not answer, it exits with `qdrant-unavailable` and says how many
chats it saved, and `mash list-chats` and `mash view` read them. Start Qdrant,
then run `mash index` again to make search current.

On native Windows, a Qdrant log mentioning `gridstore`, “path specified,” and
`os error 3` can mean its data path is too long. Use the short absolute paths in
the [native Windows setup](installation.md#native-windows-without-docker).
For an existing installation, stop Qdrant before moving its data and keep a
backup; do not delete storage as a first troubleshooting step. Merely changing
the configured path starts a separate empty store rather than moving your data.

## Models are missing

For word/phrase search, use `--intent exact-mention`; it does not require models.
For semantic search, follow [model setup](indexing.md#enable-semantic-search).
Only `mash models install` downloads models.

Installing models alone does not change an existing lexical-only configuration.
Follow the configuration steps in that guide, then index. Preserve custom provider and shared
settings instead of replacing your entire configuration.

## A chat still shows what an older release read

Mash skips a chat whose files have not changed since it last read them. When
an upgrade changes how an agent's history is read, for example which folder a
chat is filed under, chats indexed before the upgrade keep the old result
until they change. To apply it to everything now:

```sh
mash index --reread
```

This reads every chat again and updates the ones that come out differently.
Chats that come out the same keep their search data, so it is quicker than a
first index. Add `--provider codex` to limit it to one agent.

## Search views are missing or stale

If the current database is compatible and its retained history is ready, rebuild
the derived local search data:

```sh
mash index --rebuild-local-views --full
```

This uses stored history. It does not scan provider files, fix incomplete source
coverage, or migrate schemas. For new or previously failed sources, use ordinary
`mash index` and inspect the final result.

## The index schema is older or newer than Mash

Stop here and follow [upgrades and backups](upgrades.md). Keep the old executable
and data. Export with the release that wrote the index, and validate an import
with the new release at a separate path.

Do not delete the database, change schema numbers, or expect `index --full` to
migrate it. If an upgrade marker is present, recover or finish that upgrade
before creating a new index.

## My agent cannot find Mash

Run `mash status --installation` and follow its repair suggestion. Restart the
agent after setup or a binary/plugin upgrade. A successful `--probe` tests a new
MCP process; an existing chat may still hold an old connection.

If an agent's `connection` is `mcp-only`, its MCP tools work and only the
plugin is missing.
Plugin installation requires network access; hooks also need Python 3.9+.
See [agent setup](agents.md).

## Indexing is busy, incomplete, or shared-pending

- **Busy:** another writer may be active. Let it finish or stop your watcher
  before retrying. Do not delete lock files.
- **Incomplete:** inspect the reported provider/source error, correct it, and
  run `mash index` again. Valid retained history is preserved.
- **Shared-pending:** local work may have succeeded. Check connectivity, token
  configuration, and `mash status --shared`, then retry indexing.

A combined search can return local results with a `shared_error`. Use
`--local` if you want an explicitly local search, or repair the shared connection
before relying on results across all machines.
