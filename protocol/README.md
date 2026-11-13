# Mash protocol checks

Small TLA+ state machines describe persistence boundaries that are difficult
to exercise through examples alone. TLC explores every reachable state in each
configured finite model, checking safety and, where stated, progress after
failures stop. These are manually maintained abstractions, not generated Rust
specifications or proofs of the backing services.

## Run

`FleetJobs` covers dashboard fleet admission in
`crates/mash-fleet/src/jobs.rs`. `Reserve` is create-new request-directory
admission and acquisition of its worker lock. `Record` is the atomic receipt
publication after file sync, followed by directory sync on Unix. `Launch` is
the single awaited subprocess launch; `ChildExit` is its exit, and `Finish`
is publication of a completed result. `LoseOwner` covers worker interruption
or timeout: the child may keep running, and the retained directory prevents
another admission using that ID. An incomplete reservation fails closed;
it does not authorize retry. Repeated IDs and browser disconnects stutter.
Different IDs remain different operations. The model does not establish
exclusion between distinct requests, supervisor readiness, command success,
or power-loss durability. Preparation failure and uncertain results are
conservatively represented by loss of ownership without a successful result.
It checks receipt-before-launch and at-most-once submission for two IDs; the
missing-receipt variant must violate `LaunchRecorded`. Rust regressions cover
simultaneous admission, lost workers, corrupt receipts, request binding,
and refusing to report success without per-machine evidence. External
subprocess effects after loss of ownership are deliberately permitted.

`ServiceControl` models dashboard supervisor actions in
`app/dash/services/operations.rs`. `Acquire` is the OS-held per-service lock;
`Receipt` is the completed create-new request write and file sync (plus directory
sync on Unix); `Send` is the one manager invocation; `Finish` is the result
receipt. A process crash releases the lock but retains receipts. Repeated IDs
read the receipt and cannot resend, including when the result is unknown.
The model checks at-most-once submission per ID, not supervisor readiness,
exactly-once effects, or exclusion of externally initiated operations. Different
IDs are different user requests. Browser cancellation leaves the detached worker
running. Partial/corrupt receipts fail closed in Rust; the model abstracts them
as durable receipts because both prevent resubmission. Filesystem power-loss
durability, especially directory persistence on Windows, is not established by
these checks. The missing-receipt configuration deliberately sends before the
receipt and must violate `AtMostOnce` after crash/retry. The matching Rust
regression checks interrupted receipts and target/action ID binding. Runtime
schema reporting adds observational data only and does not change the existing
`RuntimeUpgrade` admission protocol.

Requires Bash, Java 11+, GNU `timeout`, `sha256sum`, and `curl` for installation.
The checker is the fixed [TLA+ tools v1.7.4 release](https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4),
verified against SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

