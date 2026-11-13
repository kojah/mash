# Back up, restore, and upgrade

An ordinary upgrade keeps your history and search data: install the new
release, run `mash index` once, and restart your agents. Back up first; the
backup is also how you move history into a release that cannot upgrade your
index in place.

## Upgrade an existing installation

1. **Back up**, and keep the old `mash` executable until step 6 succeeds:

   ```sh
   mash export /absolute/path/to/new-backup
   ```

2. **Stop everything that uses the old version**: `mash index --watch`
   processes and the agent sessions connected to Mash.

3. **Install the new release** the same way you
   [installed Mash](installation.md#install-the-cli).

4. **Update Qdrant if the release needs a newer one.** Rerun
   `mash qdrant install`; it upgrades the service Mash installed and leaves
   its data alone. If you run Qdrant yourself, upgrade it on the same storage.

5. **Upgrade the index:**

   ```sh
   mash index
   ```

   This is the step that converts the database. Search, `status` and other
   read-only commands never do, and report that an upgrade is required until
   `mash index` has run.

6. **Check, then restart** your agents and watchers:

   ```sh
   mash status
   mash search 'a word you remember' --local
   ```

The upgrade keeps your history, identities and existing search data. It does
not rebuild Qdrant collections or recompute embeddings.

**Saved open questions are deleted.** Upgrading an index written before open
questions were retired as a kind of saved record removes them, with their
revisions and the links that resolved them. Decisions, preferences and facts
are untouched. The export from step 1 still contains them.

**Which indexes can be upgraded.** `mash --version-info` on the new release prints
`silver_schema_min`, the oldest index format it upgrades in place; this
release accepts schema 62 and later. If `mash index` reports an older index,
it changes nothing: follow
[Replace an index that cannot be upgraded](#replace-an-index-that-cannot-be-upgraded).
If it reports a newer index, a newer Mash wrote it and an old executable or
agent session is still running: upgrade or restart that instead. Never edit
database version numbers or delete the index to get past either message.

### Upgrade a shared index

If you [share history across machines](shared-index.md), upgrade the server
and every computer to the same release together. Mismatched releases refuse to
synchronize; that never requires deleting or rebuilding search data.

On the server, upgrade Qdrant first, then start the new `mash-server`. It
upgrades each machine's database on the server as it opens it. `mash export`
on a computer backs up that computer only, so preserve the server's own data
before upgrading it.

## Make a portable backup

Export to a new directory:

```sh
mash export /absolute/path/to/new-backup
```

This saves your indexed conversations and retained attachments. It does not
include your configuration, credentials or models, or the agents' original
history files; keep copies of the configuration and credentials yourself.

**Keep backups private.** They contain the full text of your chats, including
any credentials that appeared in them, even though search results mask those.

## Restore without replacing your current history

Import into a new database file, rebuild its search data, and try a search.
Keep the local search service running:

```sh
mash import /absolute/path/to/new-backup --into /absolute/path/to/restored.sqlite3
mash --db /absolute/path/to/restored.sqlite3 index --rebuild-local-views --full
mash --db /absolute/path/to/restored.sqlite3 search 'a word you remember' --local
```

The restored copy is separate from your usual index: pass `--db` every time
you want to use it. Also pass `--config PATH` or `--models-dir PATH` if yours
are not in the default locations.

The backup must come from a release whose archive format this one reads. If
import fails, keep the backup and the release that wrote it.

## Replace an index that cannot be upgraded

Use this when Mash reports an unsupported older index. Ordinary indexing
cannot upgrade it; keep the old executable and the old index until the
replacement works.

1. Export with the version that wrote the index, then import with the new
   version at a separate path. Add `--db PATH` to the export if the old index
   is outside its default location:

   ```sh
   /path/to/old/mash export /absolute/path/to/new-backup
   /path/to/new/mash import /absolute/path/to/new-backup --into /absolute/path/to/staged.sqlite3 --keep-identity
   ```

   Some older backups cannot be imported either. If import fails, keep the
   original data and executable.

2. `--keep-identity` makes the replacement continue as the same machine on a
   shared index. Stop the old watcher and agent connections before switching,
   and never publish from both copies.

3. Move the staged database to its final path, then build its search data:

   ```sh
   mash --db /absolute/path/to/replacement.sqlite3 index --rebuild-local-views --full
   mash --db /absolute/path/to/replacement.sqlite3 status
   ```

4. If the replacement is not at the default location, keep passing `--db`,
   including in your agents' MCP setup. Restart your agents and watchers.

## Other changes you may meet

### Coming from Qdrant 1.19

Mash needs Qdrant 1.20 or later. Upgrade Qdrant before Mash: stop the 1.19
service, start 1.20 on the same storage, install the new Mash, and run
`mash index` once. That run adds a word-search index to your existing search
data without rebuilding it or recomputing embeddings; until then, search
reports that the views need upgrading. On a shared server, starting the new
`mash-server` adds the index.

**Storage opened by Qdrant 1.20 should not be opened by 1.19 again.** Back up
first if you may need to go back.

### Settings that are no longer accepted

- `views.query.experimental_gpu_reranking`: remove it. Reranking runs on the
  CPU. Nothing needs reindexing and no new models are needed.
- Old `[embeddings]` remote-provider settings: replace them with the
  configuration in
  [embedding inference](configuration.md#embedding-inference).
- `redaction.remote_embeddings`: remove it, from node and server
  configuration.

### Changing the embedding model

Vectors from the local model you already use stay valid across upgrades. If
you change to a different embedding model, run `mash models install`, then
rebuild local search data:

```sh
mash index --rebuild-local-views --full
```

On a shared index, change the server and all computers together. Install the
models on the server, then start it once with:

```sh
mash-server --server-config PATH --models-dir PATH --activate-views --replay-views
```

This rebuilds the server's search data from the history it already holds; the
history itself is not replaced. Back up first and expect shared search to be
unavailable during the rebuild.
