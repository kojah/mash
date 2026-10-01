# Separate-sample granularity calibration: recall improves, merge quality remains weak

Exploratory development experiment, bead `mash-c0d.19.2.31`. No production method
selected. This follows the singleton-fragmentation diagnostic; it does not tune
on its original 32 groups or on the new reserved outcomes.

## Preregistered two-phase procedure

Pinned MultiWOZ development shard:512 complete text groups. Exclude the original
32 exposed groups; select32 new groups by SHA256(seed+group), seed
`mash-topic-multiwoz-calibration-v1-20261001`. First16 calibration, next16 reserved
outcome evaluation. No service/intent stratification, resampling or replacement
when support is inconvenient. This is **development-internal reserved outcome
evaluation**, not an external untouched test: full source structure/aggregate
annotations were inspected earlier, and some new text previously contributed to
the old lexical background. Neither new partition had its clustering outcomes
evaluated before this experiment. Template/paraphrase/pretraining overlap remains
unknown. Task-oriented service/find/book proxies are not coding-topic gold.

Refit the fixed lexical policy using448 remaining groups, excluding all64
exposed/calibration/reserved groups and exact selected USER strings. Background:
3380 USER occurrences minus21 exact selected strings and32 duplicates =3327
unique strings;3648 features,60170 nonzeros. Unlabelled text only, same fixed
tokenization/unigram+bigrams/min_df2/sublinearTF/smoothIDF/L2/no stoplist policy.
Basis is fit identically in both phases and checked against the calibration
artifact. Reserved exact current/joined model-input strings occurring among
calibration inputs are excluded before support/metrics; record reasons, never
replace samples. No silent zero-vector or root fallback.

Installed, hash-pinned CPU MiniLM model/tokenizer; singleton2–512token inputs,
no truncation/padding/download, quantized normalized384D. Only current USER and
exact preceding SYSTEM+newline+USER vectors, plus fixed lexical and75/25 dense/
lexical views. Whole-phase preflight1024uniqueinput cap. Fit all commonly supported
anchors without label-driven admission; evaluate known nonempty declared proxy
labels afterward on cross-original-group pairs. No generative calls.

Calibration-forward only: five views x two methods x six thresholds =60
candidates. BIRCH radii .3,.4,.5,.6,.7,.8; GRINCH-style tree cuts
.1,.3,.5,.6,.7,.9. Branch64/no global cluster count; independent exact-neighbor
GRINCH-style prototype, not author-code parity. Maximize service pair F1 for
broad objective and **full intent F1** for fine objective; ties by lexicographic
candidate identity. Within-service F1 is only a diagnostic: its high prevalence
would reward all-one collapse. Save all candidates/partitions/counts and freeze
winners before the separate evaluation command. Evaluation refuses policy,
source/split, code, model/version, basis or recalculated-winner drift.

Evaluate only frozen winners (deduplicated if shared) plus fixed current BIRCH
.4/current tree .7 references and all-one/singleton baselines. Forward, reverse,
seed17 shuffled arrival; sorted source identity is not event-time ingestion.
No re-selection by reserved scores/order, threshold expansion or new alternatives
after reserved outcomes. Capture complete identity sets, arrivals, records/vectors,
lexical coordinates, pair truths/confusion counts, ARI and timings. The existing
fixed-threshold diagnostic retains its defaults; research fit now accepts an
explicit validated threshold. General pair scoring handles supplied named vector
views rather than requiring an unused mean view. Historical exact-repeat captures
still require their recorded original commits/code pins.

## Calibration and frozen selection

115 USER anchors,16 root-context abstentions,99 common supported anchors, no
lexical-zero rows;2590 eligible cross-group pairs:service715 positive/1875
negative;intent465/2125;within-service465/250. All60 candidates fitted. Both
objectives select **joined BIRCH radius .8**,20 groups/13 singleton anchors:
serviceF1 .529638, precision .589041, recall .481119; intentF1 .402288,
precision .361301, recall .453763. All-one service/intent F1 .432678/.304419;
within-service all-one F1 .788136. Winner is at the radius-grid boundary; this
limits calibration conclusions and is not permission to expand after evaluation.

Frozen first calibration SHA256 recorded in Beads before reserved execution:
`18fdb773b198d3f592738b5d39aaa90c5339658e0b11e57aa0f7b619cd05da04`.
Separate calibration repeat matches all non-timing outputs exactly.

## Reserved outcomes

112 USER anchors;16 root-context and one exact-calibration-input abstention;
95 matched fit anchors, no lexical-zero rows.2621 eligible pairs:service914/1707,
intent590/2031,within-service590/324. Source support is unchanged across evaluated
methods/orders. All-one service/intent F1 .517115/.367487;singleton both0,
precision undefined. Within-service all-one F1 .784574.

