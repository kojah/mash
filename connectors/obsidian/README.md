# Obsidian → Mash

A runnable, read-only vault connector. Notes become standalone `obsidian:note`
documents delivered through the existing [Python SDK](../../sdk/python/README.md).
It needs the vault folder and a configured Mash destination; local delivery uses
an installed Mash binary. No Obsidian API key or running Obsidian desktop
application is needed. Linux and macOS are supported; Windows
users can run it in WSL with the vault available there.

## Configure and run

Install the locked environment from the repository:

```sh
uv sync --project connectors/obsidian --locked
```

Copy [example.toml](example.toml) outside Git, then set the vault path, stable
vault ID, selected folders, state directory and destination. Paths are relative
to the configuration file, with `~` expansion. Keep connector state outside the
vault, on persistent storage; it contains sensitive pending deliveries. A vault
ID and its state directory must stay paired across restarts and vault moves.
Changing the vault ID creates a new independent Mash source.

```sh
# Read-only setup report: no ingestion, state creation or model downloads.
uv run --project connectors/obsidian --locked mash-obsidian \
  --config ~/.config/mash/obsidian.toml --check

# One scan and delivery; it does not ingest native provider histories.
uv run --project connectors/obsidian --locked mash-obsidian \
  --config ~/.config/mash/obsidian.toml

# Poll within this foreground process; Ctrl-C stops it. No daemon is installed.
uv run --project connectors/obsidian --locked mash-obsidian \
  --config ~/.config/mash/obsidian.toml --watch

# Delivery commits local silver without loading or downloading models.
# Explicitly update local search views after delivery:
mash index --rebuild-local-views
mash search 'your note text' --source obsidian:my-vault --local
```

Use the same `--config`, `--db` and `--models-dir` options as your normal Mash
installation for the final two commands. Search does not ingest or repair views.
The connector emits JSON counts and diagnostics, not note contents. A one-shot
partial scan exits with code 9 and retains previously ingested history; invalid
configuration/state exits with code 2, and delivery failures with code 1. During
watching, retryable deliveries resume with bounded backoff, up to 60 seconds.

The package ships a versioned [prerequisite declaration](src/mash_obsidian/prerequisites.toml).
`--check` reports each applicable requirement, its status and a remedy. It checks
Python, vault/folder access, state storage, Mash availability and local schema
compatibility, plus token presence for shared delivery. Exit code 2 means a
requirement failed. `ready` means the local checks passed; `fully_verified` also
requires every applicable requirement to be verified. Shared network access and
authorization are explicitly `unverified`: this offline check makes no network
requests, and token presence alone does not establish permission.

Local and shared destinations are independent and explicit. Native node
publication never carries connector records, so add the example's shared
destination only if you want these notes on that server. Keep its token in the
named environment variable. The local-only configuration uploads no notes.

## Capture behavior

- `folders` is an explicit vault-relative allowlist. `.` selects the vault root;
  recursion is controlled by `recursive`. Empty selections and traversal are
  rejected. `.obsidian`, `.trash` and `.git` are excluded. Symlinks and nonregular
  files are not read, and a failed enumeration cannot confirm any deletion.
- Complete original UTF-8 Markdown is preserved. Titles come from `title`
  frontmatter, the first H1, or the filename. Frontmatter properties and tags are
  metadata. Inline tags in code spans/fences are excluded.
- Only a timezone-aware `created` frontmatter timestamp establishes creation
  time. Missing, date-only and timezone-less values remain unknown; modification
  time is never substituted. `history_days` excludes unknown creation dates and
  older notes. Omit it to ingest all selected notes. Narrowing folders or history
  never automatically purges excluded history.
- Stable identities live in a separate SQLite registry. Ordinary inode-preserving
  renames, same-path atomic saves, and unique unchanged-content moves preserve
  IDs. A copy stays a distinct note while the original exists. Ambiguous moves,
  including a rename combined with atomic replacement and edits between scans,
  cannot be identified reliably from polling and may receive new IDs.
  Notes/frontmatter are never rewritten to insert IDs.
- A successful scan first observes a missing note; another successful scan after
  `deletion_grace_seconds` confirms its deletion. Transient save gaps, inaccessible
  folders, malformed notes and failed reads cannot silently remove history.
  Registry rows become inactive only after every destination acknowledges delete.
- Wiki embeds and Markdown image references are retained as metadata with a
  resolved vault-relative path when directly resolvable. Local attachment bytes
  are not copied: the current SDK transports attachment references, not files.
  References are not fetched, and paths outside the vault are never read.

Obsidian Sync remains separate. This connector reads the local vault as it is;
it does not log in, synchronize remote notes, or modify your Sync configuration.

## Verify

```sh
uv run --project connectors/obsidian --locked ruff check connectors/obsidian
uv run --project connectors/obsidian --locked ruff format --check connectors/obsidian
uv run --project connectors/obsidian --locked ty check connectors/obsidian
MASH_CONNECTOR_MASH_BINARY=/absolute/path/to/mash \
  uv run --project connectors/obsidian --locked python -m unittest discover \
  -s connectors/obsidian/tests
```

Integration tests use disposable vaults and databases. They verify actual Mash
silver, lexical search and full note evidence, rename/edit identity, confirmed
deletion, replay after a lost acknowledgement, and foreground polling. They do
not modify your real vault or require model downloads.
