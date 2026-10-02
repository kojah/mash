# Actual BIRCH and GRINCH-style discovery on causal semantic evidence

This changes discovery while holding the audited zero-inference causal evidence,
source identities, support and label policy fixed. Earlier context averaging
improves nonadjacent recurrence at weak32% pair precision; it does not select an
online topic method. Settings are reused from prior online controls before new
outcomes, not calibrated or optimized using this corpus.

## Frozen complete protocol

Use all162 QMSum training meetings/88,336 turns and84,434 common-ready sources.
No held-out text, LLM, new inference, model download or truncation. Source SHA256
`55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`;
frozen vector capture
`05642c1a77dab33d2940c39979b0ac4e4b420420debcb30ca529d7bad99bdf98`;
causal control capture
`e22d6878b6640dec38f7f2ced42956ae73abfaa3e1502741743a82d44c3eff33`.
Read only supplied pinned installed assets/captures, reconstruct current plus
at most four contiguous preceding raw directions, cumulative original model
tokens<=512, explicit unsupported gaps and normalized float64 sums. Verify
every original lineage against the audited capture; no cancellation here.

Run both original forward/reverse insertion orders, each meeting reset:

- Actual scikit-learn1.9.1 BIRCH, radius.4, branching64, `n_clusters=None`, one
  copied point per `partial_fit`, final `predict` in original order. Copying
  prevents model-owned CF state from aliasing immutable source evidence.
- Existing GRINCH-style exact-neighbor normalized-centroid `Tree(Mode.GRINCH)`,
  stable arrival/node-serial ties, cap2000 leaves/1024 dimensions, rotations,
  strict mutual-preference grafts and LCA-bounded restructuring. Final cut.7
  accepts a clade only when every internal merger meets the cutoff.

These are different uncalibrated operating points; a radius is not a cosine
threshold. The GRINCH-style prototype differs from the reviewed author Scala
implementation (float64/exact neighbors/linkage/bounded restructuring); it is
not author execution, reference parity or a proof of the paper's theorem.
Native CF/grafting topology is internal geometry, not semantic topic ancestry.
Final predictions/cuts evaluate the final state, not stable historical online
tags; edit/delete/restart/global namespace behavior is not supplied.

Canonicalize final groups lexicographically by original source index and verify
complete unique ownership. Labels use unchanged exact-marker-cleaned original
counts, source IDF, class score/top8/lexical ties, not duplicated contextual words.
Measure singleton-known all-pair and original-distance>4 recurrence, with unknown/
multi-annotated sources still entering discovery. This excludes overlapping
context components. Gold support stays77,892 known rows/27,346,484 all-pairs/
27,049,334 far pairs and does not validate coding-session labels or ancestors.

Prespecified per-native-topic support keeps zero support/zero-far-pair topics
explicit, separates1-20 supported singleton-known turns (rare) from>20 (common),
and reports exact far relevant/recovered positives. No topic names enter features.
Forward/reverse adjusted Rand score is per meeting plus unweighted and
source-row-weighted means: stability, not correctness. No parameter grid.

## First BIRCH result

| Arrival | Groups | Singletons | All precision | Far precision | Far recall | Far F1 |
|---|---:|---:|---:|---:|---:|---:|
| Forward | 19,548 | 1,998 | .887767 | .620852 | .003967 | .007884 |
| Reverse | 19,445 | 2,618 | .891945 | .639988 | .004234 | .008412 |

Median group size4, largest60/55, empty labels2/1. High all-pair precision is
largely neighboring overlap: only32,383/34,560 far true matches out of169,796/
173,419 all true matches. Far recurrence is less than0.5% at this radius.
Mean/source-row-weighted order ARI .574879/.528650. Reject this operating point
as useful recurrence discovery; it neither disproves BIRCH at another calibration
nor selects another method by an incomparable cutoff.

725 annotated topics per order:589 common (76,258 supported rows),134 rare
(1,634 rows),two zero support. Rare far relevant pairs6242; recovered180/199
(2.88%/3.19%);17 rare topics have no eligible far positive pair. Common far
relevant8,156,750; recovered32,203/34,361. Zero-support topics remain explicit.

First after-import wall102.877 s,CPU89.094 s,setup16.805 s,rusage2020032 KiB.
External target sampled RSS2021272/PSS1919217/status high water2021272 KiB,
2670 samples,one vanished,median/max gaps38.00/168.20 ms. Forward/reverse
insertion36.490/36.091 s,final evaluation1.012/1.280 s. Overlaps GRINCH;
retained original capture ledgers, scientific
imports and output state belong to the offline workspace, not production model
memory. Insertion is separated from final prediction/validation/cut/state capture
and labels; imports/final JSON/comparison lie outside stage timers and inside
external scope. Sampled peaks are lower bounds, status approximate, monitoring
intrusive, launcher high water may persist, observer/other-process memory excluded
and no subtraction or full-ingestion throughput gate.

