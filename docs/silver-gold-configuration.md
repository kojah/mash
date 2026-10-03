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
| Thread metadata | Title, project, repository, creation/update times; versioned metadata evidence retained locally and published to Qdrant | Codex/Claude summary-title, cwd and message-boundary times; OpenCode native title/directory/worktree/session times; Hermes title/cwd/repository root and Pi title/cwd are recorded. Unaudited origins remain explicitly unknown; full adapter and individual-message timestamp coverage remain pending. |
| Node ownership | Shared publication node, revision and group boundary | A local message does not carry a persisted distributed node identity independently of publication. |
| Lifecycle | Tombstones, versioned source checkpoints, rejected-source ledger, per-provider scan state and local silver certificate; shared canonical revision/fingerprint, scoped provider coverage and full-snapshot source certificates | Local completeness and interpretation version are durable. Local and shared gold have typed build manifests and independent lexical publication contracts. Shared provider coverage verifies retained checkpoints independently of vectors. Group configuration has durable authority; explicit rebuilds refresh gold after a switch. Unaudited origins remain unknown. |

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
and optional `title`, `project`, `repository` and `timestamps` origins. An absent field value
has no origin. A present value without audited provenance has `kind = "unknown"`;
no confidence score or source fact is invented.

Thread timestamp evidence audits Codex/Claude Code's minimum and maximum over
all retained normalized message timestamps, including hidden context. Each
`message-boundary` origin retains a canonical message ID, `minimum` or `maximum`
selection, and rule version 1. Equal timestamps choose the lexicographically
smallest message ID, independently of inline/spooled storage. OpenCode retains
`source-metadata` origins for `session.time_created` and `session.time_updated`;
rule version 1 selects the existing native millisecond integers unchanged.
Missing selected times have no origin. Hermes, Pi and Gemini also retain adapter-selected
timestamp provenance before their fallback choices flatten into final values.

Hermes creation selects `sessions.started_at`. Its update candidate preserves SQL
`COALESCE(ended_at, last_activity_at, started_at)` priority, rather than taking the
maximum of those three native fields. It then selects the maximum of that candidate
and retained normalized message times. The native candidate wins ties after the
existing seconds-to-milliseconds conversion (bounded, finite, truncated). Message
ties use the smallest canonical message ID. Native origins identify the selected
`sessions.ended_at`, `sessions.last_activity_at` or `sessions.started_at` field;
message origins retain the selected ID and maximum boundary. Unselected native
values are not newly decoded. Missing native and message times remain absent.

Pi creation selects the RFC3339 `session.timestamp` header converted to milliseconds.
Update selects the maximum retained normalized message timestamp, using the smallest
canonical message ID on ties; only an empty timestamp set falls back to the header.
In particular, a message time before the header can remain the update time, as in
the existing adapter. Per-message native integer versus entry-time fallback remains
an unaudited finer provenance boundary. These rule-version-1 annotations preserve
existing timestamp values and add no inference or extra corpus scan.

Gemini preserves native RFC3339 `session.startTime` and `session.lastUpdated`
after header/update replay, converting them to milliseconds. A present native
field wins even when it equals or precedes retained message times. Missing
creation falls back to the minimum retained canonical message time; missing
update falls back to the maximum, then the selected creation time when no
message time exists. Equal boundary times retain the lexicographically smallest
canonical message ID. Empty history with no native times has no values or
origins. These rule-version-1 annotations preserve existing parser values and
do not add model work. The typed native field names are additive: update API
readers before replaying Gemini sources that publish them.

Other adapters remain explicitly unknown.

The optional timestamp container is additive to evidence version 1. Older records
without it remain readable and publishable, without manufacturing provenance
during source-free gold rebuilds. Explicit ingestion of these three audited
providers, plus Hermes, Pi and Gemini, requires source replay before a checkpoint can certify
current evidence. The latter three also replay old timestamp containers containing
unknown origins when selected times are present.
Local checkpoint freshness compares timestamp-origin presence with the retained
creation/update values using the generated SQLite query. Empty origin slots do not
certify present timestamp values. Legacy reads, publication snapshots and source-free
gold rebuilding preserve the stored evidence; explicit ingestion repairs it.
The same checkpoint query now includes title, project and repository values.
Each present value, including an empty string, requires a present origin; an
explicit `unknown` origin distinguishes unaudited provenance from absence.
Origins without corresponding values also require replay. This check concerns
presence, not value equality, and never infers source provenance from metadata.

New shared conversation publications validate this correspondence for all five
metadata fields. Contradictions return a usage error before publication and do
not advance the acknowledged revision. A whole missing enrichment object or a
missing additive timestamp container remains supported for legacy snapshots.
Once a timestamp container is present, its origin slots must correspond to the
creation/update values. Existing stored evidence remains readable and untouched
by source-free gold construction; publishing contradictory evidence requires
explicit source replay. These structural checks do not authenticate source claims
or prove that message/record references belong to the declared thread.
Replay repairs annotations and advances silver's revision without changing
canonical message history. Shared publication validates rule versions and bounded
message references; it does not independently authenticate source claims or prove
that a referenced message belongs to the thread. This does not yet audit individual
message timestamp normalization or the other adapters' fallback selections.
The existing summary pass retains two additional IDs; it adds no inference,
content copies, or additional pass over staged messages.
Strict wire decoding requires the HTTP service and querying clients to understand
the timestamp vocabulary before updated nodes publish it. Coordinate executable
upgrades for a node group; older stored records remain readable by the new code.

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

Audited native fields also retain `kind = "source-metadata"` origins:

| Provider | Metadata | Native `field` |
| --- | --- | --- |
| OpenCode | Title | `session.title` |
| OpenCode | Project | `session.directory` |
| OpenCode | Repository | `project.worktree` (selected only when `project.vcs` is `git`) |
| Hermes | Project | `cwd` from `sessions` |
| Hermes | Repository | `git_repo_root` from `sessions` |
| Pi | Project | `cwd` from the session header |
| Hermes | Native title | `sessions.title` |

