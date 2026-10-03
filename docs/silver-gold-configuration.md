# Silver enrichment and active gold configuration

Silver is reusable interpreted history. Gold is a replaceable retrieval
representation of that history. Each index has one active gold configuration;
switching configurations replaces that index's projection. Optional saved
presets must not cause parallel projections to be maintained. Local and shared
indexes may use different build configurations.

## Current silver contract and enrichment audit

These are existing capabilities, not a claim that every provider supplies every
field. Missing metadata remains optional rather than being invented.

| Area | Current representation | Boundary and remaining gap |
| --- | --- | --- |
| Identity and provenance | Provider, canonical thread/message IDs, source path/key/session, fingerprints and checkpoints | Provider identity is normalized; local path is provenance, not a portable repository identity. Exact native record offsets and per-field provenance are not generally retained. |
| Dialogue | Canonical content, typed role, optional interpreted visible dialogue | Native tool/hidden/injected records are interpreted before gold. Context-only records remain available for viewing. Structured attachments/tool fields are not a general reusable silver model. |
| Relationships | Thread membership, ordinal, typed linear/root/parent context | Native branches are preserved where adapters support them. Cross-provider duplicate reconciliation is not a general canonical equivalence graph. |
| Thread metadata | Title, project, repository, creation/update times; versioned metadata evidence retained locally and published to Qdrant | Codex/Claude title and project origins are recorded. Other origins remain explicitly unknown; full adapter coverage and timestamp provenance remain pending. |
| Node ownership | Shared publication node, revision and group boundary | A local message does not carry a persisted distributed node identity independently of publication. |
| Lifecycle | Tombstones, versioned source checkpoints, rejected-source ledger, per-provider scan state and local silver certificate; shared canonical revision/fingerprint, scoped provider coverage and full-snapshot source certificates | Local completeness and interpretation version are durable. Local gold has a typed build manifest. Shared provider coverage verifies retained checkpoints independently of vectors. Group-wide build authorization, shared gold manifests and broader enrichment origins remain pending. |

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

The same optional typed evidence is published through both snapshot and routed
uploads. Qdrant retains it in typed conversation records. Both local and shared indexes
return it in thread listings and search metadata. Local listings and search do
the same, through generated SQLite reads. CLI and MCP share these core models.

Absent evidence is omitted from output rather than fabricated. Replacing a
thread with a record that has no evidence clears its previous annotations.
Uploads reject unsupported evidence/rule versions and invalid message identities
before acknowledgment. These are node-supplied provenance claims; structural
validation does not independently prove the provider's source facts.

Remaining adapter fields, timestamp derivation and precise native offsets remain
tracked follow-ups. The interpretation certificate continues to certify history ingestion, not complete provenance
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
even when file stamps and embedding generation match. Qdrant snapshot decoding treats
an absent version as version 0. The server reports those checkpoints as unready
until an applicable explicit indexing run replays them. A date cutoff can leave
older checkpoints unverified; it does not retroactively certify their contents.
Existing canonical history and gold vectors survive this migration.

The shared canonical revision is now tracked independently, as described below.
Its completeness certificate and uniform per-field source-versus-inferred metadata
evidence remain separate work; a fingerprint does not prove either has shipped.

### Server-owned shared silver identity

Qdrant publication heads persist an optional typed `silver_revision`, returned
by `/v1/state` and in each node's `/v1/freshness` entry (also visible through
CLI/MCP shared status). It contains `schema_version = 1`, a node-scoped `revision`
and a BLAKE3 `fingerprint`. This is independent of the upload revision, the local
source certificate's revision, and the embedding generation.

The fingerprint covers deterministic canonical thread metadata and enrichment,
messages (identity, membership, ordinal, branch context, role, original content,
interpreted visible dialogue and timestamps) and persistent tombstones. It excludes
vectors, model generations, source checkpoints and scan bookkeeping. The current
stored `projection` is interpreted silver dialogue; changing gold text preparation
must not overwrite it. Canonical edits advance the revision; replacing vectors,
repeating a snapshot or updating checkpoints alone does not. Forgetting also
tracks deletion intents, including tombstones for absent history.

