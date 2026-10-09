# Experimental Python connector SDK

The runnable portion provides Rust-schema-generated Pydantic models, typed
mutations, and a connector client with durable local or shared delivery and
restart recovery. The event runner, debounce, source adapters and gold
projection remain under development. This package is not published.

Mash now accepts a typed batch file through
`mash --db PATH index --connector-batch BATCH_FILE`. This local entry point uses
the shared application core and returns a durable silver receipt. It does not
load models, publish to the shared service, or certify unbuilt gold. The SDK's
local destination hides this command and temporary-file handling.

The shared service accepts the same batch at `POST /v1/connector-batches` and
commits it with its receipt in that node's shared silver. The node comes from
the bearer token, never from the batch. Shared connector input is not
searchable yet: until gold projection exists, the node reports gold as not
current and group searches fail closed unless they exclude that node.
`Destination.shared` takes the same `api_url`, `group_id`, `node_id` and
`token_env` as Mash's `[shared]` configuration:

```python
Destination.shared(
    api_url="https://mash.example",
    group_id="personal",
    node_id="laptop",
    token_env="MASH_API_TOKEN",
)
```

The token is read from the environment at each delivery and never stored. The
group and node are sent so the server rejects a token issued for another node
(HTTP 403) instead of committing under it. Redirects are not followed.
Unreachable servers, timeouts, HTTP 408, 429 and 5xx are retryable. Other
statuses need intervention. Ordering and checkpoints belong to the receiving
node, so changing the token or `token_env` keeps saved progress, while each
node has its own.

```python
from pathlib import Path
from mash_sdk import Changes, Chat, Connector, Destination, Document, Message

changes = Changes.upsert(
    Chat(id="channel-1", title="Engineering"),
    Message(id="message-1", chat_id="channel-1", content="Use Qdrant."),
    Document(id="note-1", title="Architecture", content="Silver is canonical."),
)

async def publish() -> None:
    async with Connector(
        source_id="my-connector",
        destination=Destination.local(database=Path("mash.sqlite3")),
        state_path=Path("connector-state.sqlite"),
    ) as connector:
        await connector.send(changes)
```

Connector authors pass typed changes. They do not allocate batch IDs, track
revisions, interpret receipts, or manipulate the delivery state file. The client
persists pending work
before delivery, preserves its exact identity across restarts, and advances only
on a matching durable-silver acknowledgement. A failed or malformed response
leaves the work pending. Rust's canonical silver store now commits mutations,
source revision and receipt in one transaction. Replaying the same batch returns
its original receipt; reusing a batch identity for different contents is rejected.
Opening the client resumes saved delivery automatically. Inside an open client,
resending identical pending changes reuses their batch identity; different changes
are sent after pending work is acknowledged. `DeliveryError` reports whether a
failure may be retried and leaves the batch saved. Cancellation of a caller does
not interrupt accepted delivery, and closing waits for outstanding work.
This is once-only application per batch identity, not a promise that arbitrary
upstream events are captured exactly once. Source capture and reconciliation are
still part of the forthcoming event runner.

Wire classes are generated from `app/connectors.rs`; do not edit `_generated`.
The SDK's `Chat`, `Message`, and `Document` names alias the corresponding typed
upsert records, whose operation tags have generated defaults. Opaque metadata
keeps JSON types rather than using `Any`.

Run `make generate-python-models` to emit Rust JSON Schema and regenerate models
using the locked Python generator. `make check-python-models` regenerates and
checks for drift. Checked-in generated files let SDK users install without Cargo.
The development generator uses Cranelift; on Xenia run these build targets through
`agent-test` with the normal development target directory.
CI regenerates the models, rejects drift, and runs SDK tests against the Rust
decoder. It uses `MASH_SCHEMA_CARGO_RUN=run` to reuse its LLVM build artifacts.

```sh
uv run --locked ruff check src tests
uv run --locked ruff format --check src tests
uv run --locked ty check src tests
uv run --locked python -m unittest discover -s tests
```

For cross-language checks, set `MASH_CONNECTOR_CONTRACT_BINARY` to the built
`connector-schema` example. Its `validate` mode invokes the real Rust decoder;
its development-only `apply --database PATH` mode also exercises the real silver
transaction and receipt replay. The test fails if the specified binary is missing.
Set `MASH_CONNECTOR_MASH_BINARY` to the built `mash` binary for high-level client
tests through the real public CLI. CI supplies both. Offline tests still run
without these variables, but skipped tests do not prove delivery conformance.
Shared-delivery tests use an in-process server that reproduces the ingress
contract; the Rust server tests in `app/shared/server/tests/connectors.rs` own
the server side, which needs Qdrant to run as `mash serve`.

The bundled schema checks structural rules and SDK encoding checks budgets and
duplicate mutations. Rust remains authoritative for its additional validation
and final stored relationships. No receipt or once-only application is implied
by successful local encoding.