These origins describe existing adapter selections, not newly inferred values.
OpenCode always takes its native title, even when its text equals a user message.
Hermes and Pi retain their title selection at the adapter boundary. Hermes uses
`source-metadata` with `field = "sessions.title"` for a native value; a missing
native title uses `first-visible-user-message`, referencing its canonical message
ID and the existing title rule version. Native empty strings retain the existing
native selection; they do not silently become message fallbacks.

Pi retains the last nonblank selected `session_info.name` as `source-record`:
`field = "session_info.name"`, `record_id` and `rule_version = 1`. The record ID
is a normalized reference, not a canonical message ID or the raw native ID.
It uses the existing BLAKE3 ID scheme: hash domain `source-record`, then each
NUL-separated part (provider `pi`, normalized thread ID, native entry ID), keeping
the first 24 lowercase hex characters. This remains stable across file moves,
distinguishes replacement name entries with equal text, and keeps provenance
bounded for large native IDs. The normalized thread ID is computed once per
parse, avoiding repeated hashing of a large session header for each name entry.
The reference can be resolved by recomputing keys over that source's native
entries; ordinary message viewing does not resolve metadata-record references.

A later null or blank Pi name clears the native choice and uses the first visible
user message. Context-only user records do not supply fallback titles. No visible
user and no native title leaves both title and origin absent. Both adapters
preserve the previous title text and message/relationship interpretation; equal
native and inferred text retain different origins. Absent metadata keeps
an absent origin. Repository roots/worktrees remain native paths, not portable
repository identities. Native source/session identity locates the record; precise
record offsets and timestamp derivations remain unaudited.

The evidence schema and existing title inference rule remain version 1; these are
typed additions to the native-field/reference vocabulary. `source-record` rule 1
requires a 24-character lowercase hex reference; unsupported rule versions and
malformed keys fail shared publication before acknowledgement.
Source checkpoint freshness additionally
requires the audited origins for present annotated fields on the applicable
provider. Stored unknown or differently labelled origins cause an explicit source
replay even when the native source stamp has not changed. Replay changes evidence
and its silver revision without changing canonical dialogue. It does not rewrite
old evidence during reads: structurally valid unknown evidence remains usable for
viewing, publication and source-free gold rebuilding. A silver certificate does
not certify complete provenance coverage, and the shared API validates structure,
not the truth of node-supplied origins.

Hermes/Pi title freshness also requires the retained native or inferred selection.
Historical unknown titles stay readable and publishable without retroactive
inference; an explicit applicable ingestion replay fills them. Gold rebuilds use
stored evidence and never reselect titles from provider files. Other adapters'
selection paths, general native-record offsets and timestamp provenance remain
follow-up work.

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
ingestion. Shared manifests and source-free rebuilds are implemented below;
group-wide replacement and broader enrichment coverage remain follow-up work.

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

[gold.query.fusion]
rank_constant = 60  # 1 through 1000
lexical_weight = 1  # 1 through 100
semantic_weight = 1 # 1 through 100
```

Either field may be omitted. Omitting the table preserves the existing defaults
of threads and 20 results. Unknown fields, unsupported units and invalid default
limits are configuration errors. `mash status` reports the effective defaults.

Related-discussion candidate fusion uses weighted reciprocal ranks:
`lexical_weight / (rank_constant + lexical_rank)` plus
`semantic_weight / (rank_constant + semantic_rank)`, with ranks starting at one.
Replicated evidence is deduplicated before ranking. These settings apply to both
local and shared candidates, including local fallback. They change candidate
preference before contextual reranking; they do not guarantee final ordering
because the reranker still scores selected evidence. Exact-mention retrieval
does not consult this policy. Defaults preserve the previous unweighted RRF.
Fusion is query-only: it changes no silver, vectors, build generation or manifest
and needs no indexing. Changing TOML also reaches the next idle-worker request.
CLI hybrid searches accept `--fusion-rank-constant`, `--lexical-weight` and
`--semantic-weight`. MCP accepts, for example, `"fusion":{"semantic_weight":4}`.
Each omitted field inherits the current TOML default, rather than an SDK or
worker-startup value. These request-only overrides do not persist or change the
active build. Later searches without overrides return to current defaults.
Invalid values fail before model work, SQLite or HTTP access. Explicit fusion
overrides with exact-mention are rejected because that operation does not use
hybrid fusion. Empty MCP fusion objects behave like omitted overrides. Further
ranking controls remain follow-up work.

Node configuration also supports `[gold.query] ranking = "contextual-rerank"`
(the unchanged default) or `"fusion"`. Both related-discussion modes retrieve
and fuse the same lexical and dense candidates with the configured weights.
Fusion-only ranking retains deterministic fused order and skips the two-stage
contextual reranker and its additional context fetches. Thread collection still
groups evidence and limits the returned threads; message collection limits
messages. It does not promise the same ordering or relevance as reranking.
This setting changes no build manifest, embeddings or silver and takes effect on
the next request, including an idle worker. CLI and MCP resolve it through the
same core. Exact-mention ignores hybrid ranking. Diagnostics include the resolved
hybrid `ranking`, with no rerank/context stages in fusion mode. Dense query
embeddings require local inference or a configured remote service. The model
loader loads reranker assets only for contextual-rerank. Local fusion loads its embedding
model alone, retaining installation inventory validation and generation leases.
Its cache identity includes ranking: switching ranking reloads a matching
backend instead of retaining an unused reranker. Remote fusion loads neither
local model and needs no local asset installation; it validates remote settings
and credentials instead. Status distinguishes physical `models_installed` from
effective `query_models` (`"available"`, `"unavailable"`, or `"not-required"`) for current TOML
query defaults. Exact-mention status checks certified, current lexical data without
requiring models or matching the configured dense recipe; it reports lexical
readiness even when dense coverage is incomplete. Missing, dirty or stale lexical
data still requires explicit indexing. This is local readiness; shared inspection
remains separate. Local SQLite schema 39 stores a separate
`lexical_manifest`: schema and representation versions, certified silver revision
and interpretation version, and searchable-message coverage. Its representation
is FTS5 `unicode61` over interpreted dialogue, followed by literal verification.
It contains no canonical text or dense model settings. Status exposes this record
as `lexical`; missing or incompatible records fail readiness checks.

Explicit indexing publishes lexical completion after refreshing derived search
data, before dense inference. If later inference fails after that transaction is
committed, exact search can use the completed lexical artifact while hybrid
search remains unavailable. A rejected source cannot certify new lexical data;
an unchanged previously certified snapshot may remain usable. `forget` advances
a previously current, clean lexical record atomically with deletion, including
when no dense record exists. It does not revive stale or dirty lexical data.
Upgrades from schema 38 or earlier create an empty lexical publication slot;
run explicit indexing (or source-free `index --rebuild-local-gold` with certified
silver) to publish it. Reads never migrate or build it. This establishes an
independent artifact contract. Further artifact types retain separate acceptance gates.
Local and shared builds now select `representations`: `lexical-and-dense` (the
unchanged default) or `lexical-only`. Omitted default values retain the existing
serialized recipe and dense generation identity. For a local lexical-only index:

```toml
[gold.build]
representations = "lexical-only"

