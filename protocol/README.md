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
make protocol-check       # Eleven models plus twenty-two expected counterexamples.
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

Shared activity reporting is observational: progress records follow the existing
admission-owned worker, and never gate work or change retries, transaction
boundaries or certification ordering. Its cancellation and group-isolation
regressions use Rust tests; no protocol-model transition changes are required.

## Models and source boundaries

### MutationAdmission: bounded FIFO waiting and worker ownership

`MutationAdmission.tla` models four requests, one or two active slots and two
waiting positions. `server/admission.rs` uses Tokio's fair semaphore for active capacity
and a separate semaphore to bound waiting handlers. `Arrive` models immediate
admission, joining the queue or overflow rejection. `Grant` models the FIFO
acquisition; receiving includes a reserved wake that the handler has not yet
polled. `CancelBeforeWork` covers wait expiry and dropping a queued/receiving
handler. `StartWork` transfers ownership to `admission::run`; `Disconnect` must
not release its permit, and `Finish` covers either worker success or failure.

Checks bound workers/waiters, prevent overtaking and require active workers to
own permits. Queued requests resolve assuming weak fairness of semaphore grants,
body receipt/worker start, and work completion. This is not a wall-clock latency
guarantee: the implementation has a five-minute waiting deadline, and neither the
model nor FIFO promises equal throughput across nodes or body sizes. HTTP socket
closure is not assumed to immediately cancel a handler. Rust tests verify FIFO,
overflow, timeout/cancellation cleanup, phase reporting, a real HTTP connector
request completing after waiting, and detached worker ownership. Per-node locks,
SQLite and Qdrant durability remain covered by their existing models/tests.
The deadline was increased after a 32-publisher mixed-workload trial took
104–151 seconds and forced repeated queue expiry at 60 seconds. The model and
TLC configurations are unchanged: expiry is already a nondeterministic
`CancelBeforeWork`, and the model makes no numerical timing assumption.

`MutationAdmissionReleaseWorker.cfg` deliberately releases active capacity on
disconnect and must violate `WorkOwnsCapacity`. `MutationAdmissionUnfair.cfg`
permits taking a non-head waiter and must violate `FairOrder`.

### RuntimeUpgrade: stop admitting writes after observing replacement

[RuntimeUpgrade.tla](RuntimeUpgrade.tla) models one old runtime, two executable
identities and at most two admitted operations. It separates executable
replacement, admission after the MCP mutation gate, an idle/standby watch poll,
and completion of an admitted operation. A replacement can occur after admission
and before completion: old writes may finish. The check is not installer fencing.
Safety checks require admission to observe the running identity and shutdown to
preserve admitted work. With fair polling and operation completion, the CLI
watcher eventually stops after replacement. MCP itself stays available for reads;
the polling/liveness portion applies only to CLI watching.

Source mapping: [runtime.rs](../app/runtime.rs) `Lease::ensure_current`,
[mcp.rs](../app/mcp.rs) `Mash::perform_response`, and
[cli.rs](../app/cli.rs) `run_parsed`. Registry liveness uses held OS locks;
filesystem metadata identity, lock correctness, successful registration and
eventual operation completion are assumptions, not modeled proofs. Older binaries
without these checks are outside the contract. The model excludes source data,
the filesystem's individual metadata reads, process crashes and reads under an
older schema. Rust tests cover replacement, symlink switching, unverified evidence,
queued MCP admission and the public CLI watch lifecycle.

`RuntimeUpgradeNoGuard.cfg` deliberately removes the admission guard; TLC must
violate `AdmissionChecked`. This supplements Rust boundary regressions and does
not claim that every Rust interleaving has been proven safe.

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

Concurrent certifications of one node (parallel baseline lanes) are outside the
one-writer bound. The code keeps the model's property for them by serializing
`certify` per node under `views_write_guard`, connector pass included, and by
deciding there against the stored views and the silver fingerprints read under
that lock: chats another certification recorded with the same recipe and content
count as covered (an older certification adds its chats to a newer stored one
instead of being discarded), and silver is claimed only when the views cover
every chat (`synced`) and were built from the head (`current`). `sync_upload`
writes its uncertified marker under the same lock without dropping recorded
chats, and starts fresh (deleting documents) only when no compatible views
exist. Regressions: `qdrant_source_recipe_pinning_rejects_mismatches_and_mixed_retained_vectors`
(partial rebuild after a recipe change is not current) and
`qdrant_backfill_lanes_assign_ordered_revisions` (lanes finishing in either
order end current). No model change: these restore the modeled guarantee.

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

