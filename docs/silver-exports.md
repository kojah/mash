# Local silver exports

Exports work offline and do not load models or contact the shared server.
Create one per node; this is not an export of the entire shared node group.

```sh
mash export /path/to/new-export
mash import /path/to/new-export --into /path/to/new-index.sqlite3
mash --db /path/to/new-index.sqlite3 index --rebuild-local-gold
```

`export` uses the configured database (or global `--db`). `import --into` is
required and does not change the configured database. Point subsequent commands
or an MCP session at the imported path explicitly. Both operations refuse
existing destinations; import also refuses existing SQLite sidecars and
attachment directories. Import does not merge histories or publish to shared
storage. Reconfigure the node's shared destination before explicitly publishing.

The same application core is exposed as MCP `export_silver(directory)` and
`import_silver(directory, database)`. Importing does not redirect an existing
MCP session to the new database.

## Searching an export on another machine

Copy the complete export directory to the receiving machine. Import it into a
new database, then project stored silver into the receiving user's chosen gold
configuration. The gold rebuild does not discover or read provider histories;
the original source paths are retained as provenance, not required input files.
The original machine and its provider applications need not be available.

For a standalone imported archive, use a separate TOML configuration:

```toml
version = 1
providers = {}
```

```sh
mash --config archive.toml import ./export --into ./archive.sqlite3
mash --config archive.toml --db ./archive.sqlite3 index --rebuild-local-gold
mash --config archive.toml --db ./archive.sqlite3 search 'your query' --local
mash --config archive.toml --db ./archive.sqlite3 mcp
```

The empty provider collection keeps ordinary indexing/watching from mixing in
the receiving machine's chats. Gold settings may be added to this configuration;
default semantic search requires locally installed models, or configured remote
embeddings. Install models explicitly with `models install` if needed.

When using MCP, start the session with the imported database, then call `index`
with `rebuild_local_gold=true` and `search` with `local=true`. No provider ingestion
is involved. Importing into or merging with an existing local history database
is intentionally outside this first version.

## Format version 1

An export is a directory:

```text
manifest.json
silver.sqlite3
attachments/<BLAKE3 digest>
```

The typed JSON manifest records format/schema versions, creation time, optional
configured source node, silver completeness/revision, counts, database byte size
and BLAKE3 checksum. This is archive metadata, not node configuration. The
database contains canonical messages, interpreted dialogue, context links,
threads, enrichment, provider-native Claude branch evidence, source identities,
checkpoints, scan/rejection state, tombstones and pending shared deletion intent.
Attachment files are deduplicated by digest and verified against retained
metadata. Unretained or external attachments remain metadata only; exports do
not read their provider paths. Missing or corrupt *retained* files fail export
creation instead of silently losing data.

Silver tables are copied in one SQLite transaction, including committed WAL
data, while other connections may continue indexing. Gold tables exist as empty
schema infrastructure; embedding, FTS, semantic-chunk, readiness and cache data
are not copied. Models, node/server configuration, environment credentials and
raw provider files are not included. Copying is streamed through SQLite; attachment copies and
checksumming use bounded buffers. The format is uncompressed and can be archived
with standard tools after completion.

The manifest is installed last as the completion marker. An interrupted export
without it cannot import; remove that incomplete directory before retrying.
Import checks the database checksum, integrity, relationships, versions, silver
certificate/counts and each retained attachment. It reconstructs the trusted
Mash schema and installs a new database only after the files pass validation.
It resets shared publication identity/acknowledgements and leaves search
explicitly unready. Complete silver supports an offline gold rebuild; pending,
incomplete or unverified silver retains that state and needs explicit ingestion
repair. A failed import never overwrites an existing database. An interrupted
import may leave an attachment directory, which must be inspected/removed before
retrying with that target.

Use a Mash build supporting the archive's SQLite schema and format version;
unsupported versions fail closed. Schema migration of archives, group exports,
automatic scheduling and human-readable JSONL exports are separate work.

Exports preserve canonical private history rather than redacted search output.
They can contain sensitive text and binary attachments. On Unix, directories
are created with mode 0700 and files with mode 0600; Windows inherits the parent
directory's access controls. Checksums detect corruption, not malicious
replacement. Encryption and trusted storage are handled outside this format.