Fingerprinting streams on the existing blocking publication worker with a bounded
64-KiB buffer and no cloned message bodies. The identity becomes visible with the
same conditional Qdrant head as its data. Failed or stale publication cannot
advance it. Invalid stored versions/digests or counter overflow fail closed.
Existing heads leave it absent until an acknowledged publication computes revision
1 over their retained history. Reading old state never invents a revision.

This identifies the history recorded in shared gold manifests; it does not
certify interpretation/completeness. Source-silver certificates and provider scan
evidence remain separate completeness inputs. Group-wide gold builds
must capture the set of node identities/revisions, rather than treating one node's
counter as a shared global clock.

The manual `benchmark_canonical_fingerprint_cost` diagnostic used 10,000 synthetic
messages, repeated Unicode dialogue and 384-dimensional stored vectors. On Xenia's
debug Cranelift build, snapshot encoding produced 40,046,020 bytes in 4,356 ms;
the additional canonical fingerprint took 783 ms. This is one serialization-only
sample, not full ingestion, real-session latency, release performance or a memory
measurement. Representative complete-ingestion/storage/search measurements remain
an acceptance gate. Captures use the `silver-revision-*-20261003.log` prefix under
`/home/james/scratch/mash-artifacts/` outside Git.

### Source-silver certificates on full shared snapshots

Full local snapshot publication includes a typed `source_silver` certificate
read in the same SQLite transaction as the outgoing history. Its revision is
the source silver revision, distinct from the publication revision. The server
persists it atomically with the accepted snapshot and returns it from the node
state endpoint. Qdrant stores it in the atomic publication head. Historical snapshots
leave it absent. A duplicate publication preserves its accepted certificate.

This is a node-supplied statement about the source history, independent of vector
coverage or embedding generation. It does not by itself authorize gold search
or certify full enrichment coverage. Unsupported interpretation versions and
contradictory complete-with-rejections certificates are rejected. Interrupted
uploads never publish a certificate.

Independently routed ingestion cannot attach a full-local-snapshot certificate:
its provider allowlist and retained history may differ. Routed updates, replacement
snapshots without a certificate, and server-side forget operations clear the prior
source certificate. The scoped retained-coverage contract below supports routed
ingestion. Group-wide build authorization, shared gold manifests and source-free
shared rebuilds remain follow-up work.

### Routed provider scan evidence

Routed publication now ends each verified provider scan with a typed scan report:
provider, interpretation version, discovery authority and cutoff. The server
requires authoritative reports to match a completed reconciliation and its cutoff.
It rejects duplicates, unsupported rule versions and checkpoint/observation events
after that provider's report. The reports become visible only with the acknowledged
publication and are returned as `silver_scans` in node state, independently of
embedding coverage. Partial discovery is explicitly reported as non-authoritative.

Qdrant retains this report list in the atomic head. Each publication replaces the list. Legacy uploads,
full snapshots and server-side forget leave no routed reports. Historical scan
authority is never inferred from checkpoint presence or vector readiness.

These are node-supplied observations of the latest published scan scope. They
remain distinct from durable `silver_coverage`, exposed in node state and shared
freshness. Each coverage entry contains the acknowledged `scan`, its
`publication_revision`, and typed interpretation `completeness`.

Coverage survives uploads for other providers and no-op publications. A new
report replaces only that provider's scope; cutoffs are never unioned or widened.
An edit to that provider's threads, messages, checkpoints, observations or
reconciliation without a concluding report revokes its evidence. Canonical data
after a provider's report is rejected, as are late checkpoint/observation events.
Full replacement snapshots clear routed coverage and retain their separate local
source certificate when supplied. Server-side forget conservatively clears all
routed coverage. Failed uploads and idempotent retries preserve accepted evidence.
Legacy heads have no coverage; reading them never fabricates it.

