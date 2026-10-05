# Qdrant-only shared storage

Qdrant is the sole supported shared backend. Nodes retain required local SQLite
indexes for offline use and communicate with the authenticated Axum service.
Only that service accesses Qdrant. One node group has its own shared collections.

The PostgreSQL and Redis adapters, driver dependencies, schema migrations,
Cornucopia output and generation, and backend-selection dispatch are removed.
SQLite sqlc generation remains available through the Makefile build workflow.
CI provisions only Qdrant for shared storage. The lockfile shrinks from 434 to
375 packages: 59 removed, none added. This reduces the maintenance/build surface;
it is not a measured search-latency improvement.

Server TOML requires `qdrant_url_env`, with optional `qdrant_api_key_env` and the
existing bounded Qdrant vector/memory/publication settings. There is no `backend`
selector, including `backend = "qdrant"`. PostgreSQL/Redis configuration fields
are rejected. Node HTTP configuration and CLI/MCP tools retain their interfaces.

Unique provider and CLI publication contracts were ported to Qdrant. Shared
read/index tracing remains on the concrete Qdrant methods. Historical benchmark
reports remain evidence of their recorded versions; the Redis vector pilot is
archived under `research/storage/`, outside supported development tooling.

## Validation on Xenia, 2026-10-03

- Cranelift standard suite: 618 tests passed.
- All 31 selected Qdrant contracts passed across the initial run and focused
  corrections. Coverage includes real-model relevance, scoped messages, thread
  metadata, every provider, rich filters, revisions, rollback, source-silver
  evidence, schema compatibility, native copy reconciliation and offline forgets.
- Three additional installed-model and parallel-retrieval tests passed.
- The owned Qdrant/API restart contract passed graceful restart and SIGKILL
  recovery with persisted storage and stale-revision rejection.
- All-target Clippy with warnings denied, formatting, doctests, SQLite generation
  freshness, workflow YAML parsing and deployment-script tests passed.

Two ported fixture assumptions needed correction: provider metadata comes from
thread listing rather than message viewing; and migrated, unverified silver can
publish successfully while explicit indexing still reports incomplete status.
The latter fixture now checks that publication does not certify legacy silver.
Logs remain outside Git under `/home/james/scratch/mash-artifacts/` with prefix
`qdrant-only-` and date `20261003`. Tests used a disposable Qdrant 1.19.1 service;
restart tests owned a separate container. These checks do not establish hosted CI,
production-size performance, power-loss durability or deployment completion.

## Live transition and remaining layer work

The existing Xenia service still uses Redis. No existing shared history has been
deleted or migrated as part of this code cleanup. Bead `mash-847` tracks persistent
Qdrant provisioning, configuration replacement, explicit node re-publication and
coverage/freshness/search verification before retiring the old data. The old
Redis server configuration cannot start the new binary.

### Live preparation verified, 2026-10-03

Persistent Qdrant 1.19.1 now runs on Xenia as `mash-qdrant`, with Docker volume
`mash-qdrant-data`, restart policy `unless-stopped`, and HTTP bound only to
`127.0.0.1:56333`. Its presence does not mean that chat data has been migrated.

The staged server configuration is
`~/.config/mash/server-qdrant.toml`, with a private environment file alongside it.
It retains the three `personal` group identities, existing API token variables,
4096 MiB node snapshot limit and one concurrent upload. A temporary current-code
API on `127.0.0.1:8788` successfully initialized the group, accepted each node's
authenticated capabilities request, and rejected unauthenticated status with 401.
The temporary API was then stopped; no preview service was installed.

Live traffic remains on the existing Redis API at port 8787. Its freshness
baseline contains Xenia and personal-MacBook publications; dev-MacBook publication
is absent. The dev MacBook is reachable using its existing dedicated SSH identity
and has TOML configuration under macOS's
`~/Library/Application Support/dev.jamesqo.mash/`. The personal MacBook's SSH
connection timed out. No node configuration, installed binary or existing index
was changed by this preparation.

Continue `mash-847` by explicitly preparing current certified silver and publishing
each node into Qdrant, then verifying source coverage, freshness, searches and
local offline access before changing the live endpoint. Keep Redis data until
that verification passes. Private baseline and authentication-check captures are
under `/home/james/scratch/mash-artifacts/qdrant-live-transition-20261003/`.

### Provider replay and native build preparation

