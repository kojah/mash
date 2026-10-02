# Complete source evidence in fixed sparse seed discovery

This is a deliberately limited insert-only discovery control, not a production
topic method. It follows the source/context and dual-ranking studies. Improved
retrieval representation does not establish useful topic groups or labels.

## Frozen protocol

Reuse the complete audited QMSum training source and original/context TF-IDF
matrices, baseline results SHA256
`e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488`.
All 162 meetings/88,336 source turns remain accounted for. The 88,049 common
nonempty vectors enter discovery, including unknown/multi-annotated sources;
287 empty vectors remain explicit exclusions. No labels, names, speakers or
gold membership drive construction. No LLM, inference, download or held-out read.

Each meeting starts an empty seed set. Process all nonempty source rows in
forward and reverse source order, separately for original and causal context.
Choose the seed with highest cosine; exact ties select first-created seed.
Join at cosine >=0.55; otherwise create a seed fixed at that original source
vector. Maximum 2,048 seeds per meeting exceeds observed maximum 1,832 turns;
no cap overflow or forced assignment. Seeds never move and assignments are not
revisited. This is a fixed control, not centroid maintenance, BIRCH or GRINCH.
Meeting-local scope is not the required group-wide production topic namespace.

Labels use original source unigram counts exactly once per assigned source,
never copied contextual counts. Fixed score is normalized class frequency times
`log(1 + mean_class_length / total_term_class_frequency)` times the frozen
whole-training original-source smooth IDF. Top eight terms, descending score,
lexical ties, no stoplist. Every descriptor has positive original source count.

Evaluate complete within-meeting unordered pairs of singleton-known sources;
unknown/multi-membership sources stay in groups but are excluded gold support.
There are 81,181 supported singleton rows and 29,018,426 known pairs per policy.
Report exact contingency counts and micro precision/recall/F1, all group sizes
and lexical token overlap with source annotators' topic names after construction.
The latter is an automatic lexical proxy, not semantic label quality or an
independent coding-session/topic-tree evaluation.

## Results

| Evidence / order | Groups | Singleton groups | Pair precision | Pair recall | Pair F1 |
|---|---:|---:|---:|---:|---:|
| Original / forward | 52,620 | 45,817 | .354524 | .025140 | .046950 |
| Original / reverse | 52,525 | 45,699 | .352188 | .025094 | .046850 |
| Context / forward | 24,672 | 1,027 | .982448 | .013171 | .025993 |
| Context / reverse | 24,646 | 993 | .982789 | .013184 | .026018 |

Original groups have median size one and maximum179/262; contextual groups
median four and maximum55/53. Context gives substantially fewer singletons but
even lower same-topic recurrence recall. High contextual pair precision must
not be treated as semantic success: overlapping adjacent windows share text.
After observing results, add a diagnostic excluding original distances <=4,
without changing fitting or policy. Label this post hoc, not a prespecified gate.

That diagnostic has 28,697,518 pairs per policy:

| Evidence / order | Nonoverlap precision | Nonoverlap recall | Nonoverlap F1 |
|---|---:|---:|---:|
| Original / forward | .341508 | .024591 | .045878 |
| Original / reverse | .339267 | .024552 | .045791 |
| Context / forward | .598689 | .000209 | .000418 |
| Context / reverse | .606515 | .000217 | .000435 |

Context-forward has only 1,826 true same-topic nonoverlap pairs versus 119,168
all-distance true pairs. The high all-distance precision predominantly reflects
adjacent shared context, not recurrence discovery. This is evidence against this
control as a production topic method. It does not rule out context for embeddings
or a different discovery method; no threshold tuning follows this failure.

Annotated-name token overlap means are .015927/.015864 original forward/reverse
and .017147/.017054 context, over 82,231 topic/source incidences each. Surface
overlap does not validate semantic names. The most frequent original-forward
descriptor terms include `disfmarker`5751, `it`4821, `yeah`4802, `that`3973,
`the`3432 and `vocalsound`3425. Context-forward includes `yeah`2228,
`vocalsound`1622, `disfmarker`1151 and `okay`1072. Counts are descriptor-list
occurrences, not document frequency or quality labels.

