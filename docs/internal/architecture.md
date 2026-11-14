# Code organization

[Documentation](../README.md) · [Project home](../../README.md)

CLI and MCP are wrappers around the same typed application core in `app/core.rs`.
The core owns indexing, search, chat listing, view, status, forgetting, silver export/import, and model installation.
MCP calls it directly without constructing CLI arguments or invoking CLI handlers.
The watcher also uses typed core requests.

The shared HTTP service starts through `mash-server` and `cli/server_cli.rs`.
The `mash` executable owns local commands, stdio MCP and `mash dash`; it has no
`serve` subcommand. Both executables use the same library and release version,
including the shared storage and search implementation. This separates process
installation and arguments; it does not split the dependency graph into separately
compiled server and client libraries.

`app/core/models.rs` defines the in-memory requests and serialized responses.
Operation handlers live in `app/core/indexing.rs`, `search.rs`, `chats.rs`, `inspection.rs`
and `maintenance.rs`; each returns its concrete result type. The façade resolves
configuration and paths, dispatches the request and wraps the result once.
Indexing owns per-index provider routing, progress and watcher target selection;
inspection owns read-only status and canonical views. Parsing, SQLite and local
inference run on blocking workers, while shared HTTP operations remain asynchronous.

`crates/mash-search/src/config.rs` owns the typed node settings and status models. Its private
`document` module decodes strict TOML and validates provider routing, roots,
identifiers and endpoints; `loading` handles bounded file reads and platform
defaults. CLI, MCP and watcher operations use the same resolved settings.

`crates/mash-sync/src/shared.rs` is the typed shared-index facade. Its private `client` module
owns pooled HTTP requests, authentication, deadlines and response decoding;
`publication` owns durable acknowledgements, routing ownership and streaming
uploads; `retrieval` owns concurrent local/shared candidates, fusion and contextual
ranking. The HTTP service and concrete database backends remain separate from
those node-side modules. These boundaries use existing concrete types rather
than adding transport or provider abstractions.

`crates/mash-server/src/lib.rs` owns Axum routing, authorization, typed HTTP responses
and the listener lifecycle. Its private `configuration` module loads bounded
strict TOML and resolves authorized node identities; `store` initializes Qdrant; `upload` spools bounded
NDJSON streams. Configuration and credentials are validated before connecting
to storage, and backend initialization completes before the listener opens.



## Modules

CLI and MCP translate inputs into typed application-core requests. Implementation
modules are grouped by responsibility:

- `crates/mash-model/`: the vocabulary every layer shares (roles, providers,
  tool activity, attachments, credential masking, progress events, the typed
  error).
- `crates/mash-silver/`: canonical models, typed provider records, history
  discovery, normalization and source reconciliation. It never opens the
  index; `app/silver.rs` coordinates ingestion and checkpoints, and the adapter
  tests that index through storage stay beside it under `app/silver/`.
  Provider-owned history files are bronze inputs.
  `crates/mash-silver/src/dialogue.rs` interprets visible dialogue independently of retrieval;
  canonical messages retain full context alongside an optional `dialogue_text`.
- `app/silver/local.rs` and `crates/mash-search/src/views/local.rs`: local stage boundaries for
  durable canonical ingestion and rebuildable retrieval state. The application
  core wires those stages together.
- `crates/mash-search/src/views/shared.rs`: node-side embedding and publication-payload materialization
  from interpreted silver chats; source scanning stays in the shared
  publication coordinator.
- `crates/mash-search/src/semantic/`, `search_ranking.rs` and `mash-storage`'s `search_*.rs`: views model execution, embedding batches,
  Core ML embeddings, hybrid retrieval and query/ranking policies.
- `app/shared/snapshot/`: backend-independent publication validation; `layers.rs`
  defines persisted silver history and views embeddings within one atomic revision.
- `crates/mash-search/src/views/qdrant/`: common typed REST reads/setup, gRPC document writes, collection schema,
  and dense/BM25 text retrieval. The engine takes an index scope, independently
  of Axum authentication; shared publication owns coordination around it.
  `Store::open` inspects existing collections without writes; `Store::connect`
  explicitly completes setup and then verifies the same reader contract.
- `crates/mash-storage/src/views/documents.rs` and `crates/mash-search/src/views/retrieval.rs`: common document and
  retrieval contracts, also used by the shared HTTP protocol.
- `crates/mash-search/src/views/materialize.rs`: native silver-to-Qdrant text preparation,
  inference and bounded document writes. Callers own silver snapshots, mutation
  coordination and final certification.
- `cli/tests/`: CLI contract tests grouped by behavior, with common setup in
  `support.rs`. CLI and MCP share marker fixtures from `app/test_support.rs`.

Keep assertions in individual tests. Share setup for repeated fixtures, and
split independent cases so each failure names the behavior that broke.