Publication no longer waits for collection-wide optimizer idleness (`mash-yves`).
The `SharedViews` actions/configurations are unchanged: `WritePoint` abstracts
completed, awaited Qdrant writes/deletes, and `Certify` records their revision.
Background index maintenance does not change those logical documents. Native
publication still uses `wait=true&ordering=strong`, includes unindexed data in
queries, and checks health plus bounded retrieval probes after certification.
Yellow/optimizing health is accepted; failed, paused or unreadable health is not.
A failed probe can follow a persisted certificate, as before: readiness evidence
does not promise optimizer quiescence or query latency. This change removes an
unmodeled post-certification wait, not a silver/views durability boundary.
The real-backend readiness regression checks applied insert/replacement/deletion
visibility while a proxy continuously reports optimization; API tests forbid
polling the optimization queue. These checks do not prove latency at fleet scale.

### Retry recovery source mapping

[RetryViews.tla](RetryViews.tla) adds the retry details that `SharedViews` abstracts
away. It bounds the system to one native chat, a silver deletion committed before
views synchronization, one retry and three possible supplied contents (absent,
retained, changed). Certification uses silver identity without inspecting physical
documents. Safety requires payload validation and removal of absent chats.
This is a safety-only model; it makes no liveness or concurrency claim.

- `silver_store/apply.rs`: duplicate snapshots use the ordinary transactional
  ingestion rules and compare the resulting canonical fingerprint with retained
  silver; vectors remain outside it. The entire validation transaction rolls
  back, avoiding a long-lived nested-savepoint journal. A separate small
  transaction updates only synchronization metadata. The node lock spans both
  transactions and subsequent views work. `Validate`, `Rollback`, `Metadata`
  and `BeginViews` keep these boundaries separate; `CrashBeforeViews` permits
  interruption between them. Timestamp values and SQLite physical journaling
  are not modeled. The Rust `retry_outer_rollback` regression interrupts after
  rollback, reopens a connection and checks history, the head, storage IDs and
  the AUTOINCREMENT sequence, then verifies recovery and altered-payload rejection.
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

### Local index upgrade cutover

[LocalUpgrade.tla](LocalUpgrade.tla) separates export, validated staged import,
each original-file retirement, new attachment installation, database installation
and marker removal. The old database, WAL, attachments and views are represented
as four independently moved files; the actual script also retains SHM and journal
files. A failed operation or process kill can stop between any modeled writes.
The model checks validation before retirement, attachments before the new database,
and a retained marker throughout an incomplete cutover. Two negative configurations
demonstrate premature retirement and database-first installation.

Source mapping: `tools/upgrade-local-index.sh` stages a complete import before
setting `retiring=true`, moves originals into a unique retained directory, installs
attachments, then links the new database without clobbering an existing path.
The `DATABASE.upgrade-in-progress/backup` marker points to the recovery directory;
completion atomically renames the marker directory into the backup, preserving
its pointer even if the process dies at the final boundary.
the marker is checked before the missing-database no-op. Failures after retirement
starts retain it. `tools/test-upgrade-local-index.sh` exercises rejection without
mutating original files, absent/present publication identity, busy-file rejection,
retained files after successful cutover and interruption after the first rename.
The Ansible regression separately checks that a failed step blocks binary install.

Assumptions and limits: the node stays quiescent throughout cutover. The script's
`lsof` checks detect current open files, but do not fence processes started later;
old Mash binaries do not honor the marker. Stop watchers/agent MCP sessions before
upgrading and keep them stopped until completion. The model assumes one upgrader,
same-filesystem moves/links and completed writes surviving process termination.
It does not prove power-loss durability (the shell does not fsync directories),
filesystem behavior, export completeness, import correctness or automatic crash
recovery. Recovery is manual and retries fail closed. Individual failure during
marker creation/removal may conservatively leave a marker; it must never authorize
retirement without a validated import. No progress/liveness property is claimed.

### Shared vocabulary

`docs/source-model.md` is the reference for how publications, connectors and
baselines relate: source identity (device- vs account-scoped), ownership, the
connector kind/role schema, deletion rules and the deduplication contract.
Model changes here should keep that document in sync.

