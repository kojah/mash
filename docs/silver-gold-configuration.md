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
| Thread metadata | Title, project, repository, creation/update times; versioned local metadata evidence | Codex/Claude title and project origins are recorded. Other origins remain explicitly unknown; full adapter coverage, timestamps and shared propagation remain pending. |
| Node ownership | Shared publication node, revision and group boundary | A local message does not carry a persisted distributed node identity independently of publication. |
| Lifecycle | Tombstones, versioned source checkpoints, rejected-source ledger, per-provider scan state and local silver certificate | Local completeness and interpretation version are durable. Local gold has a typed build manifest. Shared certificate/manifest propagation and per-field enrichment origins remain pending. |

The authoritative models are `app/silver.rs`, `app/silver/source.rs` and
`app/message_context.rs`; persistence is documented in `history-pipeline.md`.
Enrichment must preserve source facts, use typed models, expose absence, and
record the rule/version and provenance of inferred annotations. No mandatory
LLM enrichment is proposed. Embedding models, gold chunking and ranking do not
belong in canonical silver. Topic assignments remain experimental gold.

Prioritize shared interpretation/completeness versioning and traceable metadata
before speculative enrichment fields. Gold should consume the retained visible
dialogue and relationship model rather than attempting to reinterpret provider
formats. Adding finer retrieval units may require source-offset/segment mappings;
that requirement must be audited before a chunk schema is finalized.

## Durable local silver certificate

### Local metadata evidence

SQLite schema 38 adds `conversation_enrichment`, keyed by canonical thread ID
with cascading deletion. Its strict typed JSON contains an evidence `version`
and optional `title`, `project` and `repository` origins. An absent field value
has no origin. A present value without audited provenance has `kind = "unknown"`;
no confidence score or source fact is invented.

For Codex and Claude Code, titles selected by the native interpretation pipeline
record `kind = "first-visible-user-message"`, the canonical `message_id` and
`rule_version = 1`. Hidden context does not supply a title. Native project paths
record `kind = "source-metadata"`, `field = "cwd"`; the existing thread source
identity locates the underlying provider record stream. Repository origins are
currently unknown because the adapter may use native metadata or a Git-config
fallback. Other adapters can mix native titles, catalogue metadata and message
fallbacks; equality of their final title text is not treated as provenance.

Evidence contains references and small enums, without copied dialogue. It is
fingerprinted with canonical metadata and advances silver's revision when it
changes. Gold building leaves this table unchanged. Native message-summary
staging retains the identity of the first visible title-bearing user message.

Migration preserves history and leaves prior evidence absent. It never guesses
old origins. Missing or outdated evidence prevents a stored source checkpoint
from skipping an applicable explicit ingestion replay; malformed evidence is a
typed database error. Clean replay fills or repairs evidence even if canonical
message text is unchanged. Date cutoffs still apply. Source-free gold rebuilding
does not replay providers or invent missing evidence.

This is a local persisted foundation. Shared storage/API propagation, exposing
origins through thread metadata, remaining adapter fields, timestamp derivation
and precise native offsets remain tracked follow-ups. The interpretation
certificate continues to certify history ingestion, not complete provenance
coverage for every metadata field.

SQLite schema 36 stores a canonical `silver_state.revision`, interpretation-rule
version, per-provider `silver_scans` and source-keyed `silver_rejections`.
Canonical thread/message, deletion-intent and Claude provenance mutations advance
the revision transactionally. Embeddings, FTS, vector caches and scan/checkpoint
bookkeeping do not. Interpretation version is a separate build dependency.

`index` and `status` expose a typed `silver` certificate with `revision`,
`interpretation_version`, `completeness` and `rejected_sources`. Completeness is
`unverified`, `pending`, `incomplete` or `complete`. Older read-only status or a
missing database has no certificate. Local gold reads durable current evidence;
a transient ingestion summary alone cannot authorize publishing readiness.

All targeted provider scans become pending before any partial batch can commit.
Interrupted non-authoritative scans therefore retain pending state after restart.
Authoritative provider failures retain the existing savepoint/transaction rollback
behavior. A clean provider or date-window subset cannot clear another source's
rejection. A clean replay clears only that source's rejection; deletion clears it
only after a complete discovery verifies absence inside the reconciliation roots
and time window. Rejected snapshots leave prior canonical contributions intact.

Completeness certifies admitted stored history, not exhaustive filesystem
coverage: optional built-in roots may be absent. Their existing conservative
reconciliation behavior is retained. A hard discovery/parse error still fails the
run. A known rejected source remains incomplete even if a later cutoff skips it.
CLI/MCP indexing reports retry status until durable evidence is complete.