An original-source scan confirms 33,979 `disfmarker` and 16,301 `vocalsound`
occurrences as braced transcript markup. This motivates targeted source-preserving
markup and label-noise ablation, not a claim that deleting arbitrary filler
improves embedding quality. Fixed unigram labels and pure lexical seeds are
inadequate here; do not promote this control or tune thresholds on these outcomes.
The representation and discovery failures are distinct from the earlier RRF gains.

## Reproduction, verification and cost scope

Drivers `qmsum_seed_discovery.py` / `sparse_seed_topics.py`. Seven tests check fixed
seeds, ties/cutoff equality, reverse source ownership, unsupported gold, labels,
invalid input and exact emitted-JSON repeat behavior. Full enabled suite:449 pass.
Runs live under `/home/james/scratch/mash-artifacts/topic-qmsum-seed-{first,second}-20261002`;
memory reports `topic-qmsum-seed-memory-{first,second}-20261002.json`.
Use existing science Python, pinned `uv.lock`, and `memory_audit.py --output
MEMORY.json -- PYTHON research/topics/qmsum_seed_discovery.py --source SOURCE
--baseline BASELINE --output-directory FRESH`. No model assets are used.

The independent auditor `topic-qmsum-seed-audit-20261002.py` reconstructs every
seed decision, assignment and label from original matrices/source, and enumerates
known upper-triangle pairs independently of the contingency shortcut. Complete
source/count/vector evidence comes from the pinned successful baseline audit.
It shares SciPy's sparse-dot kernel. Terminal audit success verifies all 352,196
insert events and label scores with zero decision-score/label error, all source
coverage, pairs and summary proxies. No inference/Rust/lifecycle or semantic
quality parity follows.

Every nontiming first/repeat JSON field matches exactly. The run pair precedes
a publication-wrapper fix that canonicalizes integer JSON object keys during
`--compare`; scientific computation is unchanged. On-disk comparison uses the
corrected wrapper and a dedicated test rejects changed group sizes. Archived
code pins retain the original executed driver; do not rewrite them to newer code.
Final result SHA256 first/repeat:
`83d0b902b1833218a8be86b9888c35702e86808f5886a469facd369f8e1cc3d6` /
`4636b5af7abf2d0f98b5eb33ba8944c79c386d5e5707bc169fa41adfe5f53f0d`.
Auditor SHA256:
`4eeae1b380879fc9fbfb6320b394142ae354fc88b66bf2baba1111ce124e2028`.
Prerequisite baseline audit SHA256:
`81a90726da98a803e19ed59fbc881ecade7cdae52854b0a301b6342b4c8503b6`.
Terminal artifact `topic-qmsum-seed-audit-first-20261002.json` includes all
post-hoc nonoverlap counts/protocol; no hidden refit.

First after-import total wall98.867 s, CPU73.232 s, source/setup2.355 s.
Discovery wall original forward/reverse20.066/20.633 s, context20.095/19.077 s;
labeling4.719/4.462 and3.237/2.947 s. Each policy inserts88,049 rows; empirical
95th-percentile insertion order statistics are .3134/.3123/.3128/.3020 ms
(sorted index floor((n-1)*.95), not a production p95 claim). Timers exclude final
JSON/comparison and scientific imports; labels are final batch maintenance,
not online label updates. Full ingestion remains unmeasured.

Repeat setup5.699 s, total83.366 s, CPU71.475 s, rusage795668 KiB. It overlaps
the independent auditor and does not establish a clean timing comparison.
External repeat RSS796516/PSS782500 KiB, status high water796516 KiB, with2675
samples/one vanished read and median/max gaps32.73/111.16 ms. Both ledgers retain
offline events/labels; repeat comparison is a separate post-run process, outside
the monitored workload. Its memory is not included or attributed to discovery.

First rusage795996 KiB; external target RSS796652/PSS788584 KiB, approximate
status high water796652 KiB. There are2683 samples, one vanished read, median/max
gaps34.99/152.94 ms. External scope includes imports and final JSON, excludes
observer/other processes. Sampling maxima are lower bounds, monitoring intrusive,
status approximate, launcher high water may persist. No subtraction or claim
that retained offline ledgers represent production topic memory.

Beads bounded control `mash-c0d.19.2.20.8.1`; cleanup ablation `.8.2`;
parent discovery/quality `.8` remains incomplete. Edits/deletions, multi-topic
assignment, semantic ancestors, independent coding-quality evidence, complete
ingestion budget and selected Python/Rust differential gates are not implemented
or passed by this control.