### WebSocket synchronization: transfer and durable acknowledgement

[SyncTransfer.tla](SyncTransfer.tla) models the publication transfer path that
now runs over the authenticated `/v1/sync` WebSocket. It bounds one immutable
publication batch to two pieces. Receipt and persistence may occur in either
piece order; persisted checkpoint metadata and its client report are separate
actions. Restart retains only the checkpointed prefix. A validated, complete
transfer enters a durable queue; starting its worker transfers ownership away
from the socket. Disconnect does not cancel that worker. A process crash loses
the worker, while queued work and committed evidence survive.

Implemented source mapping:

- `Receive` / `ChangedRetry`: `app/shared/server/sync.rs::handle_chunk` checks
  the negotiated limits, zstd envelope and decoded digest before
  `app/shared/sync/catalog.rs::append` rejects gaps, foreign owners and
  conflicting retries of a retained sequence.
- `Persist`: `Catalog::write_piece` creates the piece file with `create_new`,
  calls `sync_all` on the file and the piece directory, and only then lets the
  metadata transaction begin.
- `SaveCheckpoint`: the same `Catalog::append` SQLite transaction inserts the
  piece row and advances `next_sequence`/`decoded_offset`; a crash can leave an
  orphan piece past the checkpoint, which a later identical retry accepts.
- `ReportCheckpoint`: `handle_chunk` sends `Checkpoint` after the durable append
  and returns `Credit`. The client sends only pieces that fit its budget.
- `Queue`: `Catalog::finish` verifies the manifest digest, contiguous decoded
  offset and ordered piece list, then moves `receiving` to `queued_for_apply`.
- `Start` / `Commit`: `handle_finish` calls
  `Store::index_tracked_receipt`, which passes a `SyncReceipt` into
  `silver_store::apply::write`. The receipt (operation id, manifest digest and
  receipt digest) is stamped on the shared-silver head **inside the same SQLite
  transaction as the history**, so a receipt exists if and only if that history
  committed. `Catalog::published` then records transport bookkeeping only.
  Re-applying a duplicate publication is idempotent in silver.
- `Certify`: `index_tracked` completes views synchronization/certification
  before it returns, so `Published` is sent only after views are current.
  `SilverCommitted` is emitted after that same call returns, not between silver
  and views.
- `Acknowledge`: the `Published` frame carries the revision. `Resume` and
  `Begin` report the durable outcome after a reconnect.
- `Disconnect`: the operation stays in the catalog; a later connection resumes
  from the retained checkpoint. `Close` maps to an explicit `Cancel`, with
  `TooLateToCancel` after publication.
- `Session reuse`: the client keeps one negotiated session per endpoint, group,
  node and database instance. Publications reuse it until it has been idle for
  30 seconds; a peer close or transport failure invalidates it and the next
  publication negotiates once more and resumes from the checkpoint. Control
  frames stay per-operation, so this changes cost, not the state machine.
- `Accepted`, duplicate pieces and rejected inputs are stutters in the model;
  the handler acknowledges accepted pieces and fails closed otherwise. An
  apply failure that did not commit and is not itself retryable (invalid
  history) is reported as non-retryable with its error kind, so the client
  surfaces it instead of resuming an operation that no longer receives data;
  this narrows client retries and changes no durable state.

`SyncTransferSplitClose` is a deliberate broken-behaviour configuration: it
splits expiry into `CloseClear` (release bytes) and `CloseMark` (mark the row
closed) instead of the atomic `Close`. `CheckpointIsDurable` must fail between
them, which is exactly the implementation hazard fixed by the immediate
transaction above. TLC reports `EXPECTED COUNTEREXAMPLE
SyncTransferSplitClose: CheckpointIsDurable`.

Known implementation/model gaps, recorded in Beads:

1. `Commit` spans two databases but a single logical outcome: the canonical
   receipt is written in the silver transaction, while transport state lives in
   the side catalog. A crash between them leaves `queued_for_apply`/`applying`
   or `views_pending`, and startup recovery re-applies the retained pieces.
   `CommitHasReceipt` therefore holds for the canonical receipt, not for the
   transport row. Coverage evidence is part of the receipt: a baseline
   `cover` failure, and a live `record_live` failure, both leave the operation
   `views_pending` and fail the request instead of being logged and ignored, so
   a later baseline cannot overwrite a conversation whose live revision was
   never recorded.
