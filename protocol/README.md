# Mash protocol checks

Small TLA+ state machines describe persistence boundaries that are difficult
to exercise through examples alone. TLC explores every reachable state in each
configured finite model, checking safety and, where stated, progress after
failures stop. These are manually maintained abstractions, not generated Rust
specifications or proofs of the backing services.

## Run

Requires Bash, Java 11+, GNU `timeout`, `sha256sum`, and `curl` for installation.
The checker is the fixed [TLA+ tools v1.7.4 release](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4),
verified against SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

```bash
make protocol-tools       # Explicit download; no Cargo or model assets involved.
make protocol-check       # Six models plus eight expected counterexamples.
```

`make protocol-check` uses the repository's `DEV_RUN` launcher (`agent-test` on
Xenia). TLC uses one worker, a 1 GiB Java heap and a 120-second timeout per
configuration. This is independent of Rust compilation and disposable Qdrant
services. CI runs it as a separate job, also required by the deployment gate.

For a focused check on Xenia:

```bash
agent-test bash protocol/check.sh SharedViews
agent-test bash protocol/check.sh --self-test
```

Elsewhere, omit `agent-test` if unavailable. On macOS install GNU coreutils and
put its `gnubin` directory on PATH. Override `MASH_PROTOCOL_ARTIFACTS` to choose
an output directory outside the checkout. The default on Xenia is
`/home/james/scratch/mash-artifacts/protocol`; elsewhere it is
`${XDG_CACHE_HOME:-$HOME/.cache}/mash/protocol`. Each run prints its unique log
directory and copies the exact checked model/configuration there. Java temporary
files, TLC states and counterexamples stay in that directory. Checks never
download tools implicitly and reject an unexpected jar checksum.

An expected counterexample succeeds only with TLC's safety-violation exit code
and the named invariant violation. Syntax errors, timeouts, unexpected
invariants and missing tools fail the command. Negative configurations deliberately
break the modeled contracts, including the historical replay-ordering bug below.
A green suite establishes properties of these bounded abstractions only.

## Models and source boundaries

### Publication: preserve edits made during an upload

[Publication.tla](Publication.tla) separates local edits, consistent snapshot
capture, the remote silver commit, loss of a response, and durable local
acknowledgement/journal pruning. Bounds: one instance, one unchanged target,
three local publication revisions and one in-flight publication.

Checks: acknowledgement never exceeds the committed remote revision; pending
edits remain in the journal; its pruning floor matches the acknowledgement in
this single-target model. Once edits/failures stop, fair publication steps
eventually acknowledge the latest revision. `PublicationAckLatest.cfg` models
the mistake of acknowledging the latest local revision instead of the revision
captured for the upload. TLC catches an edit between capture and acknowledgement.

Source mapping:

- [shared/publication.rs](../app/shared/publication.rs): `publish_result`,
  `acknowledge`, `already_published`.
- [storage/publication.rs](../app/storage/publication.rs):
  `with_publication_snapshot`, `acknowledge_publication`.
- Existing regressions:
  [journal preservation](../app/storage/tests/publication.rs) and
  [lost-ack recovery](../app/shared/tests/publication_recovery.rs).

The successful freshness recovery is abstracted as a transition with all
identity/source/artifact checks already satisfied. This does not verify the
two-read HTTP observation algorithm, target changes, journal delta selection,
server restores, multiple publishers, or concurrent remote forget operations.
It does not equate a durable local publication acknowledgement with perpetual
remote view readiness. Publication revision is not canonical silver revision.

### NativeSilver: revision guards and deletion intent

[NativeSilver.tla](NativeSilver.tla) chooses snapshot/delta requests with valid
or invalid ownership, bases and revision ordering, and allows abort before the
atomic silver commit. Bounds: three transport revisions, two native chat IDs,
one node, one provider namespace, one in-flight transaction. Same-revision
snapshots leave silver unchanged, and later snapshots retain prior tombstones.

Checks: the accepted revision never regresses, and retained records never
intersect tombstones. `NativeSilverNoTombstones.cfg` still applies immediate
forget deletion but ignores retained tombstones on subsequent uploads; TLC
finds resurrection.

Source mapping:

- [silver_store/apply.rs](../app/shared/silver_store/apply.rs): `start`,
  `apply`, `Upload::forget`, `Upload::conversation`.
- [silver_store.rs](../app/shared/silver_store.rs): `SilverStore::write`.
- Existing regression:
  [canonical revision and atomic publication](../app/shared/server/tests/silver_revision.rs).