An authoritative current-rule report yields `complete` only if **all retained
checkpoints** for that provider have the current interpretation version and every
retained thread has a provider-matching checkpoint. Older retained sources outside
the discovery cutoff are checked too. Missing mappings or legacy versions yield
`unverified`; non-authoritative discovery yields `incomplete`. A verified empty
scope can be complete. Embedding coverage, generation and checkpoint `ready` do
not participate. This verifies admitted-history interpretation, not the truth of
node-supplied source facts or complete provenance for every metadata field.

`complete` does not mean exhaustive filesystem discovery: the accompanying cutoff
and authority describe the observed source scope. Consumers must keep these
scopes and per-node canonical identities when forming a group-wide gold manifest.
No shared gold build is authorized by this milestone alone.

## Explicit local gold rebuilding

### Shared gold-only publication boundary

`POST /v1/gold` accepts authenticated `application/x-ndjson` for the token's
own node. It operates only on stored Qdrant silver, without provider discovery,
source files, parsing or reinterpretation. The service performs no inference;
the caller supplies embeddings. The strict stream contains:

1. `type = "start"`, `publication_revision`, `silver` (the server-owned
   revision/fingerprint/schema), and `generation` (the configured model space).
2. `type = "embeddings"`, `records` containing only `message_id` and `vector`,
   in batches of at most 128.
3. Exactly one final `type = "finish"`.

Both input identities must match current stored state. A complete current-rule
source certificate, or complete retained-provider coverage for every stored
thread's provider, is required. Empty uncertified or legacy histories cannot
become certified through this operation. An empty routed scope needs an actual
complete provider report. The model and dimensions must match the configured
shared embedding space. Every visible stored message needs exactly one valid
vector. Unknown, duplicate or context-only identities, malformed vectors,
missing coverage, canonical record events and unfinished streams fail before
publication. Input is spooled with the same byte/line limits and bounded mutation
admission as ingestion.

Replacement reuses immutable Qdrant staging and conditional head publication.
Canonical content, visible dialogue, context, metadata, tombstones, checkpoints,
scan evidence, source certificate and the silver identity remain unchanged.
Source synchronization timestamps and the **source-upload revision** also remain
unchanged. Incrementing that counter for a gold build would collide with the next
local SQLite snapshot revision. Qdrant's separate immutable version and head CAS
serialize gold updates and source uploads. Concurrent canonical changes make the
old rebuild fail or leave it hidden; a failed build preserves the active index.
There is one active node head; obsolete physical staging cleanup does not make a
second maintained projection.

`GET /v1/silver` returns the authenticated node's retained canonical history and
its silver identity, source-upload revision, target model generation and dimensions.
The strict history model contains threads, canonical messages/visible dialogue,
checkpoints and tombstones. It has no vectors or gold generations. Uncertified or
legacy history without a canonical identity is rejected. Other nodes' data cannot
be selected through this route.

The service captures and decodes one immutable snapshot, then serializes borrowed
silver rows on a blocking worker into a temporary file. The HTTP body streams that
file. It neither copies all content into a JSON buffer nor repeatedly decodes the
whole node for separate pages. Snapshot-work admission covers decoding/encoding;
the upload concurrency permit follows the blocking worker and response body until
completion or cancellation. Temporary files are removed when the body is released.
The client uses bulk HTTP admission and streamed typed decoding, separate from
interactive search/context slots. It currently retains the node's decoded silver
in memory; inference and vector-output batches are bounded, not the entire corpus.

`mash index --rebuild-shared-gold` and MCP `index` with
`rebuild_shared_gold = true` call the same typed core. They require an existing
local SQLite database with the current metadata schema and the `both` destination.
The local preflight opens read-only and never initializes or migrates SQLite.
They do not mutate local history,
local vectors, local manifests or publication bookkeeping. They download retained
shared silver, verify matching embedding settings, embed only visible dialogue
in batches of 32 with the configured local or remote inference, then call the
gold-only publication operation. Provider directories are never discovered or
parsed. Missing models fail without downloading or publishing; concurrent source
changes fail the final identity check rather than overwriting new silver.