BIRCH fresh repeat completed successfully: every nontiming field, full CF state
hash, partition, label and contingency matches. First/repeat capture SHA256:
`fbaa98e8997b629542a44d70843d1ea0c8d63fa8b520d5ee2b13f0189bfb62da` /
`a57a3245ab0bc19f1e1b1b9ba0232f2779db4b1b120445d62bc7fded1b1762ae`.
Repeat wall140.403 s,CPU91.700 s,setup47.237 s,rusage2019332 KiB; external
RSS2020768/PSS1843902/status2020768 KiB,3064 samples,zero vanished,median/max
gaps41.10/679.84 ms. Repeat overlaps the independent auditor and GRINCH; timing/
peak differences are not algorithm speed/memory conclusions.

Independent audit completed successfully: direct pinned-library BIRCH replay,
all source causal reconstruction,38,993 CF subclusters across both orders,
physical sample/linear-sum/center accounting and radii, scalar descriptor scores,
complete known upper-triangle/far pairs, rare-topic counts and independently
computed scalar ARI. Maximum descriptor error zero. Auditor SHA256:
`670fbf21749a9b8258806415fce8d023b31da6fd10bc53cb6e60dad95e02f1c0`;
terminal `topic-qmsum-discovery-birch-audit-first-20261002.json`. Same BIRCH
library/numeric engines remain; replay verifies wrapper fidelity, not an
independently implemented BIRCH algorithm. Prior source/count/vector/unit audits
are prerequisites, and no coding quality/semantic ancestor/Rust gate follows.
## First GRINCH-style result and final-state audit

| Arrival | Groups | Singletons | All precision | Far precision | Far recall | Far F1 |
|---|---:|---:|---:|---:|---:|---:|
| Forward | 22,512 | 7,107 | .576092 | .441563 | .021639 | .041257 |
| Reverse | 23,930 | 7,523 | .583121 | .434318 | .017988 | .034545 |

Median group size2, largest217/173, empty labels19/25. Mean/source-row-weighted
order ARI .385297/.344461. Rare eligible far positives remain6242, recovered
742/605 (11.89%/9.69%); common recovered175,899/146,231. At these different
operating points GRINCH-style recovers more far positives than BIRCH, but overall
recall is only1.8-2.2%, order sensitivity substantial and insertion cost high.
Neither current cut/radius is selected. This is not an optimized algorithm
comparison or evidence that either algorithm universally fails.

First capture SHA256:
`c9587e541ff38478a8c074cf2f28992d187f03ed18c15e024fbebe7d46ee0b3d`.
After-import wall755.375 s,CPU648.139 s,setup17.083 s,rusage2019356 KiB.
Forward/reverse insertion368.280/332.652 s,CPU317.351/285.402 s;
validation/cut/state capture12.809/12.340 s. These two-order Python controls
overlap BIRCH/audit/repeat; they do not establish Rust or full-ingestion overhead.
External RSS2020676/PSS1853736/status2020676 KiB,17,332 samples,two vanished,
median/max gaps39.00/349.26 ms. Same complete-source/multi-output offline workspace
and sampling caveats apply; no production worker memory inferred.

Independent final-state audit completed successfully:337,412 live nodes across
both orders, exact aggregate/link hashes, leaf/source ownership, all merger
cosines and every-merger-cut canonical partitions. Source causal reconstruction,
scalar labels, all known/far pairs, rare counts and independently computed ARI
match; maximum merger/label error zero. v2 auditor correctly models insertion
normalization plus the separately normalized leaf linkage vector. Original
BIRCH auditor bytes/SHA remain preserved. v2 SHA256:
`fa4287bf4a5bc70418479bb3651f2901bd8bd89cb4551a077f666c8b373fd4fe`;
terminal `topic-qmsum-discovery-grinch_style-audit-first-20261002.json`.
This audits final state/cuts, not independent graft insertion decisions or author
Scala execution. Earlier source/count/vector/unit audits remain prerequisites.
The full fresh GRINCH-style repeat completed successfully: every nontiming field,
live-node/source hash, statistics, canonical partition, label, pair/support count
and order metric matches exactly. Repeat capture SHA256:
`20dfa33e2f8fc8ad6e25a70b16669b93820a01c995cbec099b91f15c34b0bd9e`.
Repeat wall640.047 s,CPU585.141 s,setup14.157 s,rusage2019732 KiB;
external RSS2021184/PSS1980943/status2021184 KiB,16,361 samples,two vanished,
median/max gaps36.64/236.49 ms. It overlaps final-state and crossspan audits;
timing/peak variation is not a speed/memory conclusion. No production method
is selected from this complete bounded comparison.