| Frozen setting | Arrival | Groups | Singletons | Service F1 | Intent F1 | Forward ARI |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Joined BIRCH .8 | Forward | 20 | 16 | .530707 | .426534 | 1 |
| Joined BIRCH .8 | Reverse | 30 | 22 | .475000 | .431900 | .158404 |
| Joined BIRCH .8 | Shuffled | 25 | 20 | .699552 | .546575 | .311206 |
| Current BIRCH .4 | Forward | 83 | 72 | .017316 | .026667 | 1 |
| Current BIRCH .4 | Reverse | 83 | 72 | .015168 | .023372 | .845705 |
| Current BIRCH .4 | Shuffled | 82 | 70 | .017316 | .026667 | .814254 |
| Current tree .7 | Forward | 80 | 71 | .042735 | .065359 | 1 |
| Current tree .7 | Reverse | 81 | 71 | .030108 | .046205 | .862974 |
| Current tree .7 | Shuffled | 81 | 71 | .030108 | .046205 | .862974 |

Primary forward winner:service precision .564039/recall .501094;intent precision
.368227/recall .506780. TP/FP/TN/FN service458/354/1353/456, intent299/513/1518/291.
Within-service intent precision .652838/recall .506780/F1 .570611; lower than
all-one .784574, exposing its high-prevalence weakness as a standalone objective.
Winner intent precision/recall reverse .458175/.408475, shuffled .458621/.676271.
Do not choose shuffled just because it scores better. BIRCH singleton precision
is higher but misses nearly all positive pairs. Performance and quality tradeoffs
must be presented together.

Selection changes both representation and radius relative to the fixed-current
references, so these comparisons do not isolate a threshold-only causal effect.
Calibration substantially addresses fragmentation, but cannot be called robust:
forward intent precision remains37%, reverse/shuffled partitions have low ARI,
and reverse broad F1 falls below all-one. Small16-group dependent-pair evaluation
has no confidence estimate or significance/noninferiority claim. Proxy equality
is not a semantic-topic partition, and flat co-clustering ignores overlapping
membership and ancestor quality. No labels/hierarchy/guided-search gain tested.
Next investigate stable multi-resolution/overlapping discovery with explicit
false-merge and coverage goals; require new reserved evaluation and independent
coding-session/provider judgments before selection. Do not simply loosen .8
after observing these outcomes.

## Repetition, cost and reproducibility

All191 isolated research tests pass: disjoint label-independent splits, exact
source plans, tie selection, collapse baselines, threshold changes and invalid
threshold rejection, plus prior boundary/model tests. Independent verification
confirms split/background/exact-input separation, all69 partitions/confusion
counts and calibration-selected winners. Calibration and evaluation each repeat
exactly on all non-timing fields, including full vectors/inference statistics,
basis, annotations, arrivals, all metrics and tree statistics.

Calibration214 inference calls:479.77/582.88ms; total60 fits3.306/3.090s,
complete after-imports experiment5.347/5.381s. Reserved207 calls379.24/372.37ms;
all9 fits315.94/316.59ms; complete1.480/1.498s. Winner forward fit37.51/39.49ms,
insertion-p95 .469/.497ms. Fit timings exclude final prediction, cuts/validation,
metrics/persistence; one-thread BLAS/CPU model. Total excludes imports/serialization.
Background preparation is included in phase totals. Research refits and extra
joined embeddings are **not free production work**. No full ingestion/p95 update/
RSS/persistence/edit/delete/restart gate, no Rust inference/parity proof.

Private artifact root:
`/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`.

- `calibration-first.json`: `18fdb773b198d3f592738b5d39aaa90c5339658e0b11e57aa0f7b619cd05da04`
- `calibration-second.json`: `d25ba5b8a13feab630b148553a8669c8f6e2fda9b7ac7c07c1813396ccee3c80`
- `calibrated-evaluation-first.json`: `8819efd81ba90111f56a9cd6b2c79c4202ff71ca4b98fba3b0877c04c89e1954`
- `calibrated-evaluation-second.json`: `79bfebf0d6f83621136494933a2b9d3a1ec250338be0cd62312aad546f82bfc5`

`cluster_calibration.py --root ROOT --snapshot INSTALLED --output FRESH` runs
calibration. Separate evaluation requires `--calibration FROZEN_CAPTURE`;
repeat checking uses `--compare CAPTURE`. Capture paths must be fresh/outside Git.
Model/tokenizer hashes, dependency/code/source pins and complete split manifests
are captured. No raw histories/corpus/captures committed.