The installed Xenia SQLite index is schema 16 (568 threads, 169,425 messages,
101,808 vectors); the dev MacBook index is schema 15 (2,252 threads, 301,838
messages). Consistent SQLite backup snapshots of both were saved in their private
migration directories. Neither installed index was migrated in place.

An explicit Xenia replay into a separate schema-39 database used the configured
Claude Code/Codex roots and 90-day source horizon, with local-only lexical gold.
It discovered 818 files totaling about 4.08 GB. The run reported one malformed
record, then exhausted source-change retries on this actively appended Codex
conversation and exited with code 9. The migration database has zero committed
threads/messages. No data was published or certified by this attempt.

Bead `mash-847.1` tracks the active-source ingestion blocker. Per-parse and
post-queue stamp checks already have bounded retries; increasing retry counts
does not establish a safe snapshot. Investigate a verified committed-record
prefix while retaining rejection of unsafe edits, truncations and replacement,
correct checkpoints, and explicit reporting of malformed records.

The dev MacBook built archived source `faf7faf` using its nightly toolchain and
`cargo build-clif --locked -j 2` in a private migration directory. The native dev
build completed in 1m 46s. A read-only `status` smoke check exited successfully,
recognized installed models, reported the deliberately missing database unready,
and did not create it. Its SHA-256 is
`819b84cfa4cedb73c7dc4a8f5572ab567c0868e5b193330da9f2bb81b1fb8a16`.
The installed binary remains unchanged. The personal MacBook still timed out.

The application now supports server-owned silver revisions/completeness,
independent lexical/dense manifests, one active gold configuration and explicit
source-free shared rebuilds. See [the current layer contract](silver-gold-configuration.md).
These application capabilities do not prove live fleet migration or complete the
remaining retrieval evaluation work in `mash-q3e`.

### Active JSONL replay fix

Claude Code and Codex JSONL readers now parse a private, length-bounded file
capture while preserving original source and message identities. File identity
and a hash of the captured prefix are checked after parsing and before applying
queued results. Later appends remain pending through the original checkpoint;
edits, truncation and replacement invalidate the capture. Other provider readers
retain their existing stability checks. Captures add temporary disk usage and
file I/O, without retaining whole source files in memory.

The separate Xenia migration replay subsequently scanned 818 files totaling
4,087,707,520 bytes and retained 812 threads and 315,979 messages, including
38,660 searchable messages. It completed in 141.402 seconds with no model
inference and no source-change rollback. This is a functional replay under host
load, not a controlled performance comparison; the corpus grew between runs.

The replay still exited with code 9 because one Claude source was rejected.
Its diagnostic points to line 8712, column 184; independent JSON decoding
confirmed an invalid complete line rather than two concatenated valid objects.
Silver remains incomplete and gold unavailable. The fix does not ignore
malformed records, change provider files, publish to Qdrant, or switch the live
Redis service. Private captures are `xenia-silver-prefix.json` and
`xenia-silver-prefix.log` in the migration artifact directory above.

### Current dev MacBook replay

The dev MacBook received archived source `d5ac7ac`, including verified JSONL
captures. The native locked
Cranelift incremental build passed in 18.44 seconds. Its binary SHA-256 is
`3e7c346dbb7d8fa2291f7b7a499750da207d34b63fa84049f325d1e1a07c2b8e`.

An explicit local-only, lexical-only replay used a separate `dev.sqlite3` and
private configuration under `~/.local/share/mash/qdrant-migration/`. It exited
with code 9 on a Claude native ancestry conflict; its last progress report was
58.197 seconds, 1,101 scanned files and 745,043,233 processed bytes. The new
schema-39 database has zero committed threads/messages after rollback. The
preserved schema-15 legacy snapshot still has 2,252 threads and 301,838 messages.
Installed configuration, database and binary remain unchanged.

Two occurrences of the rejected message have identical full message hashes and
timestamps, but different parents. They precede an explicit root compaction
boundary in Claude 2.1.219 with matching logical parent and preserved-segment
tail. That tail is a new system record, so a replayed tail is not the explanation.
`mash-847.3` tracks inspection of intervening preserved records and the parser's
pending replay state before defining a narrowly proven recovery rule. Existing
payload and unrelated-ancestry conflict checks must remain intact. Private
captures are `silver-prefix.json`, `silver-prefix.log` and metadata-only
`ancestry-diagnostic.json` in the Mac migration directory. No shared data was
published by this attempt.

### Stop-hook compaction validation

