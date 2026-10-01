# Bounded source-fragment aspect controls

Mechanical finer splitting is not promoted. Two fresh processes reproduce all
nonmeasurement fields across 24 controls; the 111-test lab suite passes. This is
exposed development evidence, with no LLM or paid inference.

## Procedure

Reuse the 91-record/257-structural-span oracle source scope and frozen 50-pair
review from the [matched-unit experiment](2026-10-01-matched-source-unit-grouping.md):
14 overlapping, 33 distinct and three uncertain task-aspect pairs. Reviewer text
never enters features. These correlated known-case coding-assistant judgments
are not independent or representative truth.

Before new predictions, freeze original spans versus sentence/semicolon pieces
versus overlapping16-word windows with four-word overlap. Sentence boundaries
are terminal punctuation followed by whitespace+uppercase/digit, or semicolon
followed by whitespace. Protect paired inline backticks/double quotes and unmatched
opener suffixes. Cap at eight pieces by merging contiguous adjacent groups.
Windows also cap at eight, expanding width deterministically for long text to
retain every source character rather than discard a tail. Words are non-whitespace
runs. Code/headings remain intact; original Unicode offsets/hashes bind every child.

No rewriting, negation stripping, approval recognizers, reviewer topic names,
comma/arrow special cases or downloads. An inspected known summary uses arrows;
that exposure is disclosed and no special splitter is added. Preserving negation
text does not resolve grammatical scope, references or endorsement.

Installed quantized MiniLM singleton inference runs offline, one CPU thread,
512-token preflight and no truncation. Every mode retains all257 parents.
This dense-only comparison includes the eight zero-lexical spans excluded from
the preceding249-span lexical-intersection comparison.

Each original pair receives maximum child-pair cosine. Native overlap means any
shared nonnoise cluster; the separate centroid diagnostic means any shared
cosine≥0.7 affiliation. Max-of-many scoring can inflate unrelated matches.
Unchanged fixed settings: BIRCH radius0.4/branching64/no global clustering;
GRINCH-style all-internal-merger cosine0.7; average cosine-linkage distance0.3;
HDBSCAN minimum cluster/sample5, EOM. Four methods run forward/reverse; no tuning.

## Results

| Representation | Vectors | Pair AP / ROC AUC | BIRCH native TP / FP | GRINCH-style native TP / FP | HDBSCAN native TP / FP |
| --- | ---: | --- | --- | --- | --- |
| Original | 257 | 0.855 / 0.918 | 3 / 0 | 3 / 0 | 13 / 28 |
| Sentence/semicolon | 407 | 0.803 / 0.877 | 3 / 0 | 3 / 0 | 8 / 13 |
| Windows | 471 | 0.792 / 0.883 | 3 / 0 forward; 1 / 0 reverse | 2 / 0 | 14 / 33 |

AP/AUC are raw pair-ranking diagnostics, independent of clustering/order, not
validated topic quality. TP/FP refer to14 positives/33 negatives. Uncertain
pairs are excluded, noise−1 never counts as a shared topic, missing evidence
abstains; here every reviewed pair is represented. Average linkage matches
GRINCH-style TP/FP in all modes.

Neither splitting method recovers any of the six summary-to-family overlaps
with GRINCH-style native or centroid0.7 assignment. Cosines are0.361–0.487
original,0.401–0.479 sentence/semicolon and0.434–0.548 windows. Windows improve
some individual scores but reduce overall ranking and do not cross the cutoff.
Two legacy counterparts rise from0.520/0.288 to0.606/0.331 (sentences) and
0.688/0.330 (windows), also missed. Lowering a threshold on these exposed cases
would not establish a robust remedy.

Sentence BIRCH produces360–364 clusters, GRINCH-style356–357; windows produce
434 and427 respectively. Predominantly singleton grouping persists. HDBSCAN
forms two original clusters, five sentence clusters and two window clusters;
window partitions conflate all33 distinct reviewed pairs. Original HDBSCAN here
yields13TP/28FP versus6TP/4FP on the preceding249-span intersection under
otherwise fixed dense settings. The changed-scope comparison flags sensitivity
to short/noisy inputs; it is not a dedicated removal ablation or independent
robustness evaluation.

All HDBSCAN centroid-diagnostic results are0TP/0FP. Window BIRCH centroid results
are3TP/0FP forward and2TP/0FP reverse. Other BIRCH/GRINCH/average centroid results
match native counts. Neither rule is calibrated confidence. Labels remain
source-supported candidates without independent quality judgments; overlapping
windows duplicate source counts, so label-frequency changes cannot prove gains.

Only three threads appear in reviewed pairs, with cross-thread dependence and
concentrated family/summary comparisons. Robust group-separated operating-point
calibration is explicitly not performed. More independent reviewed units are
required; reserved groups are not consumed for fitting.

## Runtime and provenance

Xenia, Python3.11.15, NumPy2.4.6, sklearn1.9.1, ORT1.30.0, Tokenizers0.23.2;
numerical/encoder threads one. All modes combined:811 unique calls,324 cache
hits, maximum439 tokens, model inference2.43–2.53s.

| Mode | New calls / cache hits | Marginal encoding, ms | Request p95, ms | GRINCH-style grouping, ms |
| --- | --- | --- | --- | --- |
| Original | 256 / 1 | 1,346–1,518 | 11.1–16.1 | 265–316 |
| Sentence/semicolon | 232 / 175 | 692–719 | 4.4–4.5 | 547–791 |
| Windows | 323 / 148 | 819–919 | 3.5–4.1 | 766–887 |

Segmentation costs1.0–1.9ms original,3.9–4.2ms sentences,2.6–2.8ms windows.
Encoding includes preflight/cache handling; p95 includes cache hits and is not
cold-update/full-ingestion p95. One shared cache makes mode costs marginal,
not standalone. Grouping includes validation/cuts. Complete24-control process
wall15.7–16.5s, CPU15.1–15.9s, reported rusage956MiB.
The [memory audit](2026-10-01-topic-memory-accounting.md) identifies pre-exec
peak contamination; attribution to this workload is withdrawn until remeasured.
Ranges cover two processes/orders, not uncertainty intervals. The ≤10% full
production-ingestion gate remains unverified.

Modules: `fragments.py`, `fragment_metrics.py`, `fragment_experiment.py`,
`compare_fragments.py`; exact source validation and pair accounting are shared.
Tests cover complete Unicode coverage, bounded work without tail omission,
protected syntax/negation text, exact parent identities, multi-vector accounting,
noise, missing evidence and strict repeated grids.

Private root `/home/james/scratch/mash-artifacts/fragment-aspects-20261001`:

- `validated-first.json`: SHA256 `86d7e33f99b76976814a0760f1102767f31f7568c6951154e97601f2062c49f0`
- `validated-second.json`: SHA256 `3a4709c756966f072a6e742fabe344d2e7971ce782f202198be183670b2d144e`
- `first.json` is exploratory, before CPU/p95/version instrumentation, excluded
  from final paired verification.

Do not promote mechanical splitting or tune it to one summary. Wider source-
backed review and context-sensitive unit/label validation remain necessary.