## Explicitly post-hoc returns across disjoint native spans

After weak frozen cuts, freeze a stronger diagnostic before computing it: merge
overlapping/adjacent original inclusive spans per native topic, assign exact
components to singleton-known supported sources and require original-distance>4
plus different native components. Positives are the same topic across disjoint
components; different-topic negatives remain. No new fits/cuts/label selection;
this does not replace the primary metric or claim preregistration.

All771 source spans need no same-topic merge;33 of725 topics have multiple spans.
Same77,892 known rows yield18,970,692 eligible pairs but only84,350 positives,
versus8,162,992 all-far positives. Most primary far positives remain within long
contiguous topic spans, rather than returns across separate annotated spans.

| Frozen partition | Return recall forward / reverse | True pairs forward / reverse |
|---|---:|---:|
| Causal seed.55 | .106722 / .096325 | 9,002 / 8,125 |
| BIRCH radius.4 | .000451 / .000865 | 38 / 73 |
| GRINCH-style cut.7 | .005525 / .012910 | 466 / 1,089 |

Crossspan precision .003257/.002872 seed,.001918/.003741 BIRCH,
.002082/.005662 GRINCH is a changed-prevalence conditional diagnostic:
within-span true positives disappear while between-topic negatives remain.
It is not comparable to primary far precision, proof of deployment precision
collapse or independent coding/episode-return gold. Sparse native recurring
topics are not a representative global semantic taxonomy gold set.

Capture SHA256:
`de23a37b0ef5cdc14befd6412380fa2b4a7a2b5b28eda826cadac93b0eceee0a`.
Script `topic-qmsum-crossspan-20261002.py` SHA256:
`4d36303a60dbd176e88402d47cf64dd37d1598027dd7428ebd5fa7064fe92f1f`.
Separate complete original-source upper-triangle mask audit passes every meeting/
order/method contingency and summary versus counter reductions, independently
verifies no native merge needed and all771 components/33 recurring topics.
Auditor SHA256:
`48304ed67b5da0c041f94d4bf2f03713c6f3082ee949b31d38f9869fdd4dbc0a`;
terminal `topic-qmsum-crossspan-audit-first-20261002.json`. No semantic/guided-search
or Rust gate follows. Evaluate broader hierarchy/macro grouping and granularity
before treating a fixed flat cut as semantic topics.

## Implementation, pins and reproduction

`discovery_controls.py` defines actual model updates, final state hashes and
canonical partitions; `discovery_inputs.py` loads only complete pinned evidence;
`qmsum_discovery_controls.py` records full source coverage, labels/pairs/support/
order/cost. Five new tests verify immutable input, complete partition ownership,
duplicate/separated directions, invalid vectors, unsupported rare topics and
original gap-based pair exclusion. The full enabled research suite passes467.

Pinned BIRCH module SHA256:
`92070c94072b679e22dd298c07e5d0c1387b7fa7237bfe38d27a35a130a28de3`.
Supplied GRINCH author manifest commit
`a2747d3c5d3405cf27afc839bc28352b99c3e309`; all source/license/readme hashes
verified, as in [earlier online controls](2026-10-01-grinch-online-controls.md).
Every capture pins helper/driver/source/lock bytes and scientific library versions.
Fresh outside-Git directories under `/home/james/scratch/mash-artifacts`;
driver arguments `--source --vectors --units --labels --baseline
--reference-manifest --method birch|grinch_style --output-directory`, repeat
`--compare FIRST/results.json`. Use external `memory_audit.py --output MEMORY
-- COMMAND`. Final CF accounting/center hashes and complete live tree topology,
aggregate/link hashes/merger cosines support numerical audits, not semantic review.

Bounded study bead `mash-c0d.19.2.20.8.3.2` is complete with rejected operating
points. P0 follow-up `mash-c0d.19.2.20.8.3.3` evaluates cut-free broader hierarchy
against root-only/temporal baselines before calibrated frontier or bounded macro
discovery choices; arbitrary-cut failure is not algorithm-wide failure.
Production selection still requires
independent coding-topic/label/multi-tag/ancestor quality, bounded lifecycle and
full-ingestion costs, shared namespace behavior and selected-method Rust parity.