The parser now retains pending replay ancestry across a new native
`stop_hook_summary` only when it links directly to the last duplicate. Restoration
still requires the next root compaction boundary to confirm that summary as both
logical parent and preserved tail. Other new events, a new duplicate block,
unidentified dialogue, payload conflicts and malformed records remain rejected.
The rule adds one transient UUID to existing bounded replay bookkeeping.

Validation passed seven focused parser/reconciliation tests, 619 standard tests
and both Qdrant routed/snapshot copy-reconciliation contracts, whose fixtures now
include the stop-hook case. Inline and spilled ancestry stores, archived context,
surviving copies and restart checkpoints are covered. A native Mac build of the
verified working-tree fix indexed the previously rejected source in isolation:
one thread, 3,491 messages and 508 searchable messages, zero rejected sources,
complete silver and exit code 0 in 2.073 seconds. This is a functional source
replay, not a controlled throughput benchmark. Private captures are
`stop-hook-case.json` and `stop-hook-case.log` in the Mac migration directory.

The full Mac replay progressed past that source, then exited with code 9 on a
workflow journal containing multiple agent identities. Inspection found 141
`started` and 97 `result` events, 141 agents, no user/assistant message records
and no malformed JSON. `mash-847.4` tracks handling of event-only sources without
rejecting them as chat transcripts or guessing from filenames. The migration
database remains empty after rollback, and the installed index remains unchanged.
No shared publication or live endpoint cutover has occurred.

### Event-only workflow journal validation

Agent identity conflicts are now collected during parsing and rejected when
retained dialogue is normalized. A source with no retained dialogue can contain
events from many agents without being treated as one chat. Previously accepted
transcript identities, including identity supplied by metadata, are unchanged.
Mixed-agent dialogue and malformed records still reject the source; no filename
exclusion or provider-file repair is involved.

The existing checkpoint/contribution path handles event-only sources: replacing
a source's chat content with events removes its old contribution and preserves a
checkpoint across restart. Validation passed 621 standard tests and three
Qdrant source-lifecycle/routed-copy/snapshot-copy contracts. Publication fixtures
now include the event-only journal alongside dialogue.

The native Mac build of the verified working-tree fix scanned the actual journal
in isolation with exit code 0, complete silver, zero rejected sources and zero
threads/messages. Its 41 ms duration is a tiny-source functional diagnostic,
not a corpus throughput measurement. Private captures are `workflow-case.json`
and `workflow-case.log` in the Mac migration directory. The full replay uses
`silver-workflow.json` and `silver-workflow.log`; it has progressed beyond the
earlier journal rejection but completion is not established by the isolated case.

The full Mac replay subsequently exited with code 0 and complete silver, zero
rejected sources and zero malformed records. It retained 3,018 threads,
434,486 messages and 68,731 searchable messages in 214.344 seconds. This is an
explicit local-only lexical replay on a changing native corpus, not a controlled
comparison with previous failed runs. The installed legacy index remains
unchanged; shared publication and dense-gold preparation are still pending.
Read-only local status reports ready. A direct local `cbioportal` search using
the staged configuration exited successfully with two thread results. Private
captures are `offline-status.json` and `offline-search.json`; this verifies the
local lexical path, not dense retrieval quality or shared coverage.

### Native dense gold from retained silver

The staged Mac configuration now enables lexical and dense representations with
related-discussion intent. An explicit `index --rebuild-local-gold` completed
without scanning provider files: 68,731 stored embeddings, 48,287 model inferences
and hybrid readiness in 434.467 seconds. This is one native corpus run, not a
controlled benchmark or evidence of search quality.

The existing ten-table silver digest helper in `tools/benchmark-gold.py` produced
the same SHA-256 before and after the rebuild:
`b20bc8074a0cdc80fa6e0a46624a8aa74608d1e3e5cc6e34229dab1d8d535490`.
Silver remains complete at revision 627,495. Local status reports hybrid ready;
both related-discussion and exact-mention `cbioportal` searches exited successfully
with two thread results. The exact request reports lexical mode, while the related
request reports hybrid mode. Captures are `gold-before.json`, `gold-after.json`,
`dense-rebuild.json`, `dense-related-search.json` and `dense-exact-search.json` in
the private Mac migration directory. This proves the local source-free layer
switch on this corpus, not the broader shared lifecycle acceptance gates.

The authenticated Qdrant preview API now listens on the tailnet at
`linux-node:8788`, separate from live Redis on 8787. All three configured node
credentials passed capabilities checks; unauthenticated status returned 401.
The Mac authenticated from its own existing credential. Its staged configuration
now targets that preview with destination `both`.

