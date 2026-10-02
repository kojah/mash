# The 128-leaf prototype loses topic coverage on the wider frozen cohort

## Cohort and procedure

This experiment expands the end-to-end deferred prototype to an already frozen,
exposed development selection of 128 Claude Code/Codex threads. Selection capture
SHA256 `270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`
contains 1,812 deduplicated directions. `topic_cohort_demo.py` restores all
1,850 admitted user-message occurrences—including 38 exact-input duplicates—
from immutable SQLite SHA256
`251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`.
Validate every captured input hash, original identity/ordinal, vector hash and
direction against the source. There is no new sampling, model inference,
download, LLM or paid call. Original messages remain distinct occurrences, not
independent semantic examples merely because their IDs differ.

Of 8,888 canonical user-role records, 7,038 are explicit empty projections with
no vectors. These are separately retained in a private source ledger, not
treated as unfinished embeddings or routed into topic clusters. Admitted-text
coverage is 1,850/1,850. Admitted text totals 1,527,917 UTF-8 bytes, maximum
25,192 bytes/message, within the unchanged prototype budgets of 2,048 active
sources, 8 MiB text and 128 leaves. The baseline snapshot is a research source,
not proof of completed production indexing/readiness or native completeness.

Use unchanged .55 cosine routing and one coalescing background worker with
refresh requests every 16 events. Arrival is round-robin admitted messages in
native order within each selected thread; reverse arrival is simulated. Deferred
soft memberships and source-pooled lexical hierarchy/labels preserve the earlier
settings. Out-of-capacity sources remain stored; no forced nearest assignment.
Checkpoint/freshness, stale publication, in-memory forget/reinsert and restart
checks operate on the actual cohort. Semantic edits remain synthetic tests.

## Results

| Measure | Forward | Reverse |
| --- | ---: | ---: |
| Routing leaves | 128 | 128 |
| Anchored source occurrences | 785 | 779 |
| Sources hitting leaf capacity | 1,065 | 1,071 |
| Sources with no deferred topic | 1,028 | 1,017 |
| Multi-tag sources | 107 | 93 |
| Weak tree nodes | 81 | 91 |
| Zero lexical leaves | 3 | 4 |
| Co-membership pairs | 22,769 | 27,872 |

Only 822/1,850 forward sources (44.43%) have a final topic. Deferred matching
can recover some capacity-rejected sources after centroids move, so routing
capacity and final untagged counts are distinct. Forward/reverse pair Jaccard
is .675079 (2,360 forward-only and 7,463 reverse-only pairs). Forward has
57 singleton leaves and 70 weak-label leaves; 58 leaves have at least two
supported label terms. All 128 threads have some tag, which conceals the large
message-level coverage loss if reported alone. Labels and ancestors remain
unverified semantically. Repeated input occurrences can contribute support;
support is not independent judgment or topic accuracy.

Both runs match every stable artifact, excluding runtime/scheduler observations.
All 1,850 sources persist; deletion removes the selected original source from
the tree; stale snapshots are rejected; checkpoint and refreshed restart match.
The strengthened `topic_stream_audit.py` imports no producer code and verifies
original hashes, all 1,850 stored directions via independent scalar signed-byte
normalization, 7,038 excluded records and the EXACT set of 8,888 canonical
identities, all 255 node source unions and 128 thread tag/ancestor unions.
It does not reconstruct routing decisions, label scores, native parsing or
semantic quality. The research suite passes 636 tests in 5.867 seconds.

The cap is unsuitable as a primary topic-assignment policy for this cohort.
Retaining data and passing lifecycle invariants do not compensate for losing
topics on more than half the admitted messages. Next evaluate adequate explicit
cluster capacity without lowering similarity thresholds, discarding outliers or
forcing merges; measure the resulting refresh/scaling cost before promotion.

## Scoped runtime and memory

Load/validation takes 1.833/1.739s. Forward ingestion through final topic
publication takes 14.066/15.665s; foreground upsert p95 is 22.13/23.91ms.
Seven background refreshes complete in each forward run. Whole experiment wall
time before artifact serialization is 42.300/47.496s; CPU 43.145/46.694s.
This includes both orders and multiple full lifecycle/restart refreshes, not
production ingestion with models, parsing, persistence and networking.
Python GIL/background contention, host load, caches and monitoring affect the
foreground. The <=10% complete-ingestion overhead target is not demonstrated.

External smaps target RSS maxima are 479,144/693,352 KiB; PSS
471,186/685,436 KiB. Approximate status HWM is 505,980/697,476 KiB.
Rusage sampled BEFORE artifact serialization is 378,892/381,332 KiB; this is
not a final process peak and may preserve launcher peaks. Do not subtract or
attribute these counters to a worker alone. The full external target includes
imports, original selection loading, all phases and substantial research JSON
serialization. Repeat additionally loads and compares the previous artifacts.
`results.json` is 42.7 MB and `checkpoint.json` 17.5 MB, so this instrumentation
does not represent a production topic worker's memory. There are 1,593/1,723
samples, one/two vanished reads and maximum gaps 45.36/59.04ms. Samples are
lower bounds; children/observer memory excluded, monitoring intrusive.

Private outputs and inspection HTML are in `topic-128-thread-first-20261002`
and `topic-128-thread-repeat-20261002` under mash-artifacts, with matching memory
sidecars and `topic-128-thread-audit-final-20261002.json`. No source text,
labels or provenance enter Git. Source-ledger and code hashes are captured.
No production algorithm is selected; independent coding quality, full ingestion
performance and selected-method Python/Rust parity remain open.
