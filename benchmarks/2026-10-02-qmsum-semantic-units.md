# Zero-inference bounded semantic context at fixed discovery

This study isolates unit evidence after rejecting raw/masked single-turn semantic
seed discovery. It changes only bounded causal vector aggregation, retaining the
same discovery and original-source label policy. It is not an episode detector,
concatenated-text encoder, selected topic algorithm or semantic hierarchy.

## Protocol frozen before outcomes

Complete pinned QMSum training source:162 meetings/88,336 turns, source SHA256
`55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`.
Frozen installed quantized MiniLM representations capture
`05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98`;
raw matrix `86d67c9f1a0fd47fa1409633805951cdb9cfc2247a400cd8f8717e06f952238c`.
Earlier complete vector repeat, independent source/status/origin audit and
40-input separately implemented inference replay are prerequisites. No new
inference calls, downloads, LLM, remote model or held-out source.

In original within-meeting order, sum the current raw unit direction and at most
four immediately preceding common-ready original raw directions in float64,
then normalize. Use whole turns with cumulative original raw model tokens <=512
(including each turn's special tokens); stop at the first unsupported prior
source, meeting boundary or budget. Never compact across source gaps. Keep every
source ordinal, context start, total tokens, stop reason and support status.
Norm<=1e-12 explicitly abstains instead of producing a searchable zero vector.
This averages existing embeddings; it does not embed their concatenated text.
Context remains causal even when discovery arrival is reversed.

Common source support starts at84,434 rows inherited from the prior raw/masked
comparison. Any cancelled context direction is excluded from both policies but
retained in the source ledger. It is not a claim that all raw-ready inputs require
masking support. The fixed immutable-source seed control remains cosine.55,
equality joins, first-created ties, cap2048 per meeting, forward/reverse arrivals
and meeting reset. Labels retain exact-marker-cleaned original unigram counts,
fixed original corpus IDF, class scoring and eight terms with lexical ties.
No contextual label duplication, broad filter or threshold grid. Exact original
raw meeting-record replay is mandatory wherever support is unchanged.

Singleton-known gold pairs are measured all-distance and at original-distance>4,
the latter prespecified to exclude overlapping context components. Unknown and
multi-annotated sources still enter discovery; they are not forced into singleton
gold. These broad source topics are not exhaustive fine-grained coding-session,
multi-tag, label or semantic-ancestor annotations.

Before outcomes, freeze a paired meeting bootstrap:2000 draws of162 original
meeting IDs with replacement, NumPy default_rng seed20261002; context-minus-raw
micro nonoverlap precision/recall/F1 from sampled contingency totals, forward and
reverse separately, percentile2.5/97.5 intervals. Zero denominator scores zero.
This measures exploratory training-meeting uncertainty, not held-out or coding
quality, and does not refit any assignment, label or cutoff.

## First complete result

All84,434 inherited ready rows remain supported:zero cancellations and3,902
unsupported sources explicitly accounted for. Stop reasons:631 meeting root,
72,004 five-turn limit,10,398 unsupported prior,1,401 whole-turn token limit,
3,902 unsupported current. Every original raw meeting record replays exactly.
Gold support stays77,892 singleton-known rows,27,346,484 all-distance and
27,049,334 far pairs; this is a matched comparison.

| Policy | Groups | Singletons | Far precision | Far recall | Far F1 |
|---|---:|---:|---:|---:|---:|
| Raw forward | 30,328 | 20,794 | .320282 | .046487 | .081190 |
| Raw reverse | 30,396 | 20,739 | .329192 | .044663 | .078654 |
| Context forward | 5,662 | 339 | .320677 | .159298 | .212858 |
| Context reverse | 5,458 | 915 | .319177 | .162015 | .214931 |

Median group size1 raw/5 context; largest groups356/288/632/745. All-distance
F1 .082737/.080182/.227343/.229043. Empty labels110/116/0/1. Context grouping
reduces fragmentation and improves recurrence beyond overlapping neighbors, but
precision around32% and arrival-dependent memberships/coverage remain concerns.
This window can borrow unrelated prior subjects across an unrecognized topic
shift; it is not validated reference resolution for approvals or short replies.
Source-count descriptor presence is not independent semantic label quality.
Retain cheap causal aggregation as candidate evidence for the next actual
discovery comparison; do not select the fixed-seed algorithm, semantic hierarchy
or production unit policy from broad meeting annotations alone.

Prespecified paired-meeting bootstrap context-minus-raw nonoverlap changes:

| Arrival | Precision delta [95%] | Recall delta [95%] | F1 delta [95%] |
|---|---:|---:|---:|
| Forward | .000396 [-.009930,.011977] | .112811 [.094763,.131952] | .131668 [.115178,.148720] |
| Reverse | -.010014 [-.023335,.002132] | .117352 [.100203,.136589] | .136276 [.121277,.152308] |

Recall/F1 intervals are positive in this exploratory source corpus; precision
intervals include zero. This is not a precision-equivalence test, held-out
generalization or validated coding-topic improvement. Bootstrap script SHA256
`ff5cd636e4c10e95e686631409ca84dba901c3695c94a4e807fe63f0810fb2e5`;
terminal report `topic-qmsum-semantic-units-bootstrap-first-20261002.json`.

First capture SHA256:
`e22d6878b6640dec38f7f2ced42956ae73abfaa3e1502741743a82d44c3eff33`.
After-import wall196.840 s,CPU127.114 s,setup27.887 s,rusage2336624 KiB.
Aggregation once across all source rows totals1.368 s (count `context_forward`
timing entries only; both discovery orders record the same once-computed value).
Raw forward/reverse discovery53.423/53.194 s; context20.927/19.523 s;
labels5.260/4.436/2.462/2.560 s. No new inference calls. These subscopes are
offline lab measurements, not full-ingestion costs or a production speed gate.
External target VmRSS2337724/smaps RSS2337584/PSS2329734/status2337724 KiB,
2546 samples,one vanished,median/max gaps65.33/627.24 ms. The process retains
previous full discovery/vector ledgers and all compared outputs; inference-worker
memory is not inferred from these peaks. Imports/JSON/repeat comparison are
outside stage timers but within external scope; observer memory is excluded.

## Validation and reproduction

`semantic_units.py` defines typed support/stop reasons and bounded source-lineage
aggregation. Four edge tests cover whole-turn token limits, unsupported gaps,
causality, the five-turn cap, zero-sum abstention and invalid directions/support.
The complete enabled research suite passes462 tests. The driver is
`qmsum_semantic_units.py`, accepting `--source --vectors --discovery --labels
--baseline --output-directory [--compare FIRST/results.json]`. All output uses
fresh directories outside Git, under `/home/james/scratch/mash-artifacts`; wrap
measured jobs with `memory_audit.py --output MEMORY -- COMMAND`.

The initial harness stopped before outcomes because a whole-capture comparator
requiring top-level timing fields was applied to a meeting record. Replace that
with exact record equality; preserve failed `topic-qmsum-semantic-units-first-20261002`
and its terminal nonzero memory record. The scientific protocol is unchanged.
The corrected full-source run uses `topic-qmsum-semantic-units-final-first-20261002`.
The corrected run and fresh repeat both completed successfully; every nontiming
field, all source lineage, raw replay, memberships/events/descriptors and pair
contingencies match exactly. Repeat capture SHA256:
`cf13aabb719b6bc4389891b851ff21373c8cc270f7c569adf54b9578b869e2cf`.
Repeat wall142.125 s,CPU124.496 s,setup14.558 s,aggregation once1.060 s,
rusage2329544 KiB. External VmRSS2330664/smaps RSS2330388/PSS2219683/status
2330664 KiB,2532 samples,one vanished,median/max gaps59.69/171.55 ms.
Repeat overlaps the independent auditor; peak/timer variation is not a speed
conclusion. Stage timers exclude comparison/JSON/imports, external scope includes
them; whole prior capture/outputs are retained offline.

Independent audit reconstructs complete88,336-source contiguous context lineage
using a reverse candidate walk and ascending explicit vector sums, verifies
support/limits and exact raw replay, then independently replays all337,736 seed
decisions and scalar labels and enumerates every known upper-triangle pair,
including original-distance>4 directly instead of nearby-pair subtraction.
All ownership, counts, group sizes and summaries match; maximum seed-score/label
error zero. Auditor SHA256:
`ec4bea4d1e81ff65cf58e90dd5b83352b8fe5ca6fe0c5871e37bf56d4d7ff328`;
terminal `topic-qmsum-semantic-units-audit-first-20261002.json`. Same NumPy
normalization/scipy dot remains; source/count/frozen-vector audits are separate
prerequisites. Context matrices are reconstructed and checked through complete
seed scores rather than a separately persisted matrix. No independent engine,
Rust, coding quality, multi-tag or semantic ancestry evidence follows.

Offline timings and retained multi-policy ledgers do not establish production
throughput/memory. External RSS/PSS are sampled lower bounds; status counters
approximate, monitoring intrusive, launcher high water may persist. Report
sample gaps, vanished reads, overlap and timer/observer scopes; never subtract
peaks. Full-ingestion throughput, edits/deletes/restarts, multi-tags, independent
label/ancestor quality and selected Rust parity remain open. Bounded child bead
`mash-c0d.19.2.20.8.3.1` is complete; broader method-selection work remains active.
P0 follow-up `mash-c0d.19.2.20.8.3.2` holds this candidate evidence fixed for
actual BIRCH/grafting or bounded graph comparisons, with frozen algorithm-specific
settings before outcomes; different algorithm cutoffs are not interchangeable.