2. Startup recovery is implemented: `server::sync::recover` runs before the
   listener serves and resumes `queued_for_apply`, `applying` and
   `views_pending` rows for every configured node, including connector,
   attachment and backfill-manifest operations. It uses the ordinary mutation
   admission budget and logs failures instead of blocking startup. The same
   recovery runs on a 30-second task (`spawn_apply_recovery`), so a queued row
   converges without a restart or a client reconnect; it is idempotent with
   both. Receiving transfers are intentionally left for their client.
3. Abandoned receiving transfers expire. `Catalog::expire_incomplete` releases
   the spool and marks the row `expired` once its last progress is older than
   the 24-hour retention advertised as `Welcome.incomplete_retention_ms`. It runs
   in three places: a periodic server task every minute
   (`sync::spawn_periodic_expiry`, first tick immediate, aborted with the
   server), when a connection opens, and during startup recovery. Only
   `receiving` rows expire — queued, applying and views-pending work is
   retained. Terminal receipts keep the newest 64 per node.
   `periodic_sweep_reclaims_an_abandoned_receiving_spool` proves reclamation
   with no connection and no restart, and that the spool directory is gone.

   Expiry and piece appends share one immediate SQLite transaction: the sweep
   re-checks `state = 'receiving' AND updated_at < cutoff` before deleting
   anything, and `append` only advances a row that is still `receiving` at the
   expected sequence. A transfer expired by another connection therefore
   rejects further pieces instead of acknowledging checkpoint bytes that were
   just deleted
   (`an_expired_transfer_rejects_further_pieces_from_another_connection`).

   The spool is bounded: every catalog is opened with a hard ceiling
   (`SPOOL_QUOTA`, 2 GiB by default) and `append` refuses a piece that would
   exceed it inside the same transaction, removing the file it just wrote.
   `spool_quota_rejects_a_piece_and_keeps_the_checkpoint` proves the checkpoint
   does not advance and no orphan file remains; an unwritable spool
   (`an_unwritable_spool_reports_an_io_failure_without_advancing`) takes the
   same fail-closed path as a full disk, so ENOSPC cannot acknowledge bytes.

   The wire decoders are property-tested against arbitrary bytes
   (`arbitrary_frames_are_rejected_without_panicking`), against over-sized
   decompression requests
   (`decompression_never_exceeds_the_requested_output`) and against malformed
   manifests (`manifests_reject_arbitrary_bytes`).

   Crash windows are exercised for real. Debug builds honour
   `MASH_TEST_CRASH_AT` at `piece_after_fsync`, `after_silver_commit` and
   `after_published`; release builds ignore it. The server aborts at that point
   and `a_server_crash_after_silver_commit_converges_after_restart` restarts it
   on the same silver directory and waits for startup recovery to publish the
   committed batch, which is the window the TLA+ actions treat as atomic.
4. `Commit` and `Certify` are merged inside `index_tracked`. A views failure
   after a silver commit records `views_pending` and reports
   `commit_state=committed` without `Published`; recovery or a resent
   `FinishUpload` repairs and acknowledges it.