A rejected refresh that changes no canonical data may retain the previously
published gold snapshot. Its generation, silver revision and interpretation
version must match, with complete vector coverage and no pending embedding work.
This resumes existing gold; incomplete silver cannot authorize a new projection.
Changed canonical data or a changed model prevents resumption. Thus initial
rejections remain unavailable, while an unchanged prior index can remain usable
even as index/status reports an unresolved source rejection. These resume fields
are gold readiness metadata, not a second maintained projection.

Source checkpoints record `INTERPRETATION_VERSION`. A mismatched version cannot
skip replay. Existing schema-35 history and vectors are retained during migration,
but absent completeness evidence is not fabricated and old gold readiness is
invalidated. Run explicit indexing to certify the data. Old checkpoints outside
the selected time window remain unverified; replay the applicable history (for
example with `--all-history`) before rebuilding from that silver. Merely reopening
the database does not certify it. Changing interpretation rules requires bronze
replay; changing gold policy must operate on the already interpreted silver.

Shared routed source checkpoints also persist the interpretation version. Native
and Claude publication replay admitted sources when their version differs,
even when file stamps and embedding generation match. PostgreSQL schema 17
migrates retained checkpoints to version 0; Redis/Qdrant snapshot decoding treats
an absent version the same way. The server reports those checkpoints as unready
until an applicable explicit indexing run replays them. A date cutoff can leave
older checkpoints unverified; it does not retroactively certify their contents.
Existing canonical history and gold vectors survive this migration.

This remains the local certificate foundation. The shared pure-silver revision
and completeness certificate and uniform per-field source-versus-inferred
metadata evidence are still separate work; versioned shared checkpoints alone
do not prove either has shipped.

## Explicit local gold rebuilding

`mash index --rebuild-local-gold` builds local FTS and embeddings from retained
silver. Add `--full` to force replacement of local derived state. MCP exposes the
same operation as `index` with `rebuild_local_gold = true`, including progress
notifications when the request carries a progress token. The response identifies
the mode, reports zero provider files scanned and zero ingestion time, and returns
the retained silver certificate.

This mode requires an existing current-schema database with complete, current
silver. Missing, migrated-but-unverified, pending or rejected silver cannot
authorize it. The preflight is read-only and completeness is checked again under
the writer transaction before derived-state mutations. Use ordinary explicit
indexing to repair/replay silver first. No models are downloaded automatically.

Rebuilding does not discover providers, read source files, advance source
checkpoints or change canonical history. It explicitly targets the local index
and does not upload to the shared service, even when normal indexing is configured
for both. Provider/time selectors and watch mode conflict with this operation.
Existing model configuration still selects the embedding implementation; offline
inference requires the default installed local models rather than an optional
remote embedding endpoint.

The existing bounded gold builder persists invalidation before embedding work
and checkpoints partial vectors. Repeating this command resumes/rebuilds derived
state from silver after failure; it never repairs silver implicitly. Shared gold
rebuilding and active build-recipe manifests remain subsequent work.

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

## Active local build recipe and manifest

```toml
[gold.build]
embedding_text = "dialogue" # default; alternative: "title-and-dialogue"
```

The default preserves the existing visible-dialogue embedding input. The second
recipe prefixes each visible message with `Thread: <title>` and a blank line.
Titles are trimmed and bounded to 256 Unicode characters. Missing or blank titles
leave the dialogue unchanged; hidden records never become searchable through a
title. Deduplication runs after preparation, so equal messages in differently
titled threads can have distinct representations. This is an opt-in contextual
representation, not a demonstrated search-quality improvement.

Each SQLite index stores one typed `gold_manifest` containing schema version,
build settings, base model generation and the silver certificate used to build it.
`index` and `status` expose this as `gold`. Query defaults are excluded. Direct,
worker and combined local search check this identity before retrieval. Changing
the recipe makes the previous projection incompatible until an explicit
`index --rebuild-local-gold` succeeds. Replacement retains only the active vector
generation. Title changes conservatively rebuild all title-prefixed embeddings.
Safe forgetting maintains the remaining projection and advances its manifest only
after checking complete surviving coverage.

SQLite schema 37 adds the manifest without copying canonical content. A schema 36
index needs an explicit gold rebuild to certify its manifest; it need not reread
provider files when its stored silver certificate is complete. Build failure
leaves readiness unavailable and recovery uses retained silver.

This recipe configures local gold. Shared materialization retains the default
dialogue representation; nondefault local recipes use the independently routed
publication path rather than forwarding local vectors as shared default vectors.
Configurable shared build recipes and shared manifests remain follow-up work.

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
