# Database migration policy and implementation plan

Decision: prefer upgrading existing SQLite databases over routine export/import
and full search rebuilds. Use the same migration definitions for local silver
and each node's shared silver database. Export/import remains for portability,
recovery and legacy transitions that do not have a tested migration.

`app/storage/schema.rs` supports upgrades from versions 62 through 65 to 66.
These are the supported older starting versions. The same initializer creates
and upgrades local and shared silver. Read-only commands do not migrate.

## Library evaluation

The selected [rusqlite_migration](https://docs.rs/rusqlite_migration/2.6.0/rusqlite_migration/)
runner executes SQL migrations atomically and uses SQLite `user_version` rather
than a migration-history table. Its 2.6.0 manifest depends on Rusqlite 0.40.0,
compatible with Mash's 0.40.2 requirement. Cargo resolves 2.6.0 without changing
Rusqlite. No migration-history database is needed for this additive baseline.

Its version is the count of applied migrations. Mash's value 62 is a historical
schema number, not evidence that 62 library migrations ran. The library warns
against independently changing `user_version`. The runner reserves the first 62
entries with empty steps and rejects all versions 1 through 61 before running.
Step 63 creates the record tables. Step 64 adds the retained evidence purpose
and source role; older evidence keeps these fields unknown rather than inferring
historical intent. Step 65 adds owner-local extraction batch receipts. Step 66
retires open questions: it deletes records of that kind and the links that
resolved them, and adds triggers that refuse the retired kind and relation,
because SQLite cannot change the checks on those columns in place. It is the
first step that removes rows. Fresh
creation directly creates the same schema and sets version 66 in its creation
transaction. Do not reset a
live database to zero, silently renumber it, or let placeholder migrations make
unsupported old schemas appear valid. Fresh creation and upgraded databases must
produce the same schema. Preserve monotonic versions so old binaries continue
to reject databases written by newer ones.

[Refinery](https://github.com/rust-db/refinery) is the alternative to evaluate if
stored migration history and checks for edited migrations justify the extra
machinery. Compare its Rusqlite integration and version tracking before choosing;
do not add both libraries. Keep released migrations immutable whichever runner
is selected.

## Supported starting versions

Start by validating schema 62 as a baseline for future migrations. Support for
54 through 61 needs a separate audit and historical fixtures; it is not implied
by accepting this policy. The 54 to 62 history includes connector removal,
replacement of the generic storage model, derived turns, local Qdrant adoption
and chat terminology changes. This is more than renaming one table.

For each proposed starting version, record the exact input schema, required
data transformations, expected retained fields and a recovery procedure. Test
native message IDs, ancestry, content, attachments, privacy decisions, source
coverage, publication identity/revisions, tombstones and shared transfer state.
Resolve retention of removed connector data explicitly; do not silently discard
it or reintroduce connectors into the product. Leave unverified versions
unsupported and untouched.

## Writer coordination and failure tests

Migration must acquire the same exclusive writer admission used by ordinary
writes before inspecting the version, then recheck the version under that lock.
All processes that can write the same database must participate. A SQLite
transaction alone does not keep an already-running old MCP, watcher or server
from attempting later writes: review their schema and runtime freshness checks.
Do not migrate from a read-only query or bypass a live rollout recovery marker.

Keep SQL changes and the version update in one transaction where SQLite allows
it. For steps outside a transaction, specify interruption and recovery behavior
before implementation. Use existing blocking workers for SQLite work.

Required evidence includes failure halfway through a migration, process
termination and reopen, concurrent upgrade attempts, stale writers, repeat opens,
newer-schema rejection without writes, and preservation of existing history.
Run the same tests against local and per-node shared databases. Use independent
expected records and bounded property tests, not only schema snapshots. Update
the relevant protocol models for lock and durable-write ordering; distinguish
process-crash recovery from power-loss durability.

## Qdrant changes are independent

A SQLite version change alone must not trigger Qdrant rebuilds or republishing.
Preserve search data when message identities, searchable input and retrieval
semantics remain valid. Use targeted payload updates, filter indexes and
[supported collection settings changes](https://qdrant.tech/documentation/manage-data/collections/).
Some settings may cause Qdrant to rebuild its internal indexes; that does not
mean Mash must regenerate embeddings or republish all history.

Recompute vectors only when the embedding model or relevant input changes.
Account separately for dense vectors, the lexical text index, payload fields,
filter indexes and freshness certification. Never certify stale results as
current merely because the SQLite migration succeeded. Search must remain
replayable from canonical silver after a failure.

Start without a separate Qdrant migration framework. Building a replacement
collection while the old one serves searches is a possible later option, not
an implementation requirement. Qdrant's
[model migration guide](https://qdrant.tech/documentation/tutorials-operations/embedding-model-migration/)
describes options for model changes; it does not make collection replacement
necessary for every SQLite change.

## Deployment transition

Before changing an active rollout, inspect installed binaries, retained backups,
service state and recovery markers. Do not replace a partially executed recovery
plan with an untested migration. Versions 62 through 65 upgrade in place to 66;
the deployment checks accept this interval without retiring shared history.
Both binaries report `silver_schema_min` and `silver_schema` for installers.
Unsupported older schemas still require a separately verified export/import or
retirement procedure. Process interruption tests cover both an uncommitted
migration and termination immediately after commit; they do not establish
power-loss durability. After candidate activation, deployment failures require
forward recovery because the candidate may already have migrated SQLite.