[gold.query]
intent = "exact-mention"
```

Lexical-only builds require no embedding assets or inference API. They retain
metadata and lexical gold, remove dense vectors and packed local chunks, and
publish one active build record. Related-discussion defaults are invalid with
this recipe; explicit hybrid overrides fail instead of changing gold or silently
substituting lexical results. Status reports `semantic_support = false`. Switching
back to lexical-and-dense explicitly queues and rebuilds dense data from silver.
Changing representations is a build change, not a per-query option.

The server accepts the same `[gold.build]` setting independently of the node's
local recipe. Nodes use the authenticated shared publication plan to prepare
the requested representation, uploading no vectors for lexical-only. Source-free
shared gold replacement clears old vectors while retaining canonical silver;
hybrid queries against lexical-only shared gold are rejected. Shared-only hybrid
requests first fetch the small authenticated `/v1/capabilities` build record,
before model admission. This adds one HTTP round trip for that mode; combined
search retains its existing local/shared retrieval and fallback path. The server
checks active group authority again when querying, so stale API processes fail
closed. Per-node freshness treats zero vectors as complete only when the active
recipe declares lexical-only. Artifact switches do not publish a second profile.

Tests cover local CLI/MCP model-free indexing, fixed-vector shared replacement,
real local inference when returning to dense gold, and silver invariance. These
are correctness fixtures; representative corpus storage, indexing and search
costs, including the capabilities round trip, remain an acceptance gate.
The [bounded local cost measurement](local-gold-cost-2026-10-03.md) records
real-session construction, representation switching, storage and fresh-process
CLI costs without changing silver. It does not close shared/fleet or quality gates.
The [bounded shared cost comparison](shared-gold-cost-2026-10-03.md) now measures
the explicit dense/lexical-only/dense rebuild path on the same certified sample,
including authenticated HTTP searches and provider filtering. Silver hashes and
source revisions remain unchanged. Server/backend resources, steady-state
storage after compaction, fleet latency and held-out quality remain open.

Query resource selection does not change
build identity or vectors. CLI `search --ranking fusion` or
MCP `search` with `"ranking":"fusion"` overrides ranking for one request;
`"contextual-rerank"` is also accepted. Omission inherits current TOML, including
worker and MCP sessions. Overrides do not persist and a later request returns
to current defaults. Explicit ranking overrides with exact-mention fail before
model admission, SQLite or HTTP access because literal retrieval does not use
hybrid ranking. Unknown ranking names fail at the typed CLI/JSON boundary.

A single Linux x86_64 CI-profile run on 2026-10-03 measured backend acquisition
(inventory validation, lease selection, cache lookup and model initialization)
with the installed MiniLM/Jina assets:

| Acquisition sequence | Load time | Reranker loaded |
| --- | ---: | --- |
| First fusion backend | 274.9 ms | No |
| Reuse fusion backend | 0.8 ms | No |
| New contextual backend after fusion | 818.2 ms | Yes |
| New fusion backend after contextual | 209.2 ms | No |

These are individual observations, not percentiles or cold filesystem-cache
measurements. They exclude query inference, retrieval, reranking and process
startup. Switching policy reinitializes embeddings; stable-policy requests
reuse the sole idle backend. RSS, sustained mixed-policy traffic and complete
corpus ingestion/search costs remain acceptance work. The executable evidence
is `ranking_selects_only_required_models_and_restores_contextual_backend`; its
successful output is retained outside Git in the model-selection integration log.

The HTTP server separately accepts a strict shared-candidate query policy:

```toml
[gold.query.vector_search]
algorithm = "exact" # Default; or "hnsw" for approximate dense candidates.
# ef_runtime = 200 # HNSW only, integer 1..4096; default 200.

# Optional complete default for one configured group:
[gold.query.groups.personal]
algorithm = "hnsw"
ef_runtime = 80
```

This selects Qdrant query parameters against the existing physical collection;
it is not an index-construction recipe. It is excluded from build manifests,
model generations and group build activation. Each group's selected policy is
persisted alongside its catalogue. Start with `serve --activate-query` to
intentionally replace an existing query default; a conflicting startup without
that flag fails. New groups select the configured policy on first bootstrap.
Changing query defaults neither recomputes embeddings nor changes canonical silver. Exact
lexical retrieval remains unchanged. The old top-level `qdrant_vector_index`
table is rejected rather than silently converted. Connection credentials,
snapshot budgets and `qdrant_memory` remain deployment settings. This server
query default applies to groups without an explicit `gold.query.groups.GROUP`
override. Unknown groups and invalid policies are rejected before credentials
or database access. All API handles read the persisted policy for each hybrid
search, so a running handle configured with an older query default observes the
selected policy without restarting. Status exposes `group_gold.vector_search`.
Older catalogue records without this field have the explicit exact default;
select HNSW with intentional query activation if needed.

Query activation preserves the gold version, build/model identity and immutable
node references. It changes only policy and the catalogue CAS publication token,
so delayed writers cannot restore an old policy. Later node publications and
gold build activations preserve that policy. Selection is atomic per group;
startup across multiple groups is not one transaction. The additional authority
read and concurrent-request timing remain part of the shared cost gate.
CLI and MCP now accept a typed `shared_vector_search` request override. It applies
only to dense shared candidates; local dense retrieval and shared lexical
candidates retain their current policies. Omission inherits the group's current
persisted default, including after an earlier request supplied an override.
An override changes neither group policy nor silver, vectors or gold build
identity, and never triggers indexing. Configurable physical construction remains
follow-up work under `mash-q3e.3.7`.

```bash
mash search 'database configuration' --shared --intent related-discussion \
  --shared-vector-search hnsw --shared-hnsw-ef 80
