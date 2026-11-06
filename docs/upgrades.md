# Back up, restore, and upgrade

## Make a portable backup

Export to a new directory:

```sh
mash export /absolute/path/to/new-backup
```

This saves your indexed conversations and retained attachments. Keep your
configuration and credentials separately; models and the agents' original
history files are not included.

**Keep backups private.** They can contain sensitive text and credentials,
even when those credentials are masked in search results.

## Restore without replacing your current history

Use a compatible Mash release and a new destination:

```sh
mash import /absolute/path/to/new-backup --into /absolute/path/to/restored.sqlite3
mash --db /absolute/path/to/restored.sqlite3 index --rebuild-local-views --full
mash --db /absolute/path/to/restored.sqlite3 search 'a word you remember' --local
```

Keep the local search service running. The restored copy is separate from your
usual index; keep passing `--db` to use it. If you use a custom configuration
or model directory, also pass `--config PATH` or `--models-dir PATH`.

## Upgrade an existing installation

Back up before replacing Mash, and keep the old executable until you have
checked that the new version can read your history.

If Mash reports an incompatible index, ordinary indexing cannot upgrade it.
Export with the version that wrote the index, then test an import with the new
version at a separate path:

```sh
/path/to/old/mash export /absolute/path/to/new-backup
/path/to/new/mash import /absolute/path/to/new-backup --into /absolute/path/to/staged.sqlite3 --keep-identity
```

Use `--db PATH` with the old executable if your index is outside its default
location. Some older backups are incompatible too: if import fails, keep the
original data and executable. Do not delete the index or edit version numbers.

The `--keep-identity` option is for replacing an installation. Stop the old
watcher and agent connections before switching to the replacement; do not
publish from both copies at once. At the replacement's final path, run:

```sh
mash --db /absolute/path/to/replacement.sqlite3 index --rebuild-local-views --full
mash --db /absolute/path/to/replacement.sqlite3 status
```

Continue passing `--db` when the replacement is outside the default location,
including in your agent's MCP setup. Restart your agents and watchers after upgrading.

If you share history, upgrade the server and machines together. A local export
is not a complete shared-server backup; have the server administrator preserve
the existing installation before changing it.