The first explicit refresh completed locally with unchanged silver revision and
all 3,021 sources checkpoint-skipped. It uploaded 1,163,587,427 bytes, then exited
9 with shared state `pending` and an invalid/oversized-data rejection. The preview
allowed a 4,096 MiB node snapshot but retained the default 1,024 MiB mutation
snapshot budget. `Store::index` reserves incoming plus retained serialized bytes
before validating the upload, so this incoming body necessarily exceeded that
budget. The HTTP client exposes only the generic rejection, not the server's
specific error text.

The staged server now explicitly sets `max_mutation_snapshot_mib = 4096`, retains
one concurrent upload, and enables existing opt-in tracing. Xenia reported about
42 GiB available memory before the change; the serialized-work budget is not an
RSS limit. The preview restarted and passed authenticated capabilities again.
A new explicit publication used captures `qdrant-publication-retry.json`
and `.log`; no successful shared publication or live endpoint cutover is claimed.
The first attempt's captures remain available as `qdrant-publication.json` and
`.log`. Installed node configuration and live Redis remain unchanged.

The retry uploaded the full body and tracing confirmed admission of 1,110 MiB
of snapshot work. The client subsequently exited 9 at its 600-second HTTP
deadline (679.035 seconds including local preparation), with shared state pending
and `shared index HTTP request timed out`. Local silver remained complete at
revision 627,495 with unchanged counts. This is a terminal client request, not
proof that the server mutation stopped: `server::admission::run` retains admitted
work and its capacity permit after the HTTP requester disconnects.

The server proceeded through validation, 67.9 seconds of snapshot serialization,
116.2 seconds of snapshot staging and 1.34 seconds of thread catalogue staging.
Document staging remains active; a read-only Qdrant inspection found 90,496 staged
message points, while authenticated application status still exposed zero rows.
Do not restart the server or retry the upload until that admitted operation is
terminal or its accepted revision is verified. Existing tracing is retained in
`preview-server-retry.log`. An optimized release build is being prepared in
`release-build.log`; it is not yet installed or used by this running operation.

During publication, one native local exact `cbioportal` search returned a thread
in 238.9 ms (190 ms core time), selecting only metadata and lexical artifacts.
The preview status request returned in 22.5 ms with zero published rows. These
single smoke checks show read availability during this operation; they are not
latency percentiles, a concurrency load test or shared publication acceptance.
The local capture is `during-publication-search.json` in the private Mac directory.

### Preview publication completed after the client deadline

The detached mutation reached terminal `outcome = ok` at 20:21:38 UTC. The
preview now exposes 3,018 threads, 434,486 messages, 68,731 searchable messages
and 68,731 embeddings, matching the staged Mac. Freshness identifies node
`dev-macbook`, source publication revision 506,236 and shared silver revision 1;
both lexical and dense manifests are current. Captures are `published-status.json`
and `published-freshness.json` in the private Xenia migration directory.
This verifies server publication, not the timed-out command's local acknowledgement
or complete group migration. The other two nodes have not been published here.

Native Mac shared-only `cbioportal` checks returned two threads for each intent:
exact-mention used lexical mode in 305.8 ms, and related-discussion used hybrid
mode in 3.877 seconds. Both reported successful shared retrieval without a shared
error; diagnostics selected the expected lexical versus lexical+dense artifacts.
These single fresh-process smoke timings use the existing debug binaries and
do not establish quality, percentiles or optimized production performance.
Captures are `published-exact-mention.json` and `published-related-discussion.json`
in the private Mac directory.

The application bulk budget is now thirty minutes, shared between client
transport/admission and the matching server routes. Interactive clients retain
their two-second deadline. This addresses the observed ten-minute acknowledgement
timeout without adding a job API or configuration framework. Validation passed
621 standard tests, strict all-target Clippy, formatting and doctest compilation.
The changed budget is committed in `8fbdda6`. Release and native Mac builds
both passed. After the admitted mutation finished, the old preview exited
gracefully and was replaced by the release executable (SHA-256
`cb46f41ebb3969c70ffcc1bc8945e17cc2ec2cdfd41d19b140abdf1e16c80d0a`).
Authenticated status and freshness are exactly unchanged across that restart.
All three node credentials pass capabilities checks and unauthenticated status
returns 401. The updated staged native Mac executable has SHA-256
`1d466e2b2e1954c8bf6489cf622b9596538893a4366b6f4c17cde2cfd443811c`.