5. Progressive backfill is implemented through manifest creation, durable
   membership, batch ordering and completion. `BeginBackfill` transfers a
   bounded member manifest, `Catalog::store_manifest` installs it durably,
   `BackfillBatch` publications must match the frozen baseline revision, and
   `Catalog::cover` records member evidence before the receipt. Live `/v1/sync`
   publications record `live_revision` per conversation; a baseline batch drops
   records, messages and tombstones superseded by newer live state, then rebases
   the rest onto the node revision, so a late baseline cannot overwrite a live
   conversation or resurrect a deletion. Opening a baseline behind the node head
   fails closed. `mash index --backfill` drives the whole flow.
   `journal_pruning_waits_for_baseline_completion_and_contiguous_coverage`
   enforces the journal gate.

   The client plans the baseline itself: conversations are grouped whole,
   ordered by `updated_at` descending with the identifier as tie-breaker, and
   greedily packed to 128 conversations or 1 MiB of decoded Protobuf records. A
   conversation is never split across batches. Each batch is one
   `BeginPublication` with mode `BackfillBatch`, so **the server commits and
   certifies it as soon as its last piece lands**, independently of the others;
   the first batch is searchable while later batches are still uploading.
   Per-batch receipts persist beside the database
   (`<database>.backfill.json`), and a retry with the same identity skips
   completed batch digests before finishing.

   `mash index` chooses the strategy itself: a database with at least 200
   conversations and **no publication for that node on the server yet** is
   published as a baseline, so the newest batches become searchable while the
   rest upload. Established nodes keep replace/delta semantics, because absence
   must still mean deletion there. `--backfill` remains as a hidden override for
   diagnostics and tests. `Freshness` reports the latest baseline as
   `baseline: {id, state, covered, total}`, so `mash status --shared` shows
   progress while it runs and its completion afterwards. Debug builds honour
   `MASH_TEST_AUTO_BACKFILL_MIN` to exercise the threshold.

   Remaining gaps: no priority scheduling beyond recency, no multi-session or
   restored-baseline ordering, and the journal gate is only exercised by the
   incremental path in production.

   Duplicate history across nodes is measured, not hidden. Silver already stores
   a canonical per-conversation fingerprint, and `GET /v1/fingerprints` folds
   those across the group into `duplicated_conversations`, `redundant_copies`,
   `redundant_messages` and `duplicate_vector_bytes` (an f32-per-message
   estimate at the group's embedding width; text and index overhead excluded).
   `mash status --shared` reports it as `duplicates`. The endpoint exposes
   content hashes only, never plaintext.
6. Connector batches and attachments now use the same transfer path
   (`BeginConnectorBatch`, `BeginAttachment`, then `Chunk`/`FinishUpload`).
   Connector batches commit through the existing `Store::ingest_connector`
   receipt boundary and return a connector receipt revision, not a native
   publication revision. Attachments are sent as uncompressed pieces
   (`Compression::NONE`), validated against the published message metadata, and
   written with `fsync` before `AttachmentStored`.

   **Cutover:** native publication moved to `/v1/sync` first. `POST /v1/index`
   and `/v1/attachment-upload`, their handlers and the base64 attachment writer
   are deleted outright — no test-only route remains. Server tests publish
   through the production WebSocket client, so the suite exercises the shipped
   path rather than a retired one, and
   `released_server_no_longer_exposes_legacy_mutation_routes` proves the routes
   answer 404.
   `/v1/connector-batches` is gone as well. `Published` now carries the
   source-scoped `ConnectorReceipt` (source id, batch id, source revision,
   silver revision) plus views readiness, the Rust client ships
   `deliver_connector`, and the Python SDK delivers by running
   `mash index --connector-batch <file> --shared` over `/v1/sync`. The cutover
   test asserts 404 for the old route.
7. Multiple concurrent sessions are serialized by an in-process catalog mutex
   plus operation ownership checks. Client publication is fenced per local
   database and destination by an advisory file lock
   (`<database>.publish-<target>.lock`, where the target is the destination
   identity). One process owns the publisher for that pair; a second waits up to
   30 seconds for the owner to finish, then reports shared-pending
   ("another process is publishing this database and destination") instead of
   claiming success. The background watch publisher takes the same lock, so a
   queued job cannot race a one-shot CLI or MCP call. Distributed fencing across
   several servers, and server-side cross-process ownership, remain outside the
   model.

Two benchmarks cover different layers:

- **Codec** (`app/shared/sync/bench.rs`, `cargo test --lib sync::bench --
  --nocapture`) compares the old uncompressed NDJSON body, whole-batch zstd over
  that NDJSON, and the production Protobuf+zstd pieces over a deterministic
  varied corpus. It reports wire bytes, encode/decode medians and projected
  transfer time over 10/50/200 Mbit/s links.
- **End to end** (`server::tests::sync::
  qdrant_publish_latency_and_bandwidth_http_versus_websocket`, ignored) times
  real `POST /v1/index` against `/v1/sync` until `Published`, with silver and
  Qdrant views in the loop. It reports measured medians/p90, payload bytes per
  batch, and the same measurements plus wire time at 10/50/200 Mbit/s.

Measured on loopback with disposable Qdrant 1.19.1 and varied vectors, with the
WebSocket side running the production publisher (one reused session). A
160-message batch is 1278.7 KiB over legacy HTTP and 455.8 KiB over `/v1/sync`
(64.4% fewer bytes). Publish latency is dominated by views certification
(about 316 ms vs 327 ms measured across runs), but adding wire time the typed
path is about 49% faster at 10 Mbit/s (1.36 s vs 0.70 s), about 23% at
50 Mbit/s, and about 6% at 200 Mbit/s.

Small batches (6 messages, 48.3 KiB vs 17.3 KiB, 64.2% fewer bytes) measure
about 49 ms vs 65 ms. Session reuse removed the per-publication handshake, so the
typed path is now about 11% faster at 10 Mbit/s; at 50 and 200 Mbit/s it stays
behind by roughly 20–28% because Protobuf+zstd setup outweighs a 17 KiB saving
when the link is short. Neither benchmark measures model inference or deployment
latency.

### Progressive backfill: coverage, live edits and deletions

[BackfillReceipts.tla](BackfillReceipts.tla) checks the client's receipt file
separately from server coverage. The frozen source revision is 10 while three
server commits receive revisions 1..3. `Commit`, `Receive`, `Save` and `Restart`
represent separate remote commits, returned receipts, atomic file replacement
and reopening the client. Responses may arrive out of order or be lost before
saving. `BackfillReceipts::record`, `load_backfill_receipts` and
`store_backfill_receipts` in `app/shared/publication.rs` implement these local
boundaries; the sender's atomic maximum aggregates concurrent lane responses.
Source identity stays fixed, and server progress is the maximum received server
revision, initially zero. The mixed-domain and last-response-wins configurations
must violate `ServerProgressIsReceipt`. The Rust regression
`backfill_receipts_keep_source_identity_and_server_progress_separate` reopens
the actual receipt file and checks out-of-order receipts and baseline changes.
This bounded model assumes one client owns the receipt file, fixed destination
and baseline, and atomic replacement; it does not claim power-loss durability,
server certification, or correct mapping after another publisher changes the head.
The production gap tracked in `mash-e3f4` initialized server progress from the
source revision and overwrote source identity on each receipt; the mixed-domain
configuration reproduces the first invalid state.

[ProgressiveBackfill.tla](ProgressiveBackfill.tla) is a **partially implemented
design contract**. The manifest transfer, durable membership table, coverage
evidence, completion check, per-conversation ordering and tombstone dominance
exist in `app/shared/sync/catalog.rs` and the `/v1/sync` handlers:

- While a baseline is active, every committed `/v1/sync` live publication records
  `live_revision` per conversation in `backfill_members` and covers its baseline
  member. That evidence is durable in the transfer catalog, not silver.
- A baseline batch drops conversation records, their messages and their
  tombstones for every conversation with `live_revision` above its source
  revision, then rebases the remaining records onto the node's current revision.
  A batch whose records are all superseded writes no silver, marks its members
  covered and acknowledges the current revision. This keeps silver's monotonic
  node revision intact while honouring per-conversation order.
- A live tombstone therefore survives late baseline arrival, and a newer live
  conversation is never overwritten by the frozen baseline.
- Opening a baseline behind the node's current revision fails closed with
  `backfill_baseline_stale`; the server never rebases a baseline over history it
  cannot order.

Live ordering evidence is collected from `/v1/sync` publications. A legacy
`POST /v1/index` publication during the cutover window does not record it, so an
open baseline must not overlap legacy-client writes; the stale-open check limits
the damage but does not replace recording.

Local journal pruning is gated by `storage::publication::JournalCoverage`:

- `Incremental { through }` is the ordinary publication path and prunes through
  the published revision.
- `BaselinePending` records the acknowledgement but deletes nothing, so live
  edits still replay while the baseline is incomplete.
- `BaselineComplete { through }` prunes only through the contiguous live
  watermark and never through the newest received revision.

`journal_pruning_waits_for_baseline_completion_and_contiguous_coverage` proves
all three, and
`random_operation_sequences_preserve_catalog_invariants` replays generated
operation sequences (begin, append, duplicate, changed retry, gap, finish,
expire, restart, cancel) against an independent model, asserting checkpoint
equality and spool release in terminal states. Ordinary publication uses the incremental gate; the baseline variants
are enforced and tested at the storage boundary. Multi-session and
restored-baseline ordering remain outside the model.

The client can drive a baseline end to end: `sync::client::upload_backfill`
encodes the frozen batch's conversation membership as the manifest, streams
`BeginBackfill`/chunks/`FinishUpload`, sends the batch as a `BackfillBatch`
publication, then drives `FinishBackfill` and waits for `BackfillComplete`.
`qdrant_client_backfill_sender_completes_a_baseline_end_to_end` runs that sender
against the real handler and asserts the catalog reports `complete`,
coverage equals the manifest, and views are certified.

The normal operator surface is `mash index`; the baseline is chosen
automatically for a large first publication. `mash index --backfill[=ID]` is a
hidden override that forces the baseline path for tests and diagnostics. Either
way it indexes local sources, then
freezes the next local revision, plans the history into batches (128
conversations or 1 MiB each), installs the member manifest, and publishes those
batches in order under the publisher lock. Each batch commits independently and
the journal is acknowledged with the baseline-complete gate once coverage is
complete; the command reports
`{"shared": {"state": "published"}, "backfill": {"id": ..., "revision": ...}}`.
Retrying with the same identity resumes the same server session and skips
batches already recorded in `<database>.backfill.json`. It requires the default
views recipe and a configured shared destination, and conflicts with
`--watch`, `--connector-batch` and `--rebuild-local-views`.
`qdrant_backfill_command_publishes_a_baseline_through_the_cli` runs the real
binary against a real server and asserts the catalog reports `complete` with
full coverage and that shared status shows the same revision.
A frozen baseline at source revision 1 contains conversations `a` and `b`; live
revision 2 edits `a`, and revision 3 deletes it. Four independent batches may be
scheduled in any order. Each carries complete conversation state or a tombstone.
Only one apply/repair worker runs per node, but source edits and crashes may
interleave with its work. Retrying an already committed batch uses its immutable
receipt without applying it again.

`Commit` atomically records silver, per-conversation ordering, the receipt and
baseline membership coverage. A newer live record can satisfy its baseline
member. `WriteDocument` models each separate durable views write/delete, followed
by `Certify` and then the client's `Publish` receipt. The receipt count represents
a monotonic server revision; source revisions are separate. A crash preserves
completed writes and loses the worker or reply. Recovery repairs views from
silver. A tombstone value in `docs` means no searchable document, not an indexed
tombstone. Views settings and node identity are fixed in this model.

Checks require the newest committed conversation version to win, deletions to
survive late baseline arrival, and certified views to match silver. Baseline
completion requires evidence for every member. Local acknowledgement and journal
pruning require baseline completion plus contiguous published live coverage;
the highest received version is insufficient. Per-batch `published` records are
historical acknowledgements, not perpetual claims of current views. A subsequent
batch may make views stale until its repair completes. Fair processing after
edits and crashes stop eventually completes coverage, acknowledges the latest
source revision and restores current views.

Four negative configurations remove version ordering, drop tombstone protection,
finish with missing baseline members or prune using only the highest published
revision. These must expose stale writes, resurrection, incomplete coverage and
lost journal gaps respectively. The finite batch set allows weak fairness to
establish progress; this is not a scheduling or latency guarantee for an unbounded
live stream. Baseline construction, multi-conversation transactions, local
snapshot/journal persistence, identity changes and simultaneous publishers are
outside this model. Tombstones have no later recreation in the bounded history.

The two models are checked separately; their composition is not proven.
`ProgressiveBackfill` still needs each remaining model action mapped to its
transaction/await/lock boundary, and a client that actually sends baseline
batches, before it can replace the HTTP connector and snapshot paths.

Client-driven backfill coverage:
`qdrant_client_backfill_sender_completes_a_baseline_end_to_end`.

Coverage: manifest and completion flow in
`qdrant_backfill_manifest_coverage_completes_only_after_baseline_batches`, late
baseline and tombstone dominance in
`qdrant_late_baseline_cannot_overwrite_live_state_or_resurrect_deletions`
(including the stale-open rejection), and per-conversation evidence in the
`live_publications_supersede_baseline_members` catalog unit test.

Publisher ownership is verified by
`another_process_owning_the_publisher_reports_shared_pending`: a real child
process holds the advisory lock, the parent reports shared-pending within its
bounded wait and leaves publication pending, and once the child exits the parent
gets past the lock and fails at the unreachable server instead.

Watch mode no longer awaits the network publisher. `core::indexing` hands the
sealed snapshot to a process-wide publisher queue when
`IndexRequest::defer_publication` is set; the queue coalesces jobs per database
and publishes the newest snapshot after the local pass has already returned,
running attachment publication behind it. `deferred_publication_queues_without_
blocking_the_indexing_task` proves the call returns before the exchange and that
the background job still reaches the server. One-shot CLI and MCP calls keep
awaiting publication so their reported state stays synchronous.

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