```

For MCP, supply `"shared_vector_search":{"algorithm":"hnsw","ef_runtime":80}`
or `{"algorithm":"exact"}`. The corresponding HTTP `POST /v1/search` field is
`vector_search`. HNSW effort defaults to 200 and is bounded to 1..4096; exact
does not accept an effort field. Unknown algorithms/fields fail validation.
Overrides with exact-mention, local-only search or no configured shared index
are rejected before model admission. Explicit overrides also check authenticated
shared capabilities before admission, including for combined searches, so
lexical-only shared gold cannot silently ignore a vector request. Such requests
need the shared API to be available at preflight; ordinary combined requests
without an override retain their existing local-fallback behavior. Stale or
incompatible gold is still rejected by the backend.

With diagnostics enabled, `shared_vector_override` reports the requested
override when supplied. It does not claim remote retrieval completed and does
not report an inherited default. Exact remains the unchanged default. Increasing
HNSW effort can trade latency for recall; this feature does not establish quality
equivalence to exact search or close the held-out shared cost/quality gates.

Node query defaults (intent, result unit/limit and fusion) retain their separate
schema; they are not accepted as server query fields.

The per-group persistence milestone passed 673 standard tests, 22 disposable
Qdrant backend tests and two HTTP gold-rebuild tests, plus strict all-target
Clippy, formatting and doctests. Tests compare complete retained snapshot bytes
before/after query activation, preserve node references and build identity,
read through independently opened handles, isolate groups, reject invalid
policies and fence a delayed catalogue write. Per-request vector controls are
tracked separately in `mash-q3e.4.2.2.2`; the shared cost gate remains open.

The request-override milestone passed 677 standard tests and 26 disposable
Qdrant/HTTP/installed-model contracts, plus strict all-target Clippy, formatting
and doctests. Direct CLI and worker requests and the real MCP protocol forward
typed policies and restore omission on subsequent requests without model assets
or local database changes. HTTP checks preserve group defaults and retained
state, reject invalid/unauthorized requests, and keep stale-gold guards intact.
An installed-model combined search preserves local bytes, shared freshness,
filters and thread-copy deduplication under both overrides. Transport fixtures
use deterministic local embedding responses; they do not evaluate relevance.
Shared cost and held-out ANN quality remain separate gates.

Validation before request overrides on 2026-10-03 passed 642 standard tests and 55 real-model/Qdrant
contracts. Weighted lexical/semantic preferences changed candidate ordering on
deterministic fixtures. Real-model direct and cached CLI searches reflected
fourfold uniform weights while the database remained byte-for-byte unchanged.
Combined retrieval retained concurrency and equal weighted local results on
shared failure. Default-policy relevance and Qdrant contracts passed. Logs are
`gold-fusion-standard-current-20261003.log` and
`gold-fusion-models-qdrant-20261003.log` in the external artifacts directory.
This validates policy wiring and default preservation; it does not establish
that nondefault weights improve held-out relevance or representative latency.

Request-override validation on 2026-10-03 passed 645 standard tests, 56
real-model/Qdrant contracts, strict all-target Clippy, formatting and doctest
compilation. Real MCP SDK calls inherited partial defaults after TOML reload,
returned to defaults on later requests, and left the database byte-identical
with provider files removed. Direct and cached CLI calls exercised the same
override/reset behavior; invalid requests failed with model capacity unavailable
without creating storage or model paths. MCP schemas advertise the numeric
bounds, and CI includes the real-model MCP contract. Captures are
`gold-fusion-overrides-standard-20261003.log`,
`gold-fusion-overrides-models-qdrant-final-20261003.log` and
`gold-fusion-overrides-doc-20261003.log` in the external artifacts directory.
This verifies query-only execution, not a quality or performance advantage for
particular nondefault weights.

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

## Query diagnostics

Opt-in CLI `search --diagnostics` and MCP `search` with `"diagnostics":true`
include a typed, request-only `diagnostics` object. Policy version 1 reports
resolved intent, unit, limit and hybrid fusion; policy-selected artifacts;
requested local/shared/combined scope; successfully completed retrieval sources;
shared fallback; returned result count; and core elapsed milliseconds. Exact
search selects metadata and lexical artifacts and omits fusion. Related
discussion selects metadata, lexical and dense artifacts. Selection describes
the resolved policy, not measured inference calls or candidate counts.

Core timing includes configuration and model admission, but excludes process
startup, worker connection/queue time before admission and response serialization.
Zero-result successful retrieval still counts as a completed source. A combined
shared failure reports local completion and fallback; failed shared-only calls
remain errors. Reports contain no source text, credentials, paths or model URLs.
Diagnostics do not change ranking, run indexing or persist query policy. They
are omitted by default. Artifact readiness/coverage, filter-stage counts,
metadata/topic diagnostics and held-out attribution remain `mash-q3e.4.4` work.

Validation on 2026-10-03 passed 645 standard tests, both real-model CLI/MCP
contracts, strict all-target Clippy, formatting and doctest compilation.
Exact local, shared, combined and fallback reports were verified without model
capacity; real-model direct/cached CLI and SDK MCP reports reflected current
defaults and overrides while the database remained byte-identical. Subsequent
ordinary calls omitted diagnostics. Captures are
`gold-search-diagnostics-standard-final-20261003.log`,
`gold-search-diagnostics-models-20261003.log`,
`gold-search-diagnostics-clippy-complete-20261003.log` and
`gold-search-diagnostics-doc-20261003.log` in the external artifacts directory.
These checks establish policy/outcome reporting, not held-out relevance or
representative end-to-end performance.

The report also measures bounded request-local `stages`, independently of
`MASH_TRACE`. Each stage contains `calls`, `completed_calls`, inclusive
`total_us`, longest-call `max_us`, and optional aggregate `items_in/items_out`.
Timing overlaps across concurrent local/shared work; sums are not wall time.
Counts cover successful calls that report counts, not necessarily unique
messages. A successful empty retrieval reports zero; an incomplete call leaves
its output count unreported. Aggregate counts cover reported calls only.
Incomplete calls can reflect errors or cancellation.
Stages never entered are absent. At most eight stage records are retained.

| Stage | Input count | Output count |
| --- | --- | --- |
| `local-exact`, `local-lexical` | Not reported | Filtered message candidates before result grouping |
| `local-dense` | Eligible stored chunk vectors scanned | Hydrated message candidates |
| `shared-candidates` | Not reported | Lexical plus semantic candidate entries; duplicates may occur across streams |
| `context` | Selected evidence anchors | Completed contextual documents; local and shared work included |
| `embed` | Texts submitted | Embeddings returned |
| `rerank` | Query/document pairs submitted | Model scores returned; includes thread and passage passes |
| `fusion` | Candidate entries supplied | Deduplicated entries before later diversification/combined ranking |

Stage timing omits configuration, model admission/loading, storage setup and
result serialization/grouping; core elapsed time retains the in-core work.
The collector propagates into blocking workers and remains isolated across
concurrent calls, without installing or replacing tracing subscribers.
Model-free exact search reports no embedding, dense or reranking stage.
It can report `fusion`: literal ranking reuses lexical deduplication and fixed
reciprocal ranks, while remaining independent of configurable hybrid weights.

`completed_sources` now uses successfully measured retrieval, including empty
retrieval. `shared_fallback` specifically denotes candidate retrieval falling
back locally. `shared_context_incomplete` distinguishes later context degradation:
successful shared candidates remain a completed source even when remote context
fails. Ordinary `shared/shared_error/realized_mode` now use a typed candidate
outcome independently of diagnostics. Successful shared retrieval reports
`shared = true` and the merged/shared mode even if context is incomplete;
`shared_context_error` carries that later degradation. `shared_error` remains
candidate failure with local fallback; shared-only candidate failure remains
an error. Both optional error fields are omitted when absent. Artifact readiness,
coverage, filter-stage counts and metadata/topic diagnostics remain parent work.

The ordinary outcome correction passed 647 standard tests, four real-model
contracts, strict all-target Clippy, formatting and doctest compilation on
2026-10-03. The context-failure matrix verified combined/shared-only modes
with diagnostics enabled and disabled, retaining identical hits and unchanged
index bytes. Existing direct/cached CLI, SDK MCP and transport-fallback/concurrent
retrieval contracts passed. Captures are `gold-shared-outcome-standard-20261003.log`,
`gold-shared-outcome-models-20261003.log`,
`gold-shared-outcome-clippy-complete-20261003.log` and
`gold-shared-outcome-doc-20261003.log` in the external artifacts directory.
This corrects source status; it does not change retrieval, build identity or silver.

Stage validation on 2026-10-03 passed 647 standard tests and four real-model
contracts, strict all-target Clippy, formatting and doctest compilation.
Concurrent task/blocking measurements remained isolated and retained fixed
storage after 1,000 calls. Direct/cached CLI and SDK MCP reported actual local
candidate and inference counts with unchanged index bytes. Shared-context
failure retained peer evidence and completed shared retrieval; transport
failure and blocked SQLite preserved existing fallback/concurrency behavior.
CI includes the context-degradation regression. Captures are
`gold-search-stage-standard-final-20261003.log`,
`gold-search-stage-models-complete-20261003.log`,
`gold-search-stage-clippy-complete-20261003.log` and
`gold-search-stage-doc-20261003.log` in the external artifacts directory.
These tests verify instrumentation and default preservation; representative
costs and held-out relevance remain separate gates.

## Active local build recipe and manifest

```toml
[gold.build]
embedding_text = "dialogue" # default; alternative: "title-and-dialogue"
# Optional dense input budget; omitted means no additional truncation:
# max_embedding_chars = 4096 # integer, 256..65536 Unicode scalar values
```

The default preserves the existing visible-dialogue embedding input. The second
recipe prefixes each visible message with `Thread: <title>` and a blank line.
Titles are trimmed and bounded to 256 Unicode characters. Missing or blank titles
leave the dialogue unchanged; hidden records never become searchable through a
title. Deduplication runs after preparation, so equal messages in differently
titled threads can have distinct representations. This is an opt-in contextual
representation, not a demonstrated search-quality improvement.

`max_embedding_chars` limits only the prepared dense embedding input, including
any title prefix. It trims surrounding whitespace and keeps the first N Unicode
scalar values, without splitting UTF-8 characters. It is not a token or byte
budget. When enabled, the title contributes at most one quarter of the budget
(and never more than 256 characters), leaving room for dialogue. Canonical silver,
lexical matching and message views retain their full text. Hidden records remain
hidden. Local and shared builds can use different budgets.

A budget change has a distinct derived generation and requires an explicit gold
rebuild from stored silver. Omission retains the existing input, serialized build
identity and default behavior. Smaller inputs may reduce work but can lose dense
evidence near the end of a message; no representative quality or speed gain is
claimed. The active budget is included in local and shared build manifests.

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

### Configurable dense text preparation

Local node and shared server TOML independently accept:

```toml
[gold.build]
text_preparation = "collapse-whitespace"
# embedding_text = "title-and-dialogue"
# max_embedding_chars = 512
```

The default `preserve` retains current embedding input and generation identities.
`collapse-whitespace` splits the prepared embedding text on Rust's Unicode
whitespace boundaries, rejoins nonempty spans with one ASCII space, and removes
leading/trailing whitespace. It runs after optional bounded title enrichment and
before the optional character budget. Thus tabs, newlines, indentation and
nonbreaking spaces are collapsed in model input. Canonical content, interpreted
silver dialogue, lexical indexing text and displayed passages remain unchanged.
Empty dialogue remains empty even when a title exists. Lexical-only builds retain
the configuration without doing dense preparation or inference.

The version-1 whitespace generation includes the embedding-text recipe, budget
and model generation; manifests also retain the typed setting. Changing it
requires explicit gold rebuilding and never triggers indexing from search.
Local and shared preparation consume stored silver through the same function.
Reverting to `preserve` replaces the active projection rather than maintaining
another profile. Older stored recipes lacking this additive field resolve to
`preserve`; services and clients must understand the new setting before publishing
it. Query settings and deployment configuration do not affect this identity.

This is opt-in configurability, with no claimed retrieval-quality gain. Collapsing
code indentation may remove useful structure. Held-out quality and representative
storage/build/search cost comparisons remain open; content selection and new
retrieval units are also separate unfinished work.

### Dense content selection

Local node and shared server TOML independently accept:

```toml
[gold.build]
dense_content = "user-messages"
```

The default `all-visible` retains existing behavior. `user-messages` embeds only
nonempty interpreted dialogue whose normalized silver role is `user`. Assistant,
system, developer, tool and unknown roles remain in canonical silver and lexical
search, but receive no dense vectors. This is a gold build policy, distinct from
the provider allowlists that control ingestion. It neither rewrites displayed
history nor removes messages or thread metadata.

The selection is recorded in the active manifest and dense generation together
with text preparation, title enrichment, character budget and model identity.
Switching requires an explicit gold rebuild from retained silver. Search never
builds it automatically, and request overrides cannot change dense eligibility.
Replacement keeps one active projection. Coverage and readiness count eligible
messages rather than every visible message; shared heads carry an optional
visible-user count. Legacy heads lacking this count cannot certify a user-only
build until rebuilt. Invalid counts and vectors for excluded messages fail closed.

A user-only index with no eligible messages completes without loading inference
assets. If eligible messages exist, missing assets still prevent dense completion;
canonical ingestion and lexical construction may already have succeeded. Related
search still needs query-model assets even when the index contains no dense
vectors. Exact-mention search remains available offline without model assets and
includes assistant messages.

Deploy clients and the HTTP service that understand this field before enabling
it; strict configuration decoding intentionally rejects unknown build settings.

Tests cover unchanged canonical history across replacement, mixed-role inference
batches, empty eligibility, source-free rebuilding, role edits, restart, forgetting
and rejected shared vectors without publication. This option has no claimed
quality advantage. Broader content selectors, new retrieval units and comparative
quality/cost evaluation remain tracked follow-ups.

### Message-anchored retrieval units

```toml
[gold.build]
retrieval_unit = "parent-and-message" # default: "message"
```

Both modes keep one dense vector per eligible anchor message. The optional
parent unit represents the anchor plus up to 2,048 Unicode scalar values of
its parent's interpreted dialogue. The result ID, role, time and thread refer
to the anchor. Lexical retrieval and viewing still use canonical message rows;
the parent input is not a new canonical message or a copied persisted gold body.

For native tree histories, the recorded parent ordinal chooses the context.
Roots have none. For linear histories, only ordinal minus one is considered.
Missing, hidden or role-excluded parents contribute nothing; there is no scan
back to another visible message and no context borrowed from another thread.
`dense_content="user-messages"` excludes assistant text from both anchors and
parent context. The anchor precedes the parent in the embedding input; title,
whitespace preparation and the overall character budget apply afterwards.
A short overall budget can therefore omit the parent entirely.

This build setting is independent of query result grouping into messages or
threads. Local and shared indexes may select it independently. Changing it
requires explicit gold rebuilding, not a search-time override. Local generation
`gold-parent-unit-v1:<anchor-recipe-generation>` versions the unit rule and cap;
shared manifests and the durable group build record bind the same typed recipe.
Default serialized recipes and model generations remain unchanged. Upgrade
clients and services together before publishing this new enum value.

All node materialization, SQLite snapshot preparation and retained shared
rebuilds use the same parent-resolution rules. Streaming preparation keeps a
private disposable SQLite lookup containing at most 2,048 characters per
eligible parent, with a 2 MiB SQLite page-cache budget and one thread retained
at a time. It runs on blocking workers; it is deleted after build or failure.
The default message mode creates no parent database. Local planning resolves
parents directly from the consistent silver snapshot. Local rebuilds invalidate
the non-default projection conservatively on silver revision changes; shared
reuse requires the complete thread fingerprint, including parent text and edges.

This is a configurable construction option, not a demonstrated quality win or
a new default. Multi-passage vectors, source-offset mappings, broader unit
selection, and held-out guided-search evaluation remain tracked follow-ups.
The bounded [local cost comparison](parent-unit-cost-2026-10-03.md) measured
19.443 s for parent replacement versus 12.684 s for message replacement over
the same 1,061 anchors, without changing canonical/provenance table hashes.

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

### Durable shared group configuration

Qdrant's state collection contains one strict typed group configuration point:
schema version, immutable configuration version, build recipe, model generation,
dimensions and a catalogue of visible node snapshot references. Schema 2 has a
separate publication token that changes on every catalogue update. Ordinary API
startup bootstraps an absent record once or requires
matching persisted settings. Conflicting TOML fails startup before listening.
Concurrent conflicting bootstrap uses insert-only publication; explicit
replacement conditions on both the configuration version and publication token.
Both paths reread durable state to verify the selected configuration. Matching
startup is idempotent and does not advance its version.

To intentionally switch, change the server TOML and run:

```sh
mash serve --server-config /absolute/path/to/server.toml --activate-gold
```

Activation changes group authority, without modifying canonical history or
re-embedding it. All API processes must run the authority-aware protocol;
older binaries do not implement these checks. APIs using this protocol with
old recipe/model settings fail reads and publication. `/v1/freshness` reports typed `group_gold` configuration metadata
alongside per-node manifests/readiness. Node and group reads check durable
authority; publication checks its version before staging, before the catalogue
CAS and after verification. A switch observed during publication prevents
acknowledgement. Malformed or unknown control schemas fail closed rather than
being overwritten, even with the activation flag. Shared operations add
control-point reads; there is no polling task, canonical-body copy or model
inference for authority checks. Representative latency/throughput costs remain
part of the broader measurement gate.

Rebuild every contributing node after activation. Dense search fails until the
group's manifests match; current lexical retrieval is independent of dense
readiness. This is **not an atomic all-node gold replacement transaction**.
Activation and node publication now conditionally replace the same catalogue
point, fencing delayed publications at storage. Different-node updates may rebase
metadata up to three times; same-node conflicts and configuration changes cannot
rebase. The catalogue contains at most 512 node references, without canonical
bodies or vectors. Recipe changes intentionally require explicit node rebuilds;
there is no candidate staging or uninterrupted all-node replacement feature.
Model-dimension migration and broader cost measurements remain separate work.
Activating several configured groups is per-group, not one cross-group transaction.

Existing schema-1 groups require explicit `--activate-gold` migration even when
their build settings match. Upgrade all API processes to the authority-aware
protocol before migration; binaries predating that protocol are unsupported.
Migration first seals the group against old writers, then adopts its immutable
snapshot references after checking that legacy heads are stable. Interrupted
migration fails closed and resumes with the same settings and flag. It does not
read provider files or re-embed history. A late unacknowledged legacy write cannot
change the active catalogue; its staging may require later reclamation.

Catalogue-fencing validation passed 641 standard tests, all 53 Qdrant contracts
with installed local models, and the separate real API/database graceful and
SIGKILL restart contract. A delayed conditional catalogue update after activation
could not remove the retained node reference; concurrent different nodes both
remained visible. Explicit schema-1 migration and interrupted sealing/resume
preserved snapshot versions and canonical fingerprints. Ten malformed control
records failed closed. Source-free CLI/MCP rebuild, recipe negotiation, relevance
and scoped-search contracts also passed. Logs are
`group-catalogue-standard-20261003.log`,
`group-catalogue-qdrant-final-20261003.log` and
`group-catalogue-restart-20261003.log` in the external artifacts directory.
These fixture timings are not representative corpus throughput or latency gates.

Before catalogue fencing, validation on 2026-10-03 passed 641 standard tests and 50 disposable-Qdrant
contracts. Conflicting first-start selections produced one winner; matching
reopen preserved its version; six malformed control records failed without
being overwritten. The real CLI contract verifies conflicting startup failure,
explicit activation, stale-process rejection and idempotent API restart.
A controlled activation immediately after node CAS rejects acknowledgement,
retains canonical history and exposes the old manifest as incompatible with
the new recipe. The separate real API/Qdrant graceful/SIGKILL restart contract
also passed with unchanged persisted freshness, including group configuration.
This does not prove power-loss durability, backup restoration or atomic group
replacement. Logs are `group-gold-authority-standard-final-20261003.log`,
`group-gold-authority-qdrant-final-20261003.log` and
`group-gold-authority-restart-20261003.log` under the external artifacts directory.

Routed ingestion negotiates the server's typed `gold_build` through `/v1/state`
and pins it in the upload start. Native and Claude materialization apply that
recipe to embedding inputs, while retaining canonical content and interpreted
dialogue unchanged. Changed source conversations are embedded in bounded batches;
compatible source checkpoints can skip unchanged history. A server configured
with a different recipe rejects the pinned upload before staging.

`/v1/state` also returns `publication_version`, the immutable node-head identity
(empty before bootstrap). Routed ingestion copies it to the typed start field
`expected_version`. The source publication counter alone cannot protect prepared
work against a gold-only replacement: that operation leaves the counter and
silver revision unchanged. The server compares the observed identity before
accepting source data, including duplicate snapshots; a mismatch returns a
retryable conflict without staging or acknowledging the upload. An explicit
empty pin only admits a still-absent head. The existing commit-time conditional
head update protects changes occurring after this preparation check.

The version already exists in the persisted head. For routed ingestion, exposing and pinning it adds
small JSON fields to the existing state response and upload, with no extra
request, inference or canonical-content copy. With the current 64-character head
identity, compact JSON adds 89 bytes to node state and 86 bytes to the start
record. Full-snapshot publication uses the same guard through the plan endpoint
below. This node-version pin is separate from the durable group authority
described above.

The server certifies a source upload when its retained vectors already have a
matching recipe/model manifest, or every remaining searchable conversation was
replaced by the upload. A partial recipe transition leaves dense gold unverified;
equal dimensions cannot prove representation compatibility. Complete replacement
or explicit retained-silver rebuilding recovers it. Empty bootstrap, idempotent
snapshots, compatible no-op refreshes and deletion of the last thread retain a
current recipe manifest. Local and shared recipes remain independent.

### Recipe-aware full snapshots

Full-snapshot publication first requests the authenticated node-scoped
`GET /v1/publication-plan`. Its strict typed schema includes the schema version,
active build, model generation/dimensions, immutable head version, source
instance/revision and eligible thread dependency fingerprints. Default dialogue
publication only retrieves the small head: it does not decode or fingerprint
shared history and continues forwarding existing local vectors unchanged, with
zero additional inference. Negotiation adds one HTTP round trip to that path.

For a nondefault shared recipe, eligibility requires a current recipe/model/rule
manifest and valid vectors for every visible message in a thread. The BLAKE3
dependency stream includes canonical conversation metadata/enrichment and every
message's identity, ordinal, context, role, canonical body, interpreted dialogue
and timestamp in thread/ordinal order. Hidden messages participate too. Vectors,
model identity and source-checkpoint stamps are excluded from the digest; their
compatibility is checked separately. This conservative dependency also invalidates
reuse for metadata-only changes. Changed threads are rebuilt as whole threads.

The node captures a consistent read-only SQLite silver snapshot. Nondefault
preparation does not read local embedding blobs or provider files. One active
thread hasher and lightweight conversation/fingerprint maps avoid retaining all
canonical bodies in RAM. Two restrictive temporary streams freeze the read
snapshot and prepared upload without holding a SQLite transaction during model
work; both disappear on drop. Inference batches contain at most 32 canonical
rows, remain inside a thread and count hidden rows before selecting visible
inputs. They match the retained-silver rebuild schedule. Unchanged eligible
threads carry canonical rows without vectors plus an explicit `reuse_gold`
start flag and `reuse_gold` events containing the thread ID and dependency digest.

The server checks the declared recipe and immutable head before parsing changes.
Reuse requires the explicit flag, a pinned snapshot and matching old/new thread
dependencies; unknown, changed, duplicated or ambiguously supplied reuse fails
before staging. Retained vectors move into a gold-only map while old
canonical rows are discarded; no second old canonical body copy is retained.
Server digest ordering uses an array of borrowed message references, not cloned
bodies. Existing snapshot byte/admission budgets and final conditional head
publication still apply. Interrupted or stale uploads cannot acknowledge local
publication. A recipe refresh at the same source counter is allowed only when
the complete canonical fingerprint remains unchanged; ordinary duplicates retain
their accepted vectors.

CLI/MCP indexing provide model resources through the same core. Forget may reuse
existing shared vectors, but never initiates model work: if other unpublished
threads require preparation, publication stays pending until explicit indexing.
The local deletion remains durable. `shared.preparation` reports reused threads/
vectors, forwarded vectors, model inferences, model loading/embedding/preparation
milliseconds and upload bytes. These are operation diagnostics, not CPU/RSS or
corpus throughput measurements. Group selection now has durable authority;
group switches use explicit per-node rebuilds, and representative
node/server storage, inference and publication-cost acceptance remains `.5`.

The 2026-10-03 unfiltered CLI fixture indexes one Codex and one Claude message,
switches the shared recipe over unchanged silver, reuses both unchanged threads,
edits one thread and compares its scores with an explicit retained-silver rebuild.
It then removes both provider files and forgets one thread without inference.
Canonical local rows and vectors remain unchanged during the recipe switch.
The debug Cranelift diagnostic on Xenia reported:

| Operation | Complete CLI time | Shared preparation | Shared model inferences | Reused vectors | Upload bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial dialogue publication | 1,847 ms | 12 ms | 0 | 0 | 5,770 |
| Switch to title-and-dialogue | 1,723 ms | 1,555 ms | 2 | 0 | 5,886 |
| Unchanged title-and-dialogue refresh | 96 ms | 17 ms | 0 | 2 | 2,008 |
| Edit one thread | 3,293 ms | 1,412 ms | 1 | 1 | 3,924 |

Shared model loading accounted for 1,506 ms during the switch and 1,380 ms
during the edit; shared inference took 16 ms and 8 ms respectively. Complete
CLI time also includes local indexing and HTTP publication. These are single
samples from a two-message correctness fixture under host load, not corpus
throughput, memory, relevance or percentile measurements. The capture is
`shared-snapshot-recipe-qdrant-ci-20261003.log` in the external artifacts directory.

Validation for this full-snapshot increment passed 640 standard tests and all
46 disposable-Qdrant contracts. An earlier parallel default-profile run timed
out in the existing Claude-copy reconciliation fixture; that test passed in
isolation and in the complete serial CI-profile run. The owned persisted-restart
contract also passed, including graceful and SIGKILL recovery. Its gold-rebuild
assertion checks that the immutable publication version advances while the
remaining node state stays identical. This is process-restart evidence, not
power-loss, failover or backup/restore validation. Captures are
`shared-snapshot-recipe-standard-postlint-20261003.log` and
`shared-snapshot-recipe-restart-final-20261003.log` outside Git.

Retained-silver rebuilding now matches the routed inference schedule: threads
are separate, rows follow ordinal order, and hidden rows count toward each
32-row boundary before visible embedding inputs are selected. The two-provider
integration test initially exposed different scores when rebuilding batched
across threads; matching the schedule restored exact score equality for the
fixture. This matters for batch-sensitive quantized inference. Sorting moves
retained records into a vector without cloning canonical bodies; it adds a
sorting step and can reduce cross-thread batch utilization. Full-corpus rebuild
throughput remains a measurement gate.

The Xenia debug CLI fixture used one searchable Codex message and one Claude
message, independent local dialogue/shared title-and-dialogue recipes, unchanged
refresh, edits to both providers, retained-silver rebuild comparisons and
deletion of both threads. Initial indexing took 2,919 ms; an unchanged refresh
102 ms; the Codex edit 3,093 ms; deletion 149 ms. These are small-fixture,
process-inclusive correctness diagnostics with cold model loading, not corpus
percentiles or quality evidence. Logs are under the external artifact directory
in `shared-recipe-batch-parity-20261003.log`.

Earlier routed-path validation passed 634 standard tests and 43 disposable-Qdrant contracts,
including the two-provider lifecycle, both recipe guards, source-free rebuilding,
exact/hybrid queries, concurrent publication and graceful/SIGKILL restart. The
final Qdrant run is captured in `shared-recipe-ingestion-qdrant-final-20261003.log`.
These checks preserve the broader pending acceptance gates described above.

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
Qdrant graph topology remains fixed at HNSW `m=16`, `ef_construct=200`.
Query-time exact/HNSW controls choose how to search those existing vectors;
there is no physical-generation replacement framework or topology build option.

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