Post-update native checks returned two `cbioportal` threads for shared exact,
shared related and combined exact requests, without a shared error. Single
fresh-process wall times were 2.127 s, 2.839 s and 301.8 ms respectively. Shared
results identify `dev-macbook`; combined results remain local and diagnostics
report completion of both sources, consistent with excluding this node from
remote combined candidates. These are functional smoke samples, not comparable
percentiles or proof of a release speedup. Private Mac captures begin
`release-shared-` and `release-combined-`; Xenia restart captures are
`release-status.json`, `release-freshness.json` and `preview-release.json`.
The timed-out publication's local acknowledgement and fleet cutover remain
unfinished. Installed configurations and live Redis have not changed.

### Avoid repeating an accepted full snapshot

Full-snapshot publication now checks existing publication-plan and freshness
responses before preparing a body. When the local source instance/revision is
already accepted, silver is complete under the current interpretation, and both
lexical and configured gold manifests are current, it rechecks the immutable
publication head and conditionally acknowledges that local revision. The client
does not scan message bodies, upload data or perform model inference. A changed
head or stale artifact takes the existing conditional upload path. Local changes
after the check cannot be acknowledged by the old revision.

This uses the existing typed API contracts and adds no endpoint or background
job. It also covers a lost HTTP acknowledgement after successful server commit.
The new client has not yet been deployed to the staged Mac; its pending local
acknowledgement and the fleet migration remain open at that source checkpoint.

### Native recovery verified; candidate deadline corrected

The staged dev Mac client built `aae6653` successfully (SHA-256
`e775cc45663d9b0fec89682804075bc479b8cd29e0bf5e36870f9e20d1ae7b5c`).
A normal refresh checked 3,021 source files with no malformed records or model
inferences. Its source publication revision stayed 506,236 while the local
acknowledgement advanced from 0 to 506,236. Shared preparation took 737 ms with
zero upload bytes; complete process wall time was 5.161 seconds. Silver remained
complete. This resolves the staged node's lost acknowledgement without another
1.16 GB upload. Private captures are `recovery-before.json`, `recovery-after.json`,
`recovery-index.json` and `recovery-status.json` in the Mac migration directory.

Combined exact `cbioportal` search then returned two threads in 261.8 ms. A
shared-only related-discussion request timed out, although release API request
22 completed successfully in 1,939 ms. The two-second client deadline left
insufficient margin for network transit and response handling. Candidate search
now has a five-second request budget; metadata/context reads remain at two
seconds, context aggregation at one second and bulk work at thirty minutes.
There is no automatic retry or algorithm change.

The corrected native build has SHA-256
`62901efb52d1be3a896f58325befebf1f8b1c1a5ace3cdf6099e5e2046d6050c`.
The shared semantic retry returned two threads without shared/context errors in
4.227 seconds including fresh-process model work; candidate retrieval itself
took 725 ms in that sample. Combined exact returned two threads in 259.8 ms.
These are functional samples with variable cache state, not a speedup comparison
or percentile evidence. Captures begin `deadline-shared-` and
`deadline-combined-`. Installed configurations, the live Redis endpoint and the
staged Qdrant server remain unchanged; fleet migration is still incomplete.

### Full shared source-free rebuild and preview update

The dev Mac's source-free shared gold rebuild completed successfully using the
native `4e1b15b` debug/Cranelift binary pinned above, with provider roots deliberately
unavailable and installed local inference only. It rebuilt all 68,731 vectors
for 3,018 threads and 434,486 messages, with 68,731 model inferences. The local
ten-table silver digest remained
`fedd96c45751eb4d98fe4a8c68522766420cae82089c5e0341dc48507611ea33`;
shared silver revision/fingerprint remained unchanged, and source publication
revision stayed 506,236. Both shared lexical and dense manifests are current.

| Observed native phase | Time |
| --- | ---: |
| Retained-silver download | 102.349 s |
| Model loading | 3.411 s |
| Embedding work | 5,223.276 s |
| Gold upload/publication | 558.119 s |
| Core total | 6,043.954 s |
| Externally observed process wall | 6,045.087 s |

