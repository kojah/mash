# Frozen phrase/dense corroboration: coverage risk, no promotion

## Procedure and scope

User explicitly requested a bounded ensemble test and a generous ingestion
budget. `research/topics/ENSEMBLE_PROTOCOL.md` was committed as f23b179 before
implementation/results. Compare unchanged source-aware phrase coverage against
the same proposals plus other-thread semantic corroboration at fixed cosine
0.55. No cutoff/weight/stoplist sweep, new inference, LLM or paid call.
The lexical score/order/eight-tag cap, phrase IDs and literal source evidence
remain intact; rejected tags are not refilled. Rare subjects stay provisional.

Reuse the exposed 128-thread/1,850-source checkpoint and provenance capture.
Checkpoint/capture hashes are frozen in the protocol; represented text hashes
are verified against original captured provenance. The entire lexical snapshot
matches the prior streamed phrase allocation control after separating its
arrival revision/version counters from discovery semantics. These are captured
decoded embeddings, not new independent native/database parsing or held-out gold.

Geometry averages unit-normalized vectors once per distinct input/thread and
normalizes the result. Compare to the normalized mean of other supporting
threads' directions, excluding the complete target thread. Corpus recurrence
needs two threads and two distinct inputs. Undefined directions abstain.
Incremental geometry changes only affected phrase/thread buckets; its source
validation and current query implementation still scan ledger metadata.
The lexical snapshot recomputes corpus statistics: the complete pipeline has
**not passed the online requirement**. Geometry buckets before full completion
do not demonstrate online semantic topics or sustained-arrival publication.

## First-run observations

| Diagnostic | Phrase baseline | Phrase/dense |
| --- | ---: | ---: |
| Topics | 442 | 283 |
| Corpus-recurring topics | 291 | 132 |
| Provisional topics | 151 | 151 |
| Thread tags | 720 | 538 |
| Sources without topic evidence | 769 | 1,022 |
| Sources with multiple tags | 413 | 318 |
| Threads without tags | 0 | 6 |

Tag rejection removes 182/720 proposals. Losing evidence on another 253 sources
and all tags on six threads is a coverage risk; fewer labels is not measured
precision. Labels themselves are unchanged phrases. Without independent review,
we cannot tell whether removals preferentially discard noise or meaningful
secondary subjects. Long/multi-subject message vectors can dilute one phrase's
direction; an all-peer mean can also dilute a genuine subgroup. Conversely,
similar generic language can survive despite an unhelpful lexical label.
Do not promote this rule or tune the threshold against these outputs.

First-run lexical snapshot 4.085s, geometry insertion total 5.224s (p95 7.896ms
per source), proposal filtering 9.578s. Whole control including source loading,
JSON recreation, reverse recreation, duplicate checks and final hash rechecks
takes 59.679s wall/54.248s CPU after initial hashes, before output serialization.
Process rusage peak 1,452,628 KiB includes multiple retained model/checkpoint
copies and imports, not an isolated worker or a full ingestion measurement.
One OMP/OpenBLAS/MKL worker in the pinned CPython3.11 scientific environment on
Xenia; host load/power/CPU scheduling are uncontrolled. No performance cutoff
is inferred from the user's now-generous budget.

## Verification and decision

Four fixtures cover own-thread exclusion, provisional subjects, duplicate votes,
idempotence, edits/deletes/restart/reverse order, atomic invalid replacement,
undefined/overflow geometry and exact source identities when tags disappear.
All 681 research tests passed in 6.879s with the two author references supplied.
Real-cohort checks verify checkpoint recreation, reverse final geometry,
duplicate invariance/cleanup and exact lexical baseline semantics.

An exact repeat matches every non-runtime field. Repeat lexical snapshot 4.725s,
geometry inserts 4.683s (p95 6.939ms), filter 9.942s, whole control 63.888s wall/
55.005s CPU, process rusage peak 1,453,264 KiB. The independent auditor imports
the shared token scanner but no ensemble/phrase producer, recomputes all 720
decisions with scalar arithmetic, and verifies thread/source memberships.
States: 346 corroborated, 192 provisional, 182 uncorroborated; maximum cosine
error 1.67e-15, minimum distance from cutoff .001310. This is numerical/source
agreement, not independent semantic review or native parsing.
The final audit also reconstructs every surviving topic's exact thread/source
union. The manuscript builds with resolved references; rendered pages 147--148
were visually inspected. This is paper validation, not topic-quality evidence.
No independent primary/secondary quality review, complete online publication,
persisted production lifecycle, full ingestion comparison or Rust parity is
established. Keep `mash-c0d.19.2.40` open for these gates; no production selection.
This completes neither the topic feature nor the production-readiness goal.

The first harness comparison wrongly included store arrival counters; corrected
comparison verifies every semantic field. A subsequent capture was rejected by
the final provenance check because input-validation code changed while running.
Both failed logs remain; the unchanged final capture is authoritative. Neither
failure caused algorithm parameter tuning.

Private artifacts under `/home/james/scratch/mash-artifacts`:
`topic-ensemble-final-{first,repeat}-20261002.json` and logs,
`topic-ensemble-audit-20261002.py`, its initial/final JSON audits, focused/full
research test logs.
Raw histories and individual assignments remain outside Git.