Local and shared rebuild flags are mutually exclusive and cannot be combined with
provider/time selectors or watching. `--full` is accepted, but this explicit
shared operation always replaces complete vector coverage. The core uses a typed
indexing-mode enum; wrappers retain ergonomic CLI/MCP flags. Progress includes
`shared-silver-read` with periodic elapsed updates, model loading, shared embedding
counts and phase ETA, upload/commit, and completion. MCP streams these through the
existing progress-token notifications. The response identifies the shared counts,
silver revision, model generation, inference counts and download/model/build/
publication/total timings; it does not report a fictitious local ingestion run.

A one-message functional fixture on Xenia with real MiniLM assets and disposable
Qdrant measured a CLI rebuild at 2,411 ms: 2,233 ms model loading, 39 ms retained
silver download/decode, 6 ms inference and 56 ms publication, plus orchestration
and serialization. This used the debug Cranelift build on the concurrent test
host. CLI and MCP succeeded after provider folders were removed; all compared
local canonical, embedding, manifest and publication tables remained unchanged.
The tiny fixture verifies the workflow and timing fields, not real-corpus
throughput, peak memory or the full storage/build/search cost acceptance gate.
Evidence is in the external artifact
`shared-rebuild-client-readonly-final-20261003.log`.

Independent shared recipes, build manifests and group-wide configuration
activation remain in the parent implementation scope. This client operation
rebuilds the existing dialogue recipe and preserves current search behavior; it
does not claim that shared configuration acceptance criteria are complete.

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

Local recipes remain independent of shared recipes; nondefault local recipes
use the independently routed publication path rather than forwarding local
vectors as shared default vectors.

## Query intent: exact mentions

`[gold.query] intent = "related-discussion"` retains the existing hybrid retrieval
default. `intent = "exact-mention"` selects the lexical and metadata artifacts,
without embeddings, reranking, model loading or changes to stored silver/gold.
The CLI `--intent` and MCP `intent` override this query default independently of
the active build recipe. Query settings are excluded from build manifests.

Examples:

```sh
mash search voyage-code-4 --intent exact-mention --local --unit messages --conversation thread-a
mash search '"connection pool"' --intent exact-mention --local
```

Equivalent MCP search arguments use the same typed core:

```json
{"query":"voyage-code-4","intent":"exact-mention","local":true,"unit":"messages","conversation":["thread-a","thread-b"],"provider":["codex"],"limit":10}
```

For related discussions, the existing metadata scopes compose with the default
hybrid path, for example:

```json
{"query":"Redis shared index","intent":"related-discussion","unit":"threads","after":1772323200,"before":1775001600,"limit":10}
```

The entire trimmed query is one literal; one paired set of enclosing double
quotes is removed. Matching folds ASCII case only, preserves internal punctuation
and whitespace, and rejects matches adjacent to Unicode letters/numbers, `_`,
`-` or `+`. Thus `voyage-code-4` does not match `voyage code 4`, `voyage-code-40`
or `voyage-code-4-v2`; `C++` does not match `C++20`. Other punctuation separates
mentions. This is literal retrieval, without stemming, typo correction, synonyms
or language-model restrictions. Queries need at least one letter/number and are
bounded to 16 KiB. Common words are allowed because they may be code identifiers.

SQLite FTS5 supplies phrase candidates; a deterministic literal predicate and
metadata filters run before newest-first ordering and the bounded candidate limit. The
existing thread/message deduplication and result shaping remain in use. Only
interpreted visible text is matched; titles and hidden canonical text are not
extra matching fields. Completed indexing, a matching silver certificate and a
clean lexical artifact remain required even if dense vectors/models are absent
or a different embedding recipe is configured. Search opens SQLite read-only
and cannot initialize an index. The CLI bypasses its model worker for this mode.