Phases are not an exhaustive partition of total time: materialization and other
work are also included. The upload contained 144,211,634 bytes. Externally sampled
process RSS reached 2,747,792 KiB (about 2.62 GiB), sampled every two seconds;
this is a lower bound, not an exact peak, and excludes API/Qdrant resources.
This is one native debug-build run, not an optimized throughput, percentile,
initial-ingestion or relevance result. Different inference counts, batch shapes,
cache/power conditions and work boundaries prevent a causal comparison with the
earlier local rebuild. Retained captures are `shared-gold-{before,after}.json`,
`shared-gold-local-{before,after}.json`, `shared-gold-measurement.json` and
`shared-gold-rebuild.{json,log}` in the Mac migration directory and the private
Xenia `qdrant-live-transition-20261003/native-shared-gold/` artifact directory.

After terminal success and the preservation checks, the preview API was updated
from `8fbdda6` to the optimized `d77024b` release. Its frozen binary SHA-256 is
`04402a069fc7be0a743dad1fb2b49cd7438f6aabb75db903a4ea9345da7eea6e`.
The authenticated freshness response stayed identical except for the newly
reported configured-but-unpublished nodes: `personal-macbook` and `xenia`.
The API remains at `linux-node:8788`; its exact binary, PID and source pin are
recorded in `active-api-release.json`, with trace in `preview-server-current.log`.
No installed node configuration, local index or legacy Redis service was changed.

A shared contextual `cbioportal` search completed with full context on both API
versions against this rebuilt corpus: fresh-process wall times were 2.765 s and
1.821 s respectively. These single observations have variable model/filesystem
cache and Qdrant optimizer state and do not establish a speedup. The subsequent
six-query shared/combined cost control stopped on
`how was cbioportal configured`: its context stage exceeded the one-second
deadline. Candidate retrieval still succeeded, with partial-context evidence
reported explicitly. The larger topology pages therefore do not prove that the
timeout is resolved. Failed captures remain in `ranking-shared-cost-20261003`;
small-window context lookup needs further work before completing that cost gate.

Fleet migration remains incomplete: the personal Mac is still unreachable and
Xenia's new silver scan remains blocked by one malformed source. Legacy history
is retained on port 8787 until those nodes can be republished and verified.
Xenia's staged migration TOML now uses the actual tailnet preview address rather
than the earlier loopback address. Its authenticated `status --shared` sees the
dev publication and both unpublished members, while correctly reporting its own
local index as not ready. This updates only staged configuration, not the installed
node configuration or the incomplete silver certificate.

### Small native context reads

Radius-zero through radius-two views now query indexed integer ordinals instead
of downloading the entire thread outline. Linear views query the saturating
ordinal window, preserving gaps; tree views follow ancestors and fetch children
of the selected path, choosing the smallest ordinal as before. Branch fan-out
still determines the size of each child read. Larger explicit views retain the
full-thread topology path. Ordinal filters serialize integers without conversion
to floating point. The service creates the two payload indexes through its
existing collection initialization; no silver payload or embedding changes.

Complete-thread ordinal uniqueness and parent existence remain publication
checks. Small reads validate the fetched neighborhood and selected bodies,
including identity, scope, edge validity, duplicate ordinals and unchanged
publication. They no longer inspect unrelated thread rows for backend corruption
on every request. This is a narrower read-validation boundary, not a relaxation
of publication validation. The branch/publication and 1,256-row linear contracts
passed serially against disposable Qdrant 1.19.1. A tree fixture with 1,300
unrelated roots also passed on a fresh disposable instance, comparing small and
large views in both content representations. Earlier concurrent/setup attempts
timed out; the fresh fixture passed in 47.507 seconds including initialization,
upload and cleanup. This is functional evidence, not search latency. The real
failing query still needs to be retried before claiming the timeout is resolved.

### Preview neighborhood deployment and completed search control

The optimized `6e8f2bb` release is now running on the staged endpoint
`linux-node:8788`, PID 3928903, frozen binary `mash-native-neighborhood`,
SHA-256 `f82bf0bf52a931a3f91899fbf890ead30625f1f0edb7d681b731b2866ce7674c`.
The service added its two integer payload indexes through existing initialization.
Authenticated freshness stayed unchanged across the update. Prior API metadata
and binary are retained for rollback; `active-api-release.json` identifies the
current process, with trace in `preview-server-neighborhood.log`.

The failing shared query now completes with full context. The corrected bounded
control completed 52 requests across six queries, two scopes and ranking modes,
fresh processes and one resident worker, preserving local silver and shared
freshness. See [the measured costs and limitations](shared-ranking-cost-2026-10-04.md).
The first attempt's worker-path failure is preserved separately. This completes
the bounded context regression/cost check, not a universal latency or relevance
claim. Fleet migration and search quality acceptance remain incomplete.