```bash
make protocol-tools       # Explicit download; no Cargo or model assets involved.
make protocol-check       # All configured models and expected counterexamples.
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

The Qdrant engine lives under `app/views/qdrant`, with common scope, projection,
and retrieval types for direct local access and the shared HTTP service. The
original extraction preserved shared mutation ordering and revision checks.
The local cutover is modeled separately by `LocalWriter` and `LocalGold` below;
shared coordinators retain their existing models.

Chat evidence lives in `NodeViews.projections`, keyed directly by chat ID,
with a fingerprint, vector count, and completeness flag. `Projections::merge_matching`
implements the preferred/current fingerprint rule modeled by `CoverageMerge`;
`retain_matching` drops evidence that no longer matches committed history.
The `LocalGold`, `SharedViews`, `RetryViews`, and `CoverageMerge` durable actions
and lock boundaries remain unchanged. Node-scoped deletion is tested against
another publisher with identical chat and message IDs.

Qdrant point publication now uses the generated Tonic client in
`app/views/qdrant/api/grpc.rs`; REST remains for reads and collection setup.
This changes encoding and transport, not modeled atomic actions: each upsert or
delete awaits strong ordering and requires `Completed`, without automatic RPC
retries. Partial batches still fail before certification. Request/response limits,
credential handling and completion statuses are checked in Rust boundary tests;
the existing recovery tests exercise the same silver-first transitions over gRPC.
The models do not gain any guarantee about delayed operations after a crash.

Shared activity reporting is observational: progress records follow the existing
admission-owned worker, and never gate work or change retries, transaction
boundaries or certification ordering. Its cancellation and group-isolation
regressions use Rust tests; no protocol-model transition changes are required.

## Models and source boundaries

### SystemdCutover: shared-state retirement and activation

`SystemdCutover.tla` maps to `tools/restart-systemd-api.sh`. Preparation writes
the pending marker; `Stop` represents a successful systemd stop and zero MainPID.
`Retire` is the same-filesystem rename of the entire shared-silver directory,
including its transfer catalogue and spool. `Activate` records the recovery
boundary before `systemctl start`; `Start` and `Accept` permit a new acknowledged
write before readiness or the release-pointer update succeeds. `Rollback` is
allowed only before activation. After activation, failures retain the marker and
require forward recovery. A process interruption can leave any completed step
intact; a subsequent invocation refuses an existing marker.

`InstallBinary` and `ConfigureUnit` represent installing the server executable
and replacing/reloading the optional systemd unit while its process is stopped.
They are separate steps: interruption between them leaves the pending marker.
Pre-activation rollback restores the original executable path and unit before
starting the old server. After activation the new unit stays in place for forward
recovery. The model abstracts successful unit replacement and daemon reload as
one step; shell fixtures also inject a reload failure after the file replacement.

The model checks that receipts stay with their history, retirement stops the old
owner, and rollback cannot discard new acknowledgements. Two deliberate mutations
retain the old catalogue or permit rollback after activation; both must produce
the named counterexample. Shell fixtures exercise pre-activation restoration,
failed starts with a simulated accepted write, pending-marker rejection, and
successful coupled retirement. These fixtures mock systemctl; they do not prove
systemd process containment. The live service must own all writers to its root,
and operators must prevent independent server starts during cutover. Metadata
durability after power loss and manual recovery are outside this model. The
script retains old state for recovery; it does not migrate archive formats or
establish that every node can republish. Validate those prerequisites separately.

### LegacyActivation: migration before readiness

`LegacyActivation.tla` maps to `tools/restart-dev-api.sh`. `Prepare` writes the
pending marker before stopping the recorded process; `Stop` completes only when
that process exits. `Activate` writes the recovery phase before spawning the
candidate. `Change` represents a migration or durable write that may happen
before readiness, even if the candidate subsequently exits. After activation,
failure retains the marker and never restarts the old binary. A subsequent
rollout refuses that marker. Only pre-activation failure may restore old state.

The model checks that an old binary never runs after a candidate changes state,
and interruptions retain the recovery marker. Its deliberate rollback mutation
reproduces the old unsafe behavior. The shell fixture makes a candidate write
data and exit, checks that the write and marker survive, and verifies that retry
is refused. The fixture's write simulates a migration; SQLite transaction safety
is covered separately by `SilverMigration` and storage tests. This model does
not certify the legacy per-file retirement procedure, process-tree containment,
power-loss durability, or manual recovery. Rollouts must exclusively own their
server processes and state, as with the systemd model.

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
overflow, timeout/cancellation cleanup, phase reporting, and detached worker
ownership. Per-node locks,
SQLite and Qdrant durability remain covered by their existing models/tests.
The deadline was increased after a 32-publisher mixed-workload trial took
104–151 seconds and forced repeated queue expiry at 60 seconds. The model and
TLC configurations are unchanged: expiry is already a nondeterministic
`CancelBeforeWork`, and the model makes no numerical timing assumption.

`MutationAdmissionReleaseWorker.cfg` deliberately releases active capacity on
disconnect and must violate `WorkOwnsCapacity`. `MutationAdmissionUnfair.cfg`
permits taking a non-head waiter and must violate `FairOrder`.

### LocalWriter: retain local ownership across silver and gold work

`LocalWriter.tla` models two processes targeting one local database. `Acquire`
maps to `storage::admission::acquire` and the OS-held sibling `.writer.lock`.
Core dispatch admits indexing (including init's first index and views rebuild),
forgetting, record mutations and import to its destination database.
Readers do not acquire this lock. Contention
returns retryable `index-busy`; there is no queue or fairness claim. MCP
`record` (`Mash::record_within`, `admission::acquire_until`) instead repeats the
same non-blocking `Acquire` every 100 ms for at most 45 s, then returns
retryable `index-busy`; cancellation also ends the wait. A waiter holds nothing
between attempts, so the model's idle process with a disabled `Acquire` already
covers waiting, and giving up is a step from idle that changes neither phase
ownership nor `held`. The admitted owner is handed to core dispatch, which reuses
it rather than acquiring twice. No model change is needed; the runtime check of
`RuntimeUpgrade` runs after this admission, so a replacement during the wait is
still refused.

`StartBlocking` maps to `crate::blocking` capturing an Arc of the task's owner.
`Cancel` can drop the caller while that worker continues; `Commit` ends the
worker and retains the task's owner through subsequent `refresh` work.
`Finish` covers success or failure. The model abstracts multiple SQLite commits
and intervening model work under the same owner; it does not make them atomic.
`Crash` terminates local work and releases the OS lock. Both deliberately broken
configurations must violate `WorkOwnsLock`: losing the worker's Arc on cancellation,
or releasing ownership at the silver commit before refresh finishes.

Assumptions: all local application writers use core admission; one database is
addressed by its canonical path (hard-linked aliases are unsupported); the lock
file is never replaced while a writer owns it. External SQL tools are outside
this contract. MCP operation tasks already survive client disconnection. This
model proves no remote-write fencing, Qdrant durability or certification claim:
requests sent before process death may still arrive afterward (tracked in
`mash-c05a`). Rust tests cover alias resolution, independent databases, continued
blocking work after cancellation, and public CLI contention with reads available.

### LocalGold: invalidate before mutation and certify completed replay

`LocalGold.tla` checks the direct local coordinator with two silver revisions,
two build settings, and two chat projections. Revision 2
deletes the native projection. A reader reports current only when certification,
the captured silver revision, and build settings agree. `CurrentMatchesSilver`
then requires both persisted projections to equal that input.

Exact readers select the persisted build recipe rather than activating a newly
requested dense recipe; they still require that recipe's complete certification
for the current silver. Status reports the persisted recipe and leaves requested
dense artifacts stale when it differs. This does not alter any durable transition
or weaken the model's certification guard. The real-model CLI contract and
`local_qdrant_exact_uses_certified_recipe_without_activating_dense_changes` cover
that reader policy, including rejection after silver changes.

Source mapping:

- `Commit` abstracts a committed canonical transaction in `core/indexing.rs`,
  `core/maintenance.rs`. `LocalWriter` checks
  that admission spans these writes and the following refresh.
- `Capture` maps to `storage/gold_input.rs::Input::capture`; `Activate` covers
  changed settings selected before replay in `views/local.rs::build`.
- `Invalidate` is the awaited `put_node_views` with an empty, uncertified node,
  before any point deletion or upsert.
- `Delete(item)` and `Write(item)` represent separate completed Qdrant mutation
  requests in `views/local/projection.rs` and the common `Writer` and
  `Documents` helpers. Clearing a point before replacing it is an allowed
  intermediate state, not an atomic silver-and-gold transaction. Modeling each
  item separately permits more interleavings than a batched request.
- `Certify` follows all flushes, settle, and revalidation of the input silver
  certificate and active settings. Only this final metadata write claims
  current views. Compatible unchanged point reuse is abstracted as writing the
  same projection; digest correctness is covered by Rust/backend regressions.
  Local replay fingerprints cover chat metadata and minimal search-message
  dependencies, including hidden positions and parent links that preserve
  inference batch boundaries. Canonical-only tool/attachment/content changes
  do not change those fingerprints; the captured silver revision still advances
  certification. `gold_capture_fingerprints_search_dependencies_without_canonical_payloads`
  and the generated search-input tests cover this dependency boundary. The
  snapshot transaction, invalidation, mutation and certification ordering are unchanged.
  The common materializer may reuse a vector from an uncertified point only
  when its node, source, anchor, model generation, dimensions and prepared-text
  fingerprint match. It still writes fresh silver-derived evidence and follows
  every flush and certification guard above. This adds no durable transition;
  fingerprint collisions are excluded. The `qdrant_materializer_*` backend
  regressions cover metadata edits, title/parent dependencies and corrupt-cache
  repair; bounded models do not establish the correctness of those hashes.
  A refresh of a changed chat leaves a document in place when its stored node,
  source, role, chat, project, repository, time, anchor, text, position, model
  generation and embedding fingerprint equal what it would write, and sends
  only the others. That is the same abstraction: the model's write of an
  unchanged projection is a write of equal content, so it adds no transition
  and no interleaving, and invalidation, flush and certification order are
  unchanged. It reads neither the stored vector nor the stored tokens of a
  document it leaves; `document_hit` still checks both whenever search returns
  one, and a full replay rewrites them. A chat's title, times and enrichment
  are read from silver when results are built and are no longer stored on
  documents, so they are outside this comparison; a document written before
  that is rewritten once. `qdrant_chat_window_*` cover leaving, the one-time
  rewrite and changed text.
  The materializer prefetches one bounded group of search inputs/cache evidence
  while the preceding group embeds and writes. Prefetch is read-only; mutations
  still have one ordered writer. `tokio::join!` waits for both branches even when
  preparation fails, before returning to the coordinator. No task owns detached
  mutations, and every flush still precedes certification. These read-only
  steps stutter in this model; its durable actions/configurations are unchanged.
  Inference retains the original 32-row boundaries inside each 256-row cache
  group. `qdrant_materializer_prefetch_preserves_inference_batches_and_retry`
  checks hidden rows, cross-group positions, reuse and edited-message retry.
- `Crash` preserves completed durable writes and releases local ownership.
  Retry captures silver again and starts with invalidation. This is a safety
  model, without a delivery, retry fairness, or eventual recovery promise.

`LocalGoldClearFirst.cfg` deliberately deletes a point before invalidation.
`LocalGoldEarlyCertify.cfg` deliberately certifies before both projections have
finished. Both must violate `CurrentMatchesSilver`; neither is a proposed
implementation.

Limitations: this model assumes mutations have either completed or have not
been sent when `Crash` occurs. It does **not** cover requests still executing
at Qdrant after cancellation, timeout, or process death. The local OS lock does
not fence those requests; that uncovered risk is tracked in
`mash-c05a`. It also assumes all silver writers obey admission, Qdrant retains
acknowledged writes, and collection/configuration mutation by external clients
does not race the operation. No power-loss or remote fencing proof is claimed.
The direct local lifecycle tests in `app/views/local/tests.rs` exercise real
gRPC mutations, hidden/deleted projections, failed inference, and replay. Status
reads use one SQLite snapshot for counts and certificate and decline readiness
if their later Qdrant inspection captured different silver.
Retrieval also rechecks database identity and certificate in one SQLite snapshot
before and after Qdrant search. Replacing a database with the same certificate
must invalidate an existing reader; the Rust regression isolates this identity
check. The bounded model fixes one database identity, so this additional reader
guard does not change its transitions or completed-write assumptions.
The model's unchanged-projection reuse assumes equal fingerprints cover every
stored field. Chat fingerprints include canonical message and chat metadata;
embedding reuse separately requires matching prepared text and model settings.

The cutover removes SQL-gold participation from publication snapshots. The
existing `Publication` and backfill models retain their transition semantics:
snapshot rows come from a consistent silver read, local journal bases select
delta rows, wire bases name acknowledged server heads, and embeddings are
prepared after capture. No gold mutation advances the native publication
journal. The mapped journal/wire-base Rust regression supplements those models.
Publication can now reuse local Qdrant vectors after capture, using the shared
target's model, dimensions and prepared redacted input fingerprint. Cache reads
are advisory: missing or incompatible local views fall back to inference. They
never supply canonical rows, advance the journal or certify either destination.
This changes computation within snapshot preparation, not the modeled durable
transitions, lock boundaries or acknowledgement semantics. The disposable
`qdrant_publication_reuse*` tests cover target isolation, redaction and cache loss.

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
[mcp.rs](../app/mcp.rs) `Mash::perform_response` (and `Mash::record_within`,
which checks after both the gate and local writer admission), and
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

Deltas amend long chats (`mash-evnl`): the same journal triggers also record
each changed message in `shared_message_changes` and mark a chat `whole` when
it was created, removed, tombstoned, journaled before schema 67 or changed a
field its documents carry. A delta sends an `Amend` event (chat record, the
changed/removed message IDs since the base, the resulting message count) and
only those messages for the other chats under a message-local recipe. Message
rows carry the same revisions, are captured in the same snapshot transaction
and are pruned in the same acknowledgement transaction as the chat rows, so
`Edit`, `Capture` and `Acknowledge` keep their boundaries and the
configurations are unchanged; the model's journal is the set of pending
revisions, which both tables realize. The server applies an amendment inside
the one upload transaction and rejects it, committing nothing, unless the
chat's documented fields and resulting message count match; the publisher then
sends a whole-chat baseline. `qdrant_long_chat_delta_publishes_only_changed_messages`
compares an amended server with a fresh replacement of the same history.

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
  `apply`, `Upload::forget`, `Upload::chat`.
- [silver_store.rs](../app/shared/silver_store.rs): `SilverStore::write`.
- Existing regression:
  [canonical revision and atomic publication](../app/shared/server/tests/silver_revision.rs).

Chat bodies, source certificates, canonical hashing, ordinary `Remove` events,
provider-qualified identity is outside this model.
Delta requests add chat IDs and/or forget them; replacement bodies are abstracted
away, including `Upload::amend`, which is accepted only in a delta that has
passed `start` and never in a snapshot or rebased baseline batch. Aborted transactions are atomic rollback, not a model of SQLite internals.
Invalid inputs have no committing transition; input decoding remains a Rust test
responsibility. This model makes no delivery/progress claim.

### SharedViews: separate data writes from certification

[SharedViews.tla](SharedViews.tla) models a native silver commit, invalidation,
destructive replay, individual point writes/deletes, and certification as
separate steps. A crash preserves whichever durable operations have finished.
Bounds: two messages, two canonical revisions, two settings identities and one
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
- [views/materialize.rs](../app/views/materialize.rs): `Writer::write_chat`
  and `finish` retain replay's inference batches and awaited point writes;
  invalidation, ownership and certification remain in the replay coordinator.
- [views/qdrant/views.rs](../app/views/qdrant/views.rs): `NodeViews::current`,
  `put_node_views`, `delete_node_documents`.
- [views/qdrant/configuration.rs](../app/views/qdrant/configuration.rs):
  `select_views`, `active_configuration`.
- [server/store.rs](../app/shared/server/store.rs): per-node `lock`;
  [silver_store.rs](../app/shared/silver_store.rs): exclusive directory ownership.
- Existing regressions in [recovery.rs](../app/views/qdrant/tests/recovery.rs)
  and [recipe.rs](../app/views/qdrant/tests/recipe.rs).

Concurrent certifications of one node (parallel baseline lanes) are outside the
one-writer bound. The code keeps the model's property for them by serializing
`certify` per node under `views_write_guard`, and by
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

An amended chat's views write only its supplied messages and delete the
documents of replaced messages that have no successor; its other documents
stay. These are the model's `WritePoint` steps for the points that changed,
before the unchanged `Certify`; untouched points are the model's unchanged
projection. The chat's evidence is carried forward only from stored evidence
whose fingerprint equals the chat's fingerprint before the amendment, that is,
from a certification of exactly the documents being kept; otherwise the chat
stays uncovered, the views are not current and the publisher resupplies every
chat. No action or configuration changes.

No all-time physical-subset guarantee is claimed while views are stale: in-place
updates permit old documents until deletion completes. This model also excludes
search response snapshot consistency, independently restored
Qdrant metadata/data, arbitrary delayed writes after a process crash, and
multiple servers with different silver directories sharing one Qdrant namespace.
Successful operations are assumed durable and ordered at the modeled boundary;
there is no proof of storage engines or network cancellation semantics here.

Publication no longer waits for collection-wide optimizer idleness (`mash-yves`).
The `SharedViews` actions/configurations are unchanged: `WritePoint` abstracts
completed, awaited Qdrant writes/deletes, and `Certify` records their revision.
Background index maintenance does not change those logical documents. Native
publication uses gRPC `wait=true` and strong write ordering, includes unindexed data in
queries, and checks health plus bounded retrieval probes after certification.
Yellow/optimizing health is accepted; failed, paused or unreadable health is not.
A failed probe can follow a persisted certificate, as before: readiness evidence
does not promise optimizer quiescence or query latency. This change removes an
unmodeled post-certification wait, not a silver/views durability boundary.
The real-backend readiness regression checks applied insert/replacement/deletion
visibility while a proxy continuously reports optimization; API tests forbid
polling the optimization queue. These checks do not prove latency at fleet scale.

Retiring a node ([server/store/retire.rs](../app/shared/server/store/retire.rs),
`mash-server --retire-node`) is not a modeled action and needs none. It runs
with the service stopped, under the history directory's exclusive hold, so it
interleaves with no upload, replay or certification. Its three durable steps
follow the modeled order: persist an uncertified empty views record, delete
the node's documents, then empty its silver. Views therefore stay uncertified
from the first step until the node next publishes, and a crash between steps
leaves either the whole history with stale views or no history and no
documents; neither can be reported current. Afterwards the node is in the
modeled initial state. A real-backend regression interrupts after each of the
first two steps and checks the retained history, the stale marker, a completed
retry and a fresh publication from a different database.

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
- `server/store/sync.rs` and `views/qdrant/views.rs`: synchronization removes native
  documents whose chats are absent from silver, including points left by a failed
  request whose in-memory deletion set or metadata was lost. The node filter
  preserves other publishers' records.
- `views/qdrant/tests/retries.rs`: backend regressions for altered/incomplete
  retries and forgotten documents surviving retry, plus writer serialization.

The negative configurations restore the reproduced bugs `mash-vcfn` (accept
changed payload) and `mash-w8q7` (lose deletion intent). The original `SharedViews`
model required `docs = Desired` before certification; it did not verify that the
implementation's per-chat metadata was sufficient to establish that condition.
This additional model separates the supplied contents from retained silver.
Multiple messages, partial point writes and cryptographic
collision resistance remain Rust/backend test or review responsibilities.

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

### SilverMigration: additive schema upgrades

`SilverMigration.tla` models schema 62 to 63, an old admitted writer retaining
ownership across its operation, an upgrader, and interruption before SQLite
commit. Version and added tables become durable together; history and the chat
search certificate are unchanged. `SilverMigrationNoAdmission` deliberately
removes admission and must violate `WriterExcluded`.

The schema-64 evidence columns use the same admission and transactional migration
boundaries. They introduce no new interleaving, so the model retains its original
version constants. The provenance migration regression checks that existing
evidence survives with unknown attribution.

`storage::tests::schema::interruption` pauses the real initializer with a SQLite
authorization callback after earlier migration DDL has executed, then kills its
child process. Reopening checks rollback of the schema/version, retained history
and identities, release of writer admission, and successful upgrade retry. A
second case kills the child after commit and checks that the upgraded schema is
retained. These tests cover process termination, not power loss. They exercise
the existing `Crash`/`Commit` boundaries without changing the model's semantics.

Source mapping: `storage/schema.rs::initialize_silver` rejects unsupported
versions, acquires/reuses `storage/admission.rs` ownership, rechecks the schema,
and calls `rusqlite_migration` with an immediate transaction. The shared server
also owns its silver directory exclusively. `lib.rs::blocking` carries the lock
into synchronous work. Read-only opens reject incompatible versions. Existing
old binaries recheck their expected version on opening a new writer; already
admitted operations keep admission until their views work completes.

Assumptions: callers do not bypass Mash admission, and SQLite transactional
DDL/version updates commit atomically. The initializer checks that a retained
admission covers this canonical database path before reusing it.
This is a bounded model of process interruption, not power-loss durability or
arbitrary external SQLite clients. The Rust `records_migration_*` regressions
exercise rollback after partial DDL, retry, history/identity/certificate
preservation and actual lock exclusion. They simulate a SQL failure rather than
killing the upgrader process. No liveness guarantee is claimed under repeated
crashes. Record mutation/publication semantics require a separate model when
those paths are implemented.

### Records: atomic receipts, supersession and forgetting

`Records.tla` bounds two competing successors prepared against one revision.
`storage/records.rs::mutate_record` opens an immediate SQLite transaction, checks
the operation digest/receipt, calls `current` to compare the expected revision,
then commits the mutation and receipt together. `supersede` updates both records
and their link in that transaction. `purge` erases all revision text and retains
a forgotten identity; replaying an old receipt does not recreate its text.
`resolve` checks the question and optional answer record revisions inside the
same transaction, then writes the question revision, historical link and both
publication journal entries before the receipt. This adds no durable boundary
or new interleaving to the model. The model abstracts the revision guard;
Rust regressions separately check the answer's owner, scope, revision and evidence,
and verify shared-link delivery and rollback for invalid imported links.
Exact-duplicate reuse also runs inside that immediate transaction: it validates
the request's evidence, finds the existing live interpretation, and saves only
an operation receipt. It does not advance the record revision or publication
journal. This read-and-receipt path adds no durable boundary to the modeled
mutating commits; Rust regressions check repeated receipts, unchanged evidence
and confirmations, and reuse after forgetting without restoring text.

Explicit `supports`/`contradicts` links use that same immediate transaction:
check both endpoint revisions/owner/scope/evidence, insert the link, journal both
ends, and save the receipt. They do not revise either statement or confirm it.
This is an additive relation operation, not a competing-successor transition;
the model's `SingleSuccessor` claim does not apply to relation additions.
There is no new durable boundary or awaited work. Conflict flags are computed
inside existing read transactions; they do not infer semantic disagreement.
The existing model is unchanged; ordinary Rust tests cover these sequential
checks and retry behavior. Link semantics remain attached to record identities
until retirement, supersession or forgetting makes the conflict inactive.

The model's `Prepare` represents a caller retaining a request, not an uncommitted
SQL write. `Commit` is the actual SQLite transaction commit; `Crash` models failure
before it, and repeating a prepared operation models a lost response after it.
Deliberate split-receipt and missing-revision-guard configurations must fail the
corresponding invariants. The Rust receipt-trigger regression interrupts after
the record writes but before receipt insertion and checks rollback and retry.

Assumptions and limits: SQLite serializes the modeled commits, input digests are
validated by Rust, and successful commits survive process termination. This does
not model source-quote meaning, permission identity, power loss or shared record
publication; the shared models below cover that transport separately.
Ordinary source changes retain stale evidence;
only explicit forgetting corresponds to `Forget`.

Exposing the existing confirmation rows as `confirmation_history` changes only
read responses. Streaming source freshness checks retains the same quote, hash
and role predicates and performs no writes. Neither changes transaction or
publication ordering, so no model transition changes are needed. Rust tests
cover confirmation retries, revision-local history, archives and shared reads.
Bounded history/link/list reads likewise retain a single read transaction per
page and make no canonical writes. History uses an exclusive revision cursor;
newer appended revisions cannot shift the remaining older pages. This is not
a multi-request snapshot: freshness and confirmations may change between reads,
and offset-based lists/links may change when their membership changes. Rust
regressions cover revision insertion between pages, size-based continuations,
link pagination and the public MCP path; no write-protocol transition changes.
Selecting shared records in SQL before decoding does not change the publication
snapshot transaction, emitted row order or mutation boundaries. Canonical
fingerprints skip owner-local receipt/checkpoint tables at query time. Projection
tests verify unchanged fingerprints and ensure excluded rows are not decoded;
this read-path optimization adds no protocol transition.

### Extraction checkpoints

`RecordExtraction.tla` bounds two explicit reviews and two source versions.
`Read` represents `get_extraction_checkpoint` followed by the caller's review;
`Refresh` is an indexed chat change. `Commit` maps to the single immediate SQLite
transaction in `storage/records/extraction.rs::extract_records`: compare the
source fingerprint, write all candidates, upsert the checkpoint and insert the
receipt. A failed transaction leaves durable state unchanged; a retry returns
the saved receipt before checking the now-current source. Both are stuttering
steps in this model. The record set represents a successfully applied batch,
including an empty batch, rather than a count of extracted conclusions.

The model checks that coverage cannot precede its batch, every batch has a
receipt, and a batch reviewed the source version current at commit. A subsequent
source refresh is permitted and makes coverage stale. Deliberate early-checkpoint
and missing-source-guard configurations must violate their respective checks.
The model assumes SQLite transaction isolation and atomicity; Rust tests inject
a receipt-insertion failure after candidate/checkpoint writes and reopen the
database to check rollback. It does not establish semantic review quality,
fingerprint collision resistance or power-loss durability.

### Shared vocabulary

`SharedRecords.tla` bounds replacement snapshots and record deltas across four
publication revisions. `storage/records/transfer.rs::Import::shared` retains
the previous headers before a snapshot removes live records; `insert` checks
those headers before accepting a lower record revision or reviving a forgotten
identity. The shared `apply` transaction commits the rows and node head together.
The deliberate lost-prior-headers variant must violate `ForgottenStaysErased`.

The model assumes SQLite transaction atomicity and exclusive node write admission.
It models rejection/interruption as no committed change and does not establish
delivery, semantic evidence quality, privacy masking, or backfill completion.
`native_records_shared_forget_cannot_be_reversed_by_an_older_snapshot` exercises
the retained-header guard and checks persisted deletion after rollback.

`RecallBackfill.tla` models the separate durable boundaries in
`publication.rs::publish_backfill`: chat baseline completion, the ordinary
record upload's commit/receipt, then local acknowledgement. Losing the response
can require replay of the same deterministic upload; it cannot permit early
acknowledgement. The early-ack variant must violate
`AcknowledgedIncludesRecords`. Local acknowledgement and record-journal pruning
share the SQLite transaction in `Storage::acknowledge_publication`; the model's
`LocalAck` removes its journal entry atomically. `PrunedIncludesRecords` and its
early-prune counterexample check that chat completion alone cannot discard
pending record changes. The Rust regressions cover wrong destinations, pending
baselines, partial coverage, a later edit to an already captured record, and
failure after deletion but before the transaction's final metadata update.
Both journals use the same covered-revision bound and floor; requests older
than that floor require a replacement snapshot. The model bounds one baseline;
the coverage tests and `ProgressiveBackfill` cover revision holes separately.
Protocol 4 rejects older peers before transfer admission because their protocol
does not promise native record batches or the final backfill record step.
This model assumes the ordinary sync receipt
contract verified by `SyncTransfer`; it does not model source changes, network
progress, or the amount of data in the final upload. The planner regression
verifies that recall rows remain separate from chat batches. Disposable-backend
acceptance for the final record step remains required before feature completion.
The planner encodes and size-checks the final record batch before sending chat
batches, then retains its compressed pieces for the final upload. Preparation
has already applied publication redaction to those rows. This removes a redundant
JSON spool/reparse without adding a durable boundary or changing the record
commit/receipt/acknowledgement order. Planner regressions check oversize rejection
before a plan is returned, deterministic retry payloads and separation from chat
batches. The model begins after successful preparation, so needs no new action.

`BackfillTarget.tla` covers destination preparation before that baseline capture.
`Prepare` maps to `Storage::prepare_publication_target` under the publication
lock; `Capture` freezes the resulting revision, `Publish` abstracts the complete
chat and record receipts, and `Ack` maps to the destination-conditional
`acknowledge_publication` update. A crash between these steps retains durable
state and permits another attempt. The model starts with either an unset target
or an acknowledged previous target. `CompletedIsAcknowledged` rejects success
without a matching local receipt; the missing-preparation variant reproduces
the zero-row acknowledgement found in mash-2db7. The CLI baseline regressions
check a separate status call after both first publication and a target change.
This bounded model assumes the existing publication lock and SQLite transaction
atomicity. It abstracts upload internals covered by `RecallBackfill` and
`SyncTransfer`, and does not model concurrent source edits or guarantee progress.

`docs/internal/agent-history.md` defines the agent-history domain and the relationship
between canonical chats, messages, derived turns, publication, and replay.
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
   a later baseline cannot overwrite a chat whose live revision was
   never recorded.
2. Startup recovery is implemented: `server::sync::recover` runs before the
   listener serves and resumes `queued_for_apply`, `applying` and
   `views_pending` rows for every configured node, including
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
   publications record `live_revision` per chat; a baseline batch drops
   records, messages and tombstones superseded by newer live state, then rebases
   the rest onto the node revision, so a late baseline cannot overwrite a live
   chat or resurrect a deletion. Opening a baseline behind the node head
   fails closed. `mash index --backfill` drives the whole flow.
   `journal_pruning_waits_for_baseline_completion_and_contiguous_coverage`
   enforces the journal gate.

   The client plans the baseline itself: chats are grouped whole,
   ordered by `updated_at` descending with the identifier as tie-breaker, and
   greedily packed to 128 chats or 1 MiB of decoded Protobuf records. A
   chat is never split across batches. Each batch is one
   `BeginPublication` with mode `BackfillBatch`, so **the server commits and
   certifies it as soon as its last piece lands**, independently of the others;
   the first batch is searchable while later batches are still uploading.
   Per-batch receipts persist beside the database
   (`<database>.backfill.json`), and a retry with the same identity skips
   completed batch digests before finishing.

   `mash index` chooses the strategy itself: a database with at least 200
   chats and **no publication for that node on the server yet** is
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
   a canonical per-chat fingerprint, and `GET /v1/fingerprints` folds
   those across the group into `duplicated_chats`, `redundant_copies`,
   `redundant_messages` and `duplicate_vector_bytes` (an f32-per-message
   estimate at the group's embedding width; text and index overhead excluded).
   `mash status --shared` reports it as `duplicates`. The endpoint exposes
   content hashes only, never plaintext.
6. Attachments use `BeginAttachment`, then `Chunk`/`FinishUpload`.
   Attachments are sent as uncompressed pieces
   (`Compression::NONE`), validated against the published message metadata, and
   written with `fsync` before `AttachmentStored`.

   **Cutover:** native publication moved to `/v1/sync` first. `POST /v1/index`
   and `/v1/attachment-upload`, their handlers and the base64 attachment writer
   are deleted outright — no test-only route remains. Server tests publish
   through the production WebSocket client, so the suite exercises the shipped
   path rather than a retired one, and
   `released_server_no_longer_exposes_legacy_mutation_routes` proves the routes
   answer 404.
   Agent-history-only publication no longer accepts external-content batches
   or account-source ownership transfers. The native publication, attachment,
   and backfill lock and durable-write boundaries are unchanged by this removal.
   The native synchronization models therefore retain their existing actions;
   deleting the separate ingress introduces no new interleaving.
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

[CoverageMerge.tla](CoverageMerge.tla) checks per-chat certification merging
under the views write lock in `Store::certify` (`server/store/sync.rs`), including
the separate pre-write invalidation in `Store::sync_upload`. Two
pending certifications capture overlapping chat markers: revision 2 has the
current marker for chat `a`, while revision 3 has the older marker for `a` and
the new marker for `b`. Silver fingerprints are fixed during the bounded merge;
either certification may finish first. After both, every supplied current
marker must remain covered. The stale-marker configuration reproduces
`mash-e3f4`: checking only whether a preferred map contains the chat suppresses
the valid marker, then final fingerprint validation drops the stale one. The
fix tests the preferred marker's fingerprint before allowing it to block the
other candidate. `qdrant_certification_merges_valid_chat_over_stale_marker_in_both_orders`
reproduces both merge branches through real Qdrant metadata and certification;
it failed with one missing chat before the fix. `Invalidate` models the earlier
durable metadata write: its current stored coverage must replace the upload's
commit-time copy before that copy can overwrite a completed batch. The
invalidation counterexample reproduces mash-34p0 even with the correct final
merge. `qdrant_upload_preparation_preserves_newer_coverage` exercises this
boundary with captured old evidence and a completed newer batch; it failed
before the fix. The test restores those observed metadata states to reproduce
the ordering deterministically, rather than relying on thread scheduling.
This model covers metadata
merging only; document writes, later silver edits, crashes and certification
safety remain covered separately by the other models and boundary tests.

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
evidence, completion check, per-chat ordering and tombstone dominance
exist in `app/shared/sync/catalog.rs` and the `/v1/sync` handlers:

- While a baseline is active, every committed `/v1/sync` live publication records
  `live_revision` per chat in `backfill_members` and covers its baseline
  member. That evidence is durable in the transfer catalog, not silver.
- A baseline batch drops chat records, their messages and their
  tombstones for every chat with `live_revision` above its source
  revision, then rebases the remaining records onto the node's current revision.
  A batch whose records are all superseded writes no silver, marks its members
  covered and acknowledges the current revision. This keeps silver's monotonic
  node revision intact while honouring per-chat order.
- A live tombstone therefore survives late baseline arrival, and a newer live
  chat is never overwritten by the frozen baseline.
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
encodes the frozen batch's chat membership as the manifest, streams
`BeginBackfill`/chunks/`FinishUpload`, sends the batch as a `BackfillBatch`
publication, then drives `FinishBackfill` and waits for `BackfillComplete`.
`qdrant_client_backfill_sender_completes_a_baseline_end_to_end` runs that sender
against the real handler and asserts the catalog reports `complete`,
coverage equals the manifest, and views are certified.
The sender returns only the greatest acknowledged server revision: manifest
completion alone does not establish node-wide views readiness. Callers use
freshness to inspect certification. Removing the synthetic `Indexed` response
with `views_current=true` changes no wire messages, commits, ordering, coverage
checks or local acknowledgement/pruning semantics, so the existing backfill
models apply without a new state transition.

The normal operator surface is `mash index`; the baseline is chosen
automatically for a large first publication. `mash index --backfill[=ID]` is a
hidden override that forces the baseline path for tests and diagnostics. Either
way it indexes local sources, then
freezes the next local revision, plans the history into batches (128
chats or 1 MiB each), installs the member manifest, and publishes those
batches in order under the publisher lock. Each batch commits independently and
the journal is acknowledged with the baseline-complete gate once coverage is
complete; the command reports
`{"shared": {"state": "published"}, "backfill": {"id": ..., "revision": ...}}`.
Retrying with the same identity resumes the same server session and skips
batches already recorded in `<database>.backfill.json`. It requires the default
views recipe and a configured shared destination, and conflicts with
`--watch` and `--rebuild-local-views`.
`qdrant_backfill_command_publishes_a_baseline_through_the_cli` runs the real
binary against a real server and asserts the catalog reports `complete` with
full coverage and that shared status shows the same revision.
A frozen baseline at source revision 1 contains chats `a` and `b`; live
revision 2 edits `a`, and revision 3 deletes it. Four independent batches may be
scheduled in any order. Each carries complete chat state or a tombstone.
Only one apply/repair worker runs per node, but source edits and crashes may
interleave with its work. Retrying an already committed batch uses its immutable
receipt without applying it again.

`Commit` atomically records silver, per-chat ordering, the receipt and
baseline membership coverage. A newer live record can satisfy its baseline
member. `WriteDocument` models each separate durable views write/delete, followed
by `Certify` and then the client's `Publish` receipt. The receipt count represents
a monotonic server revision; source revisions are separate. A crash preserves
completed writes and loses the worker or reply. Recovery repairs views from
silver. A tombstone value in `docs` means no searchable document, not an indexed
tombstone. Views settings and node identity are fixed in this model.

Checks require the newest committed chat version to win, deletions to
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
live stream. Baseline construction, multi-chat transactions, local
snapshot/journal persistence, identity changes and simultaneous publishers are
outside this model. Tombstones have no later recreation in the bounded history.

The two models are checked separately; their composition is not proven.

Client-driven backfill coverage:
`qdrant_client_backfill_sender_completes_a_baseline_end_to_end`.

Coverage: manifest and completion flow in
`qdrant_backfill_manifest_coverage_completes_only_after_baseline_batches`, late
baseline and tombstone dominance in
`qdrant_late_baseline_cannot_overwrite_live_state_or_resurrect_deletions`
(including the stale-open rejection), and per-chat evidence in the
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