Exact mentions also support shared-only and combined retrieval through the HTTP
service. Combined search reads local SQLite and peer-node Qdrant candidates
concurrently, excludes this node's shared copy, and uses the existing copy
deduplication and thread shaping. Matching evidence is ordered newest first,
with undated evidence last and node/message identity breaking ties.

Shared publication records a separate typed lexical manifest pinned to retained
silver, interpretation rules and lexical representation version. Freshness exposes
`lexical_manifest` and `lexical_current` independently of dense readiness. Old
heads without this evidence require explicit indexing or rebuilding; search never
repairs them. Changing a dense recipe or lacking vectors does not invalidate a
current lexical artifact.

Qdrant scans token/metadata-filtered candidates in 100-point pages, retrieving
visible dialogue and metadata without vectors or canonical message bodies.
Literal checks and current visibility rules precede result limits. The server
checks publication stability and enforces a 10,000-candidate and 1.5-second
budget. Exhaustion is an explicit error asking for narrower filters, rather than
an apparently complete partial result. Combined queries fall back to local
results with `shared_error`; shared-only queries fail. Both CLI and MCP resolve
the configured intent in the shared core, so exact queries bypass model loading
and model-work admission even when the intent override is omitted.

Validation on 2026-10-03: the real-model integration fixture found the visible
`login token` message and rejected hidden tool output in both result shapes,
using a nonexistent model directory and an incompatible configured dense recipe.
The four CLI invocations took 20, 20, 19 and 17 ms including process startup on
Xenia. The two-message SQLite fixture was 184,320 bytes and remained byte-for-byte
unchanged. These small-fixture timings are diagnostic, not full-corpus latency
or percentile estimates. The change adds no persistent tables or content copies;
it reuses the existing FTS5 and metadata artifacts. Logs are in the external
`mash-artifacts/gold-exact-real-model-20261003.log` artifact.

Shared validation used two nodes with 1,271 messages each, including over 1,100
token decoys, legacy hidden-tool projections, literal boundary cases and scoped
metadata. Eight debug-build HTTP queries took 297, 270, 18, 15, 15, 17, 14 and
11 ms in the contract-suite run. A separate 10,001-candidate fixture verified
explicit budget failure and recovery with a time filter. These are synthetic
diagnostics, not real-corpus percentiles. Retained shared silver/freshness and
local database bytes remained unchanged by queries. The independent lexical
manifest adds small per-node publication metadata, without another content copy.
The Qdrant contract log is `shared-exact-qdrant-contracts-20261003.log` under
the external artifact directory. Broader artifact selection/calibration remains
tracked in `mash-q3e.4.2` and `mash-q3e.6`.

The persisted restart contract also verified lexical freshness and exact search
after graceful Qdrant/API restart and SIGKILL with retained storage. The final
standard suite passed 632 tests; the separate integrated run passed the three
exact Qdrant contracts, restart contract and real-model hybrid regression. This
establishes process-restart recovery, not power-loss durability or failover.

## Shared build recipes and manifests

The server TOML selects the shared recipe independently of node configuration:

```toml
[gold.build]
embedding_text = "title-and-dialogue" # default: "dialogue"
```

Only these two values are accepted. Backend connection, memory/index strategy
and embedding model settings remain in their existing operational sections.
Query policies are not part of this server build setting.

`mash index --rebuild-shared-gold` and MCP `index` with
`rebuild_shared_gold = true` read the selected recipe alongside retained silver.
They prepare bounded title prefixes without changing canonical message content,
visibility or stored dialogue. Uploads declare the recipe and fail if it changed
before publication. Node-local provider files and SQLite rows are not touched.
Each published node head retains one gold manifest with schema/rule versions, build
settings, model generation and the server-owned silver revision/fingerprint.
Replacing gold atomically replaces that node head; the previous immutable stage
is removed by the existing cleanup path.