### Dev Mac default deployment: 4 October 2026

The dev Mac now runs optimized native release `addb212`, SHA-256
`63a38a7b60e374e6c8b2366bbebd2ad7000d0754e0abdaec951bb78b6a048a0d`,
from its installed `~/.local/bin/mash`. Its actual macOS default configuration
and database live in `~/Library/Application Support/dev.jamesqo.mash/`.
The installed configuration uses both local and shared indexing, a 90-day
window, lexical and dense artifacts, and the existing authenticated Qdrant API
at `linux-node:8788`. Models remain in the existing macOS default directory;
this deployment did not download models or re-embed the corpus.

Before replacing defaults, read-only checks compared the legacy schema-15
database (2,252 threads, 301,838 messages) with staged schema 39 (3,018 threads,
434,486 messages). All legacy thread IDs remained present. There were 53,761
missing legacy message IDs, so ID equality alone was insufficient. Exact
same-thread/role/body multiset comparison narrowed this to 1,354 rows in 36
threads. Every remaining body was present under the canonical conversation now
owning its identical physical source path: all were Claude subagent paths whose
thread identity had changed. This establishes body coverage for this snapshot,
with explicit source-owner reassignment; it does not preserve every old ID or
assert unchanged search projections. Projection multiplicities differed
substantially and were not used as a preservation certificate.

The release passed staged local exact, shared exact and local related-discussion
searches before installation. With no Mash processes active, SQLite's backup API
copied the staged database into an incoming file; `quick_check` and the message
count passed before replacement. The old installed binary, TOML, database and
any WAL/SHM files are retained in
`~/.local/share/mash/qdrant-migration/installed-rollback-addb212/`.
The frozen release, source pin, coverage captures and install record remain in
that migration directory. Private evidence was also copied to
`mash-artifacts/qdrant-live-transition-20261003/dev-installed-addb212/` on Xenia.

Installed default-path checks passed: authenticated status reported published,
local exact `cbioportal` returned two threads in 149 ms, and combined related
search returned two threads in 1.068 s. Diagnostics confirmed both sources and
every requested stage completed, with no shared fallback or incomplete context.
Shared candidates were empty in combined mode because only this node is
published and its remote duplicate is excluded. The staged shared-only exact
check returned results in 260 ms. These are single smoke observations, not
percentiles or semantic-quality evaluation. `mash mcp` also completed stdio
initialization and exposed the seven expected tools. Its search path remains
covered by the release's existing SDK integration tests; this deployment smoke
checked protocol initialization and tool discovery.

This completes only the dev-Mac default cutover. The personal Mac remains
unreachable, Xenia's new scan remains blocked by its malformed Claude source,
and the legacy Redis service and those two installed configurations are retained.
At that cutover, the shared preview API remained the verified `6e8f2bb` release.

### Shared API model-switch release (2026-10-04)

The Qdrant HTTP API now runs the optimized LLVM release of `22dd8b6`, including
the verified named-vector dimension-switch implementation and CLI/HTTP/MCP
model-routing contracts. The existing embedding model, gold configuration,
Qdrant collections and node configurations were retained; no activation or
reindexing was performed. Authenticated freshness captures before and after
deployment are exactly equal, including source and silver revisions, both
timestamps, manifests, counts and the unpublished-node list. Authenticated
capabilities succeeded and unauthenticated freshness returned HTTP 401.

The first replacement process exited because the old HTTP listener had not yet
released its port. After confirming that process had exited and the port was
free, the replacement started successfully. The frozen previous executable and
release metadata remain available for rollback. `active-api-release.json` and
`preview-server-current-log.txt` in the private migration artifact directory
identify the current executable, PID, SHA and trace log.

The dev Mac's installed `addb212` CLI returned two thread results for exact
`cbioportal` searches in both shared-only and combined modes. Wall times were
261 ms and 2.326 s respectively; these are single observations, not percentiles
or a performance comparison. Diagnostics confirmed all requested sources and
stages completed, with no fallback or incomplete shared context. The combined
observation spent 2.296 s in local exact retrieval; its shared request took
227 ms and correctly excluded this node's duplicate shared results. This
deployment does not establish actual alternate learned-model quality or complete
the remaining fleet migration.

### Artifact readiness rollout (2026-10-04)

