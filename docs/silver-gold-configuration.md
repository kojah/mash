# Silver enrichment and active gold configuration

Silver is reusable interpreted history. Gold is a replaceable retrieval
representation of that history. Each index has one active gold configuration;
switching configurations replaces that index's projection. Optional saved
presets must not cause parallel projections to be maintained. Local and shared
indexes may eventually use different build configurations.

## Current silver contract and enrichment audit

These are existing capabilities, not a claim that every provider supplies every
field. Missing metadata remains optional rather than being invented.

| Area | Current representation | Boundary and remaining gap |
| --- | --- | --- |
| Identity and provenance | Provider, canonical thread/message IDs, source path/key/session, fingerprints and checkpoints | Provider identity is normalized; local path is provenance, not a portable repository identity. Exact native record offsets and per-field provenance are not generally retained. |
| Dialogue | Canonical content, typed role, optional interpreted visible dialogue | Native tool/hidden/injected records are interpreted before gold. Context-only records remain available for viewing. Structured attachments/tool fields are not a general reusable silver model. |
| Relationships | Thread membership, ordinal, typed linear/root/parent context | Native branches are preserved where adapters support them. Cross-provider duplicate reconciliation is not a general canonical equivalence graph. |
| Thread metadata | Title, project, repository, creation/update times | Adapter-normalized metadata can be absent or derived. Source facts and inferred annotations do not yet have a uniform per-field origin/confidence model. |
| Node ownership | Shared publication node, revision and group boundary | A local message does not carry a persisted distributed node identity independently of publication. |
| Lifecycle | Tombstones, source checkpoints, malformed-record diagnostics and gold invalidation | Source/embedding fingerprints exist, but there is no general durable silver enrichment version/completion certificate and gold build manifest. |

The authoritative models are `app/silver.rs`, `app/silver/source.rs` and
`app/message_context.rs`; persistence is documented in `history-pipeline.md`.
Enrichment must preserve source facts, use typed models, expose absence, and
record the rule/version and provenance of inferred annotations. No mandatory
LLM enrichment is proposed. Embedding models, gold chunking and ranking do not
belong in canonical silver. Topic assignments remain experimental gold.

Prioritize durable interpretation/completeness versioning and traceable metadata
before speculative enrichment fields. Gold should consume the retained visible
dialogue and relationship model rather than attempting to reinterpret provider
formats. Adding finer retrieval units may require source-offset/segment mappings;
that requirement must be audited before a chunk schema is finalized.

## Implemented query configuration

Node configuration accepts a single strict typed TOML table:

```toml
[gold.query]
unit = "threads" # "threads" or "messages"
limit = 20       # 1 through 200
```

Either field may be omitted. Omitting the table preserves the existing defaults
of threads and 20 results. Unknown fields, unsupported units and invalid default
limits are configuration errors. `mash status` reports the effective defaults.

CLI `search --unit/--limit` and MCP `search` unit/limit parameters override these
defaults independently. Omitted parameters remain omitted through the wrappers;
the application core resolves them after loading current configuration. The idle
worker executes that same core, so changes take effect on its next request.
These defaults apply to the search request, including combined local/shared
search; they do not configure the server's persisted gold build.

Changing these defaults or supplying query overrides performs no ingestion,
model re-embedding, index mutation or readiness publication. Existing explicit
query-limit semantics are preserved; the bounds above validate configuration
defaults, not a new constraint on explicit requests.

## Remaining build and query configuration

Build settings must be separate from `gold.query`: content selection, retrieval
units, text preparation, representation/model selection and index strategy.
Operational backend URLs, credentials, limits and deployment remain separate.
Do not expose settings whose runtime behavior has not been implemented.

A build manifest must identify silver revision/completeness, interpretation
version, normalized build settings, model generation and projection schema.
Query-only defaults must be excluded from its identity. Source changes and build
changes must invalidate affected gold without mutating canonical silver.
Independent rebuilding must operate on stored silver and publish readiness only
after successful completion; failure/restart must leave gold detectably stale.

The full acceptance gate remains: switch between two different build
configurations over unchanged silver, retain only the active projection, prove
canonical history unchanged, exercise invalid/stale/recovery paths across local
and shared indexes, and measure storage, indexing and search costs. Query defaults
alone do not satisfy that gate. Additional retrieval/ranking and result controls
must be introduced through the same shared core with concrete capabilities and
bounded work.