Shared status exposes `gold_manifest` and `gold_current` per publishing node.
Semantic search rejects an absent, unknown-version, stale or mismatched manifest.
Recipe compatibility is checked across the group, including excluded nodes;
excluding a node cannot hide mixed recipes. Model/vector readiness still checks
the searched nodes. Existing unmanifested deployments require explicit gold
rebuilding to certify their vectors.
If their retained silver also lacks current completeness evidence, explicit
source ingestion is required before a source-free rebuild can be admitted.

After changing server recipes, rebuild every publishing node. During this
transition search fails until the group has matching manifests. This is **not a
group-wide atomic activation transaction**: availability during coordinated
switches and a durable group configuration pointer remain follow-up work.
Ordinary source uploads currently produce dialogue vectors. After publishing a
nondefault recipe, an ingestion delta can leave mixed representations and clears
the build evidence until a complete shared gold rebuild succeeds. Automatic
recipe-aware ingestion remains follow-up work; mixed vectors never become
certified merely because their dimensions match.

Real-model integration coverage switches dialogue → title-plus-dialogue →
dialogue after removing provider directories. It compares retained history and
all local canonical/derived tables, verifies different/restored vector scores,
and executes query-only unit/limit overrides without changing either index.
This establishes representation behavior, not improved search relevance.

On the one-message Xenia test fixture, cold source-free rebuilds took 1.75–2.00 s;
model loading accounted for 1.50–1.69 s, inference for 10–32 ms, retained-silver
download for 23–26 ms and publication for 72–133 ms. The unchanged serialized
history was 1,043 bytes. These are debug-build correctness-fixture measurements
under concurrent host load, **not full-corpus throughput, storage or search
benchmarks**. The lifecycle acceptance bead still requires those measurements.

## Shared source facts and live gold readiness

Stored shared silver source stamps contain only provider, source identity/hash,
size, modification time, fingerprint, interpretation rule version and thread
mapping. They do not contain model readiness. Snapshot and retained-silver
serialization borrow these fields without copying whole histories. Reading an
existing checkpoint's known `ready` field discards its claim; new snapshots omit
it. Unknown model fields still fail strict decoding.

`/v1/state` is a live transport view: its checkpoint `ready` value is derived
from visible-message vector coverage, current model/rule versions and the
selected recipe's published manifest. That flag can change when gold changes;
the underlying source facts remain unchanged. Missing interpretation versions
remain unverified and require explicit source replay. This separation does not
change source reconciliation or provider completeness evidence.

The existing 10,000-message synthetic serialization diagnostic now emits
40,046,007 bytes, 13 fewer than its previous snapshot with one `ready = true`
source stamp. Encoding took 3,745 ms and canonical hashing 654 ms in this debug
Cranelift run. These timings are single samples under host load; they do not
isolate a speedup or replace full-corpus storage/ingestion/search measurements.
The capture is `silver-source-facts-serialization-cost-20261003.log` in the
external artifacts directory. Borrowed source serialization adds no whole-node
or body copies and performs no inference.

## Multiple artifacts under one configuration

One active configuration can enable several complementary gold artifacts:
metadata/lexical projections, dense retrieval units, contextual episodes,
multi-label topics and extractive representative passages. These are derived
from the same silver and are not simultaneously maintained alternative profiles.
Building an artifact does not require every search to use it. A typed query
intent and bounded retrieval policy will choose supported artifacts; callers
should specify their goal and filters rather than internal names or weights.

This remains implementation work in `mash-q3e.6` and `mash-q3e.4.1`–`.4.5`:
artifact identities/dependencies/readiness, intent API, selection/fallback,
topic/evidence presentation, routing diagnostics and held-out quality/cost
gates. The two embedding-text recipes above establish build identity and
replacement; they do not implement or validate intent-aware routing.

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