Chat bodies, source certificates, canonical hashing, ordinary `Remove` events,
provider-qualified identity and connector isolation are outside this model.
Delta requests add chat IDs and/or forget them; replacement bodies are abstracted
away. Aborted transactions are atomic rollback, not a model of SQLite internals.
Invalid inputs have no committing transition; input decoding remains a Rust test
responsibility. This model makes no delivery/progress claim.

### ConnectorReceipts: atomic data, ordering and retry identity

[ConnectorReceipts.tla](ConnectorReceipts.tla) checks a source-scoped batch
receipt before its expected revision, commits data/order/receipt together, and
allows a crash on either side of that transaction. Bounds: one source, two batch
IDs, two payload choices, one canonical object and one active transaction.
Requests can retry any batch, including after later commits, or reuse its ID
with different content/expected revision.

Checks: exactly one commit per batch ID, atomic receipt existence, contiguous
receipt revisions, canonical object agreement with the latest commit, durable
response revisions and fingerprint agreement. Fair processing after failures
stop eventually resolves an active request. `ConnectorWrongPayload.cfg` removes
the fingerprint check and demonstrates a successful reply for the wrong batch.

Source mapping:

- [connectors/silver.rs](../app/connectors/silver.rs): `Store::apply`, `apply`.
- [server/store/connectors.rs](../app/shared/server/store/connectors.rs):
  `ingest_connector` commits silver before best-effort recertification.
- Existing regressions in [silver/tests.rs](../app/connectors/silver/tests.rs):
  `committed_records_and_receipt_survive_lost_response_reopen_and_later_batches`,
  `changed_batch_identity_and_stale_source_revision_never_mutate_records`,
  `failure_to_record_receipt_rolls_back_data_and_ordering`.
- [Generated SQLite sequences](../app/connectors/silver/tests/properties.rs)
  check independent expected objects, revisions and receipts across two sources,
  retries, rejected writes, reopening and archive restoration.

Fingerprint equality represents equality of the entire validated batch, not
just text; hash collisions are excluded. Object graph validation, multiple
sources, rollback implementation and connector view digests are not modeled.
A receipt confirms silver durability, never view readiness. Progress means
resolution, which may be rejection; it does not guarantee eventual client retry.

### SharedViews: separate data writes from certification

[SharedViews.tla](SharedViews.tla) models a native silver commit, invalidation,
destructive replay, individual point writes/deletes, and certification as
separate steps. A crash preserves whichever durable operations have finished.
Bounds: two objects, two canonical revisions, two settings identities and one
writer. Settings change only at a quiescent owner restart. `Current` is computed
from persisted revision/settings markers, independently of the worker phase,
so checks can detect metadata lying about physical documents.

Checks: reported-current views contain exactly the modeled silver projection
under the active settings; fair repair eventually restores currentness after
edits, settings changes and crashes stop. Missing embeddings are abstracted as
unfinished point writes; certification follows only completed work.

Source mapping:

- [server/store/sync.rs](../app/shared/server/store/sync.rs): `index`,
  `sync_upload`, `certify`.
- [server/store/replay.rs](../app/shared/server/store/replay.rs): `replay`.
- [qdrant_index/views.rs](../app/shared/qdrant_index/views.rs): `NodeViews::current`,
  `put_node_views`, `delete_node_documents`.
- [qdrant_index/configuration.rs](../app/shared/qdrant_index/configuration.rs):
  `select_views`, `active_configuration`.
- [server/store.rs](../app/shared/server/store.rs): per-node `lock`;
  [silver_store.rs](../app/shared/silver_store.rs): exclusive directory ownership.
- Existing regressions in [recovery.rs](../app/shared/qdrant_index/tests/recovery.rs)
  and [recipe.rs](../app/shared/qdrant_index/tests/recipe.rs).

**Fixed implementation gap: `mash-8qaj`.** `Store::replay` now persists empty
certification before clearing documents, matching the passing configuration.
`SharedViewsReplayClearFirst.cfg` deliberately restores the historical order
and must violate `NeverFalselyCurrent`:

1. Start with matching silver/view metadata and two documents.
2. Begin replay without changing silver.
3. Delete the documents. Old metadata still says current.

A crash or failed empty-metadata write at step 3 preserves the bad state. This
does not require HTTP readers during startup replay: the next ordinary startup
can observe the persisted mismatch. The real-Qdrant regression
`qdrant_replay_interruption_never_leaves_current_metadata_and_retry_repairs`
failed on this old ordering. It interrupts the actual awaited boundaries after
invalidation and deletion, verifies stale metadata through a fresh service
handle, and checks unchanged silver and successful retry. This is simulated
boundary failure, not process termination or power-loss verification. No TLA+
action change was needed: the passing model already described the corrected
order; the negative configuration is now labeled as a historical mutation.

