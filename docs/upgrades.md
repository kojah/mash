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

Mash's upgrade policy is to preserve existing databases through supported
migrations and keep valid search data. The current schema-62 binary does not
implement those migrations yet; the instructions below are its fallback for
incompatible databases. Do not assume an older database can already be upgraded
in place, or change its version number by hand.

Back up before replacing Mash, and keep the old executable until you have
checked that the new version can read your history.

If your configuration contains `views.query.experimental_gpu_reranking`, remove
that setting. Experimental GPU reranking has been removed; contextual reranking
uses CPU. This configuration change does not require reindexing or new models.

Remote embedding APIs have also been removed. Remove the `[embeddings]` table
and `redaction.remote_embeddings` from node and server configurations. Mash
rejects these settings rather than silently changing your embedding model.
If you previously used remote embeddings, run `mash models install` to install
the local embedding model and rebuild local views with
`mash index --rebuild-local-views --full`. Existing local-model installations
and their vectors remain valid.

For a shared index that used a different embedding space, coordinate the upgrade
across its server and nodes. Install models on the server, then explicitly
activate and replay its views with `mash-server --server-config PATH
--models-dir PATH --activate-views --replay-views`. This rebuilds search data
from retained shared history; it does not replace silver. Keep a backup and
allow for search downtime during replay.

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
