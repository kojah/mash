# Mash connector SDK (experimental)

Bring other sources into Mash, such as chat channels, issues or notes, so they are
searchable beside your agent histories. A connector describes what the source
holds as typed objects. Mash stores, indexes and searches them. The SDK handles
delivery, retries and keeping Mash in step with the source. This package is not
published.

## Quick start

```python
import asyncio
from pathlib import Path
from mash_sdk import Changes, Connector, Destination, Object, Role

changes = Changes.upsert(
    Object(id="channel-1", role=Role.container, kind="slack:channel", title="Engineering"),
    Object(id="message-1", role=Role.entry, kind="slack:message",
           parent="channel-1", content="Use Qdrant."),
    Object(id="note-1", role=Role.document, kind="notes:note",
           title="Architecture", content="Silver is canonical."),
)

async def main() -> None:
    async with Connector(
        source_id="my-connector",
        destinations=[Destination.local(database=Path("mash.sqlite3"))],
        state_path=Path("connector-state.sqlite"),
    ) as connector:
        await connector.send(changes)

asyncio.run(main())
```

[`examples/markdown_folder.py`](examples/markdown_folder.py) is a complete,
runnable connector: it turns a folder of Markdown files into searchable notes
and keeps them in sync.

[The Obsidian connector](../../connectors/blueprints/obsidian/README.md) adds vault folder
selection, frontmatter, stable rename identities and confirmed deletions. It
uses this SDK as a separate application and never writes to the vault.

## Connector setup contract

A runnable connector should ship package dependencies, an example TOML configuration,
and a versioned `prerequisites.toml` declaration. Each requirement has an `id`,
`when` (`always`, `local` or `shared`), a description and a remedy. Source adapters
declare their required API scopes, filesystem access and platform dependencies;
Mash destination credentials are separate from source credentials. Secrets remain
in named environment variables. `mash_sdk.config.DestinationConfig` validates the
`[[destinations]]` tables; `relative_to` resolves paths and `destination()` builds
the `Destination`.

Its read-only `--check` reports `passed`, `failed` or `unverified` per requirement,
without ingestion, installing dependencies, creating state or exposing secrets.
Presence is distinct from verified authorization. Check results include `ready`
(no failed requirements) and `fully_verified` (all applicable requirements passed).

