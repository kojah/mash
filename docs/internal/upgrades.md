# Upgrades and backups

[Documentation](../README.md) · [Project home](../../README.md)

The [migration policy and design](database-migrations.md) prefers upgrading
existing SQLite databases and preserving valid Qdrant search data independently.
Schema 62 upgrades in place to 63 on a writable open under writer admission.
Read-only opens never migrate. Earlier and newer unsupported versions are
rejected untouched. The procedures below describe the export/import fallback for
unsupported older schemas, not a requirement for every schema change.

## Make a portable backup

Export to a new directory:

```sh
mash export /absolute/path/to/new-backup
```

This saves canonical chats, messages, retained attachments, and a checksummed
manifest. It excludes models, provider files, deployment credentials, and
derived Qdrant views. Keep your TOML configuration and credentials separately.

**Treat exports as sensitive:** they preserve canonical history and can contain
credentials that ordinary search output masks. Store them privately.

## Restore to a separate database

Use a compatible release and a new destination:

```sh
mash import /absolute/path/to/new-backup --into /absolute/path/to/restored.sqlite3
mash --db /absolute/path/to/restored.sqlite3 index --rebuild-local-views --full
mash --db /absolute/path/to/restored.sqlite3 search 'a word you remember' --local
```

Keep Qdrant running for the rebuild and search. Use `--config PATH` and
`--models-dir PATH` too if this backup needs settings or models outside the
defaults. The import does not replace or select your normal database; continue
passing `--db` while inspecting it.

Ordinary imports get a new publication identity. The upgrade-specific
`--keep-identity` option is for replacing a retired publisher, not running a
second copy alongside it.

## Before replacing a release

1. Check the installed and target releases with `mash --version`; each reports
   its `silver_schema`.
2. Preserve the installed executable, configuration, canonical SQLite database,
   and adjacent `.attachments` directory. Stop writers before making file-level
   backups; copying a live SQLite main file alone can miss committed WAL data.
3. Export using the release that wrote the index. Exports contain sensitive
   canonical history and retained attachments. They omit deployment credentials,
   provider files, models, and derived search indexes.
4. Verify that the target release accepts the exported archive **at a separate
   staging path** before retiring anything live.

Live database schema compatibility and portable archive compatibility are
different checks. The current archive uses envelope version 2 and record schema
2; older record formats are rejected, not translated. If the importer rejects
an archive, preserve the original data and executable and prepare a verified
conversion or recovery plan. Changing the manifest version alone is unsafe.

## Compatible export and import

With writers stopped, use the old and new executables explicitly:

```bash
/path/to/old/mash --db /path/to/mash.sqlite3 export /path/to/new-backup
/path/to/new/mash import /path/to/new-backup \
  --into /path/to/staged/mash.sqlite3 --keep-identity
```

`--keep-identity` continues the exporting node’s publication identity. Retire
the original database before activating the replacement; never run two
publishers with that same identity. Normal imports without this option create
a new publication identity.

For compatible archives, [upgrade-local-index.sh](../../tools/upgrade-local-index.sh)
exports, stages and validates an import before retiring the old files, and
retains recovery artifacts. It refuses a database with open users and leaves an
upgrade marker if interrupted. Investigate that marker and its backup pointer
before retrying. [upgrade-remote-index.sh](../../tools/upgrade-remote-index.sh)
applies the same workflow to a remote node.

## Rebuild and reconnect

Imports restore canonical history, with search views unready. At the replacement
database’s final path, rebuild its local Qdrant views explicitly:

```bash
mash --db /path/to/mash.sqlite3 index --rebuild-local-views --full
mash --db /path/to/mash.sqlite3 status
```

When the release changes provider interpretation, also run an explicit history
refresh using the configured source files. Rebuilding search views alone does
not repair source coverage. Models are downloaded only by `mash models install`.

Restart existing MCP connections and watchers after replacing the executable;
they otherwise retain the old release. Refresh the installed history plugin
when its instructions change. Check local and shared readiness separately.

## Shared deployments

Clients and server must run matching releases. With the current legacy retirement
procedure, shared silver is republished by its owning nodes and Qdrant views are
rebuilt from that history. A future supported SQLite migration should preserve
shared history and valid search data instead. An empty shared
silver directory cannot be recovered by `--replay-views` alone.

Treat shared silver, transfer receipts/catalog, upload spool, attachments,
configuration, and release identity as a coordinated backup. Preserve every
publisher’s authoritative local history and identity. Fence writes for cutover,
and verify that all configured nodes have republished before declaring recovery
complete. Do not retire only the node databases while retaining receipts that
could acknowledge history no longer present.

Current synchronization uses protocol 3 and catalog schema 1. Older unversioned
catalogs are rejected untouched. Local export/import does not preserve the
server’s transfer catalog or shared publication head and is not a complete
shared-server backup.

Use the [fleet deployment workflow](../../deployment/README.md) for managed
installations. A failed compatibility preflight requires a concrete migration
plan; it is not a reason to disable the guard. Once a new service acknowledges
writes, restoring an older backup can lose those writes—recovery must account
for them.