No all-time physical-subset guarantee is claimed while views are stale: in-place
updates permit old documents until deletion completes. This model also excludes
search response snapshot consistency, connector digests, independently restored
Qdrant metadata/data, arbitrary delayed writes after a process crash, and
multiple servers with different silver directories sharing one Qdrant namespace.
Successful operations are assumed durable and ordered at the modeled boundary;
there is no proof of storage engines or network cancellation semantics here.

### Retry recovery source mapping

[RetryViews.tla](RetryViews.tla) adds the retry details that `SharedViews` abstracts
away. It bounds the system to one native chat, a silver deletion committed before
views synchronization, one retry and three possible supplied contents (absent,
retained, changed). Certification uses silver identity without inspecting physical
documents. Safety requires payload validation and removal of absent chats.
This is a safety-only model; it makes no liveness or concurrency claim.

- `silver_store/apply.rs`: duplicate snapshots are validated through the ordinary
  transactional ingestion path inside a rolled-back savepoint. The resulting
  canonical fingerprint must equal retained silver; vectors remain outside it.
- `server/store/sync.rs` and `qdrant_index/views.rs`: synchronization removes native
  documents whose chats are absent from silver, including points left by a failed
  request whose in-memory deletion set or metadata was lost. Connector points
  are excluded from this deletion by the provider filter.
- `qdrant_index/tests/retries.rs`: backend regressions for altered/incomplete
  retries and forgotten documents surviving retry, plus writer serialization.

The negative configurations restore the reproduced bugs `mash-vcfn` (accept
changed payload) and `mash-w8q7` (lose deletion intent). The original `SharedViews`
model required `docs = Desired` before certification; it did not verify that the
implementation's per-chat metadata was sufficient to establish that condition.
This additional model separates the supplied contents from retained silver.
Multiple messages, partial point writes, connector records and cryptographic
collision resistance remain Rust/backend test or review responsibilities.

### Connector views after interrupted writes

[ConnectorViews.tla](ConnectorViews.tla) explores up to three connector commits
for one anchor (absent or either of two contents), with interruptions before or
after point writes and before persisted digest completion. Physical point contents
and recorded digests are independent variables. TLC first found an interrupted
insert followed by deletion; after correcting cleanup it found a completed insert,
interrupted deletion and restoration of the original content.

The independent Proptest backend generator in
`app/shared/qdrant_index/tests/connector_sequences.rs` reproduced and shrank both:
two operations for `mash-9b7s`, three for `mash-5avn`. Named regressions retain those
sequences; bounded generation also exercises lexical-only and dense configurations
using a controlled embedding endpoint. `make property-backend-check` runs them,
and the existing CI Qdrant selection includes them.

Source mapping: `server/store/connectors.rs::sync_connectors` compares digests,
invalidates evidence for changed/removed anchors and clears the silver marker
before physical mutation; `qdrant_index/views.rs::retain_connector_points` removes
all absent anchors using a node-scoped connector filter. Successful completion
persists new digests and certification. Negative configurations restore missing
physical cleanup and retained stale evidence respectively. Hash equality abstracts
full unit equality; multiple anchors, arbitrary late remote writes, process kill
and power-loss durability are outside this model. The runtime hook simulates the
boundary after completed point writes, not those other failure classes.

## Maintaining the models

Review the source mappings whenever their protocols change. Match atomic steps
to actual transactions and split separate awaited writes; preserve real failure
windows. Update models, configurations, mappings, bounds and assumptions in the
same change, or explain why the existing abstraction still applies. Model
agreement is a review obligation, not something TLC checks against Rust.

The models with liveness properties have a one-way `stable` environment flag.
It stops external changes/failures; weak fairness then requires continuously
enabled processing to run. Stabilization itself is not assumed to occur:
progress is conditional, and safety is still checked before stabilization.
No state constraints, symmetry reduction or depth cutoffs truncate these runs.
`CHECK_DEADLOCK FALSE` permits intentional quiescent terminal states; it is not
a deadlock-freedom claim. Bounds live in the modules/configurations and must be
reviewed when extending them.

For new contracts, start with the smallest meaningful state space, add an
invariant independent of the implementation transitions, and include a named
broken-behavior configuration that demonstrates the invariant can fail. Turn
useful traces into Rust boundary regressions. Do not weaken a property merely
to obtain a green run; file and label implementation gaps explicitly.