`mash_sdk.prerequisites` implements this. `Manifest.load(package)` reads the
declaration; the connector checks only its source requirements and passes their
statuses to `check`, which adds Python and, per destination, the offline `mash`
and `database` checks (local) or `credentials` and `authority` (shared, where
authority is always `unverified`). The manifest declares those IDs in its own
words. `emit` prints the report as one JSON line and returns exit code 2 unless
it is ready. See [Obsidian's](../../connectors/blueprints/obsidian/src/mash_obsidian/prerequisites.py)
source checks. This is a connector packaging convention; the SDK does not
discover, install or run connector packages inside Mash.

## Objects

Every object has an `id` (unique within its source), a `role` and a `kind`:

- `container`: something that holds a conversation, such as a channel, an issue
  or a pull request. Search groups results by their nearest container.
- `segment`: a part of a container, such as a review or a thread section.
- `entry`: one message or comment, under a container or segment (`parent`).
- `document`: a standalone record, such as a note or a page.

The `role` tells Mash how to treat an object. The `kind` is your own label, such as
`slack:message`, which results show and searches can filter on (`--kind`). Mash's
native histories use the same model: `codex:chat`, `codex:turn` and
`codex:message`.

A source can declare its topology so Mash enforces it:

```python
await connector.send(Changes.declare(
    KindRule(kind="discord:channel", role=Role.container, root=True),
    KindRule(kind="discord:thread", role=Role.container, parents=["discord:message"]),
    KindRule(kind="discord:message", role=Role.entry,
             parents=["discord:channel", "discord:thread"]),
))
```

After that, Mash rejects an object with an undeclared kind, a different role or a
disallowed parent as a permanent delivery error. It also rejects a new
declaration that stored objects would break.

## Keeping a source in sync

**Polled sources.** Describe what the source holds now as snapshots and call
`sync` on your own schedule. Unchanged objects are not resent. A `complete`
snapshot deletes what it no longer contains:

```python
while True:
    await connector.sync(DocumentSnapshot(documents=notes(), scope_ids=["folder"], complete=True))
    await asyncio.sleep(60)
```

A `Snapshot` holds one container and the objects below it. A `DocumentSnapshot`
holds documents within named scopes. A failed `sync` raises `DeliveryError` with
the work saved, and the next call resumes it.

**Event-driven sources.** `Connector.run` backfills snapshots, then keeps
refreshing as events arrive, until the event stream ends:

```python
await connector.run(
    backfill=snapshots(),        # AsyncIterable[Snapshot | DocumentSnapshot]
    events=source.events(),      # AsyncIterable[Delivery[Event]]
    event_type=Event,            # decodes journaled events after a restart
    refresh=fetch_current,       # Callable[[Event], Awaitable[Changes]]
    debounce=Debounce(quiet_seconds=2, max_wait_seconds=10),
)
```

- Event capture starts before backfill, so changes made during backfill are
  not missed. Each event is journaled in the state file, encoded with
  `event_type` through Pydantic, before `Delivery.acknowledge` is awaited.
  Events with the same `key` coalesce and the latest value wins.
- Refreshes start after backfill finishes, so an older snapshot never
  overwrites the current state a refresh fetched. A key is refreshed once it
  has been quiet for `quiet_seconds`, or `max_wait_seconds` after its first
  event. Journaled events left by an earlier run are refreshed on restart.
- Retryable delivery failures back off, up to 60 seconds between attempts.
  Permanent failures and refresh errors end the run, and the event stays
  journaled.

### What omission deletes

Only `complete` snapshots delete by omission. A `Snapshot` deletes omitted
entries whose nearest container is its container (entries under its segments
included) created at or after `created_since`. Nested containers, such as reply
threads, are their own scopes. Entries with unknown creation time, and targets
that a snapshot entry still replies to, are kept. A `DocumentSnapshot` deletes
omitted documents that a complete snapshot delivered from scopes it now fully
covers. Narrowing a scope never deletes what it excludes. Documents sent only
through events or `send` are never deleted by omission.

## Destinations

A connector delivers to each destination independently: local silver, a node's
shared silver, or both. Native history reaches shared silver through node
publication, but connector records never do. A connector that should be
searchable in both places lists both destinations. `send` returns one receipt
per destination, in order. Every destination is attempted even when another
fails. The error reports how many failed, and it is retryable only if every
failure is.

`Destination.local` runs `mash --db PATH index --connector-batch FILE`, which
commits the batch to local silver without loading models. Its records become
searchable after the next `mash index` (or `mash index --watch`), and until then
local search reports views as stale rather than omitting them.
`Destination.shared` posts the same batch to the shared service, which commits
it to that node's shared silver and indexes it in Qdrant. It takes the same
settings as Mash's `[shared]` configuration:

```python
Destination.shared(
    api_url="https://mash.example",
    group_id="personal",
    node_id="laptop",
    token_env="MASH_API_TOKEN",
)
```

The token is read from the environment at each delivery and never stored. The
node comes from the token. The group and node are sent too, so the server
rejects a token issued for another node (HTTP 403) instead of committing under
it. Redirects are not followed. Unreachable servers, timeouts, HTTP 408, 429 and
5xx are retryable. Other statuses need intervention. Ordering and checkpoints
belong to the receiving node, so changing the token or `token_env` keeps saved
progress, while each node has its own.

## How delivery works

You can skip this section to write a connector; it explains the guarantees.

Connector authors pass typed changes. They do not allocate batch IDs, track
revisions, interpret receipts or touch the state file. The client saves pending
work before delivery, keeps its exact identity across restarts, and moves on only
after a matching acknowledgement from durable silver. A failed or malformed
response leaves the work pending. Mash commits the mutations, the source revision
and the receipt in one transaction. Replaying a batch returns its original
receipt, and reusing a batch identity for different contents is rejected.

Opening the client resumes saved delivery automatically. Inside an open client,
resending identical pending changes reuses their batch identity. Different
changes are sent after pending work is acknowledged. `DeliveryError` reports
whether a failure may be retried, and leaves the batch saved. Cancelling a caller
does not interrupt accepted delivery, and closing waits for outstanding work.
This is once-only application per batch identity, not a promise that every
upstream event is captured exactly once.

The state file also keeps a ledger of what each destination acknowledged,
updated in the same transaction as the receipt. That ledger is how unchanged
objects are skipped and omissions become deletes. Its format is private and
versioned. A file from an earlier SDK version is rejected untouched; remove it to
start over, and the next sync resends what it cannot prove was delivered. Work is
split under the 512-mutation and encoded-size batch limits.

## Development

Wire classes are generated from `app/connectors.rs`; do not edit `_generated`.
The SDK's `Object` aliases the generated upsert record. Its `role` decides which
fields apply, and Rust enforces those rules when it ingests a batch. Opaque
metadata keeps JSON types rather than using `Any`.

Run `make generate-python-models` to emit Rust JSON Schema and regenerate models
with the locked Python generator. `make check-python-models` regenerates and
checks for drift. Checked-in generated files let SDK users install without Cargo.
The development generator uses Cranelift; on Xenia, run these build targets
through `agent-test` with the normal development target directory. CI regenerates
the models, rejects drift, and runs SDK tests against the Rust decoder. It uses
`MASH_SCHEMA_CARGO_RUN=run` to reuse its LLVM build artifacts.

```sh
uv run --locked ruff check src tests examples
uv run --locked ruff format --check src tests examples
uv run --locked ty check src tests examples
uv run --locked python -m unittest discover -s tests
```

For cross-language checks, set `MASH_CONNECTOR_CONTRACT_BINARY` to the built
`connector-schema` example. Its `validate` mode runs the real Rust decoder; its
development-only `apply --database PATH` mode also exercises the real silver
transaction and receipt replay. The test fails if the specified binary is
missing. Set `MASH_CONNECTOR_MASH_BINARY` to the built `mash` binary to run the
client tests and the Markdown example through the real public CLI. CI supplies
both. Offline tests still run without these variables, but skipped tests do not
prove delivery conformance. Shared-delivery and runner tests use an in-process
server (`tests/fake_mash.py`) that reproduces the ingress contract. The Rust
server tests in `app/shared/server/tests/connectors.rs` own the server side, which
needs Qdrant to run as `mash serve`.

The bundled schema checks structural rules, and SDK encoding checks budgets and
duplicate mutations. Rust remains authoritative for further validation and the
final stored relationships. Successful local encoding implies no receipt or
once-only application.