Both the shared API and dev Mac's installed CLI now run optimized native LLVM
releases of `fae282b`. The API's existing per-node freshness read reports typed
metadata/lexical/dense publication states. The installed CLI and MCP `status`
with `shared=true` expose those fields through the shared core. For the dev Mac,
all three local and shared artifact states are `ready`. This is publication
readiness, not a certificate of exhaustive provider history or search quality.

This was a binary-only upgrade. Before/after API freshness is identical after
removing the additive `artifacts` field: revisions, timestamps, manifests,
canonical fingerprints, counts and unpublished members are unchanged. The
server retained the same TOML, embedding model, gold configuration and Qdrant
collections. Authenticated capabilities passed and unauthenticated freshness
returned HTTP 401. The previous API executable and metadata remain available
for rollback; the active release record and current log pointer identify the
new process and frozen executable.

The dev Mac build completed in 68.9 s using its existing Cargo target cache.
The installed binary's SHA256 is
`5f2175891a2322c71df5f20a4eef6609245c15d6d389cf3d739be0c81db9510b`.
Before replacement, the staged release passed default-path local/shared status
checks. With no Mash processes active, the installer retained the preceding
binary/release record in `installed-rollback-fae282b/` and atomically replaced
only `~/.local/bin/mash`. SHA256 hashes of the installed TOML, SQLite database
and present WAL were unchanged after staging, installation and all smoke checks.
The database still contains 3,018 threads and 434,486 messages. No provider scan,
gold rebuild or model download was performed.

Installed `cbioportal` searches returned two threads in each tested case, with
all requested sources/stages completed and no fallback or incomplete context:

| Intent | Scope | Observed wall time |
| --- | --- | --- |
| Exact mention | Local | 152 ms |
| Exact mention | Shared | 257 ms |
| Exact mention | Combined | 256 ms |
| Related discussion | Shared | 1.744 s |
| Related discussion | Combined | 976 ms |

Each search used `--no-worker`; these are individual smoke observations with
uncontrolled filesystem/model-runtime caches, not percentiles or a comparative
speedup. Combined mode still excludes this node's shared copies, so the remote
source returned no additional candidates. An installed stdio MCP session also
initialized and called `status(shared=true)`, verifying the same local/shared
artifact states through the actual protocol.

Native build/install/status/search/MCP captures remain private in the dev Mac's
migration directory and `dev-installed-fae282b/` in the Xenia artifact directory.
Personal-Mac connectivity, Xenia's malformed source and retirement of the legacy
Redis service remain outstanding; this rollout does not close fleet migration
or the independent held-out search quality gates.

### Source-silver certificate rollout (2026-10-04)

The shared API and dev Mac's installed CLI/MCP now run optimized LLVM releases
of `5572b57`. Shared status exposes the retained full-snapshot source certificate,
separately from the server-owned silver identity, upload revision and routed
provider coverage. The dev Mac's certificate reports local silver revision
627,495, interpretation version 1, `complete` and zero rejected sources. This
is the existing certificate's ingestion scope; it does not certify all history
from every provider or independently verify the provider filesystem.

Before/after API freshness is exactly equal after removing the additive
`source_silver` field. Counts, canonical fingerprints, manifests, revisions,
timestamps, artifact states and unpublished members remain unchanged.
Authenticated capabilities passed and unauthenticated freshness returned 401.
Server configuration and environment-file hashes were unchanged. No model/gold
activation, provider scan or index rebuild occurred. The prior frozen API
executable and release record remain available for rollback.

The API release build took 41.9 s; the native Mac build took 30.8 s using its
existing Cargo target cache. Staged and installed Mac executables each passed
default-path CLI shared status and a real stdio MCP `status(shared=true)` call,
returning the same source certificate and ready metadata/lexical/dense states.
The installer preserved the prior executable in `installed-rollback-5572b57/`
and replaced only the binary. Configuration, SQLite and present WAL hashes
were unchanged after staging, installation, status and search checks.

Installed exact `cbioportal` searches returned two threads in shared and combined
modes, in 257 ms and 261 ms respectively. Diagnostics reported every requested
source/stage complete, with no fallback or incomplete shared context. These
individual smoke observations are not percentiles or a quality comparison.
The active API release record and private `dev-installed-5572b57/` captures
retain build, source archive, native binary, status, MCP and data-hash evidence.
Personal-Mac connectivity, Xenia source recovery, legacy Redis retirement and
independent search quality acceptance remain outstanding.
