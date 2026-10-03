# Sustained captured-source topic publication control

`mash-c0d.19.2.38.7.3`. Protocol committed as `36b6850` before implementation
or execution. This scheduler/lifecycle check leaves the phrase SOURCE_COVERAGE
method, labels, source policy and thresholds unchanged. It introduces no model
inference, algorithm family, semantic annotations or selection decision.

## Procedure

Replay the same pinned exposed 128-thread/1850-source cohort at 100 arrivals/s,
then 6000 alternating delete/reinsert events at 100/s for 60 seconds. Mutation k
addresses original source `(k // 2) % 1850`, deletes on even k, and restores its
unchanged original identity/provider/thread/text on odd k. The synthetic lifecycle
schedule does not establish actual native edit or deletion performance. Its final
source set and topic semantics must equal the prior baseline; revision ends at
7850 and current source versions are audited separately.

One parent ledger, one spawned process snapshot and one pending latest revision;
5 ms polls; completion includes draining and worker shutdown. Every publication
is checked against its own submitted checkpoint's complete source-version map,
revision and namespace. Diagnostic expected metadata can temporarily contain the
newly submitted and just-completed revision (maximum two), but never a per-event
job/checkpoint queue. Finite event/timing histories are research instrumentation
overhead, not a production storage design.

Ledger advancement lag is time until the published revision first reaches/passes
an event. A captured latest upsert must have that exact source version; a latest
deletion must be absent. Earlier mutations for the same identity are superseded,
not independently visible. A quickly deleted/reinserted source need never expose
its transient absence. Report both categories instead of calling every revision
an observed source update. Six fixed 1000-event churn windows report advancement
lag, drift and peak/end revision backlog; final drain is separate. Backlog during
drain is excluded from window-end observations, while lag includes later drain
publication when needed.

Use CPython3.11.15 and the pinned science-reference environment, single
OpenBLAS/OMP/MKL worker, Xenia, one exact repeat. Both controls run under the
external parent-only `memory_audit.py` observer at 20 ms. Internal 50 ms sampling
covers parent and worker separately after the first worker PID becomes known.
The observer captures earlier JSON loading; worker startup before discovery is
not sampled. Samples are lower bounds, kernel counters are approximate and
monitoring may perturb runtime. Do not sum noncontemporaneous process peaks or
attribute/subtract inherited rusage peaks. First-run research unit tests ran
during early admission; host load/power state otherwise uncontrolled. These are
descriptive controls, not controlled speed comparisons or latency distributions.

## Observations

| Measurement | First | Repeat |
| --- | ---: | ---: |
| Cold admission, seconds | 18.490 | 18.490 |
| Churn admission, seconds | 60.013 | 60.005 |
| Final drain, seconds | 4.983 | 6.173 |
| Complete control, seconds | 85.381 | 86.723 |
| Parent CPU, seconds | 22.994 | 21.934 |
| Exited-child CPU, seconds | 81.880 | 83.679 |
| Publications | 30 | 31 |
| In-flight jobs / pending markers, maximum | 1 / 1 | 1 / 1 |
| Diagnostic expected metadata, maximum | 2 | 2 |
| Churn captured-latest events | 3011 | 3013 |
| Churn superseded events | 2989 | 2987 |

All 1850 initial additions are represented at their first ledger advancement.
Approximately half the subsequent churn events are superseded, largely transient
deletions followed immediately by reinsertion. A bounded queue does not imply
one source revision of staleness. Window backlogs are in revisions, not jobs:

| Churn window (1000 events) | p95 advancement lag, seconds first/repeat | Peak backlog first/repeat | End backlog first/repeat |
| --- | ---: | ---: | ---: |
| 0 | 6.172 / 6.122 | 665 / 640 | 566 / 382 |
| 1 | 6.280 / 6.009 | 640 / 636 | 323 / 427 |
| 2 | 6.739 / 6.328 | 650 / 618 | 375 / 527 |
| 3 | 6.979 / 5.926 | 694 / 669 | 683 / 561 |
| 4 | 6.849 / 6.290 | 734 / 643 | 629 / 333 |
| 5 | 7.144 / 6.436 | 754 / 674 | 564 / 350 |

Cold p95 advancement lag is 5.780/5.729 s. Maximum churn lag is 7.525/6.724 s;
per-window admission-drift p95 spans 9.498--12.410/7.643--10.024 ms, with maximum
94.877/114.881 ms. First-run later-window peaks rise; repeat peaks vary. These
finite observations do not establish an asymptotic lag bound or no long-term
trend. Both runs continue publishing during churn, then drain to revision7850,
442 topics and the exact baseline memberships. First revision1 has two
unreviewed phrases after1.137/1.192 s, while114/120 sources are admitted.

Every publication passes captured complete source-version/revision/scope checks.
Final checkpoint, source versions, snapshot semantics and all non-runtime result
fields match exactly across the repeat; intermediate publication sequences and
classification counts vary with timing. A scalar final-state audit imports no
producer: reconstruct each last version from1850 initial additions and3000
delete/reinsert pairs, and compare all1850 original four-field PhraseSources,
versions and current code pins for both captures. An initial audit mistakenly
compared the full earlier embedding-stage record to the four-field PhraseSource;
the corrected audit explicitly projects id/provider/thread/text, matching the
frozen protocol. No experiment or source content changed.

Complete time includes JSON load/validation (1.461/1.633 s), worker startup,
admissions, all publications, final drain/shutdown, pin rechecks and internal
sampling before result serialization. CPU scopes above exclude the external
observer. Streaming parent sample maxima are RSS196616/196908 KiB and
PSS172211/182356 KiB; worker RSS165136/165584 KiB and PSS140745/140940 KiB.
The external parent observer captures the earlier JSON load: RSS400292/403156 KiB,
PSS376331/395581 KiB across3158/3373 samples. Parent rusage peaks404296/404924 KiB;
worker rusage reports the same values despite its own VmHWM165136/165584 KiB,
consistent with retained pre-exec launcher high-water marks. No process peaks
are summed, and these are not whole-ingestion/fleet memory measurements.

## Scope and limits

The reference still has a 2048-source/8 MiB capacity, and refreshes corpus lexical
statistics. A fixed 78.5-second replay can show queue and final-state behavior at
this cohort/rate; it cannot prove indefinitely sustainable throughput, scaling,
independent label utility, native complete-ingestion costs, application search
isolation, supported MacBook behavior, durable shared publication or Rust parity.
Those acceptance gates remain open. No topic method is production-selected.

Four fixtures check schedule wraparound and source preservation, superseded
delete/reinsert accounting, latest-version/deletion correctness, unknown actions,
and full captured publication scope/version rejection. They join normal research
test discovery; all 693 tests passed in 10.734 s. Synthetic fixtures are not human
topic judgments. Sources, checkpoints and per-publication details remain private
outside Git under mash-artifacts; only aggregate results belong in this report.

Artifacts: `topic-publication-churn-{first,repeat}-20261003.json` and logs;
`topic-publication-churn-{first,repeat}-parent-memory-20261003.json`;
`topic-publication-churn-scalar-audit-20261003.log` and focused/full test logs.
Manuscript procedures and results are updated alongside this control. The final
156-page manuscript builds with resolved references; new-section pages149--150
were rendered with the isolated PDF renderer and visually checked. These are
local research/build checks, not a GitHub CI or production deployment claim.
