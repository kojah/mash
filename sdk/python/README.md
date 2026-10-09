# Experimental Python connector SDK

The runnable portion currently provides Rust-schema-generated Pydantic models,
typed mutations, and deterministic, schema-checked batch encoding. Delivery,
durable pending batches, debounce, source adapters, and server ingestion remain
under development. This package is not published.

```python
from mash_sdk import Changes, Chat, Document, Message, encode_batch

changes = Changes.upsert(
    Chat(id="channel-1", title="Engineering"),
    Message(id="message-1", chat_id="channel-1", content="Use Qdrant."),
    Document(id="note-1", title="Architecture", content="Silver is canonical."),
)
batch = changes.to_batch(source_id="my-source", batch_id="stable-batch-1", expected_revision=0)
payload = encode_batch(batch)
```

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
the test fails if the specified binary is missing. Offline tests still run
without that environment variable, but do not claim cross-language validation.

The bundled schema checks structural rules and SDK encoding checks budgets and
duplicate mutations. Rust remains authoritative for its additional validation
and final stored relationships. No receipt or once-only application is implied
by successful local encoding.
