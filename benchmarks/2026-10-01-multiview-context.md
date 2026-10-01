# Frozen multi-view and token-count context controls

Exploratory development experiment, 2026-10-01, Bead `mash-c0d.19.2.31`.
This follows [the within-service intent failure](2026-10-01-external-intent-specificity.md).
No production topic method is selected.

## Frozen procedure

Before new outcomes, freeze three controls alongside the unchanged current,
mean and joined baselines. No weights/length thresholds are fitted to annotations:

- Equal two-view: normalize `[sqrt(0.5) * current, sqrt(0.5) * joined]`.
- Current-dominant two-view: normalize `[sqrt(0.75) * current, sqrt(0.25) * joined]`.
- Short gate: joined vector only when the current original tokenizer count is
  at most 10 **including special tokens**, otherwise current vector. Gate each
  endpoint independently; a mixed pair compares current against joined in the
  original 384-dimensional space.

The two-view vectors have 768 dimensions and cosine approximately equals the
frozen weighted sum of current-current and joined-joined cosines (roundoff only).
The views remain separate; this is not an average of their vectors in 384 dimensions.
Both controls double vector storage/dot-product dimensions. They do not remove
the cost of computing a contextual embedding. Cached inputs mean zero *new*
inference in this experiment, not zero contextual inference in production.

Prior multi-view topic work jointly uses complementary embedding spaces
([Gupta et al., 2021](https://aclanthology.org/2021.naacl-main.332/)). It motivates
testing separate views; the simple fixed cosine controls here do not replicate
that learned neural topic model or its reported performance.

All representations are restricted to the original 206 common model-support
anchors, with no root fallback. Same frozen 32 complete-text groups, exact source
and input hashes, unknown-intent/empty-state exclusions and 12,341 cross-dialogue
pairs. Primary service/intent/within-service AP and AUC use the same 1,000 archived
whole-dialogue draws (seed 20261001), same multiplicity-product weights, and same
no-same-original-source rule as the previous experiment. Point per-domain scores
are secondary; no new per-domain bootstrap or multiple-comparison claim.
This design follows inspected development outcomes, not independent confirmation.
Empirical percentile ranges are not coverage-calibrated confidence intervals.

`multiview.py` constructs features from original pinned vectors/token counts only.
It receives no annotation names or intent/domain labels in its feature constructor.
Labels are introduced only for evaluation after deriving all representations.
Revalidate the entire original source/anchor/pair grid and require all baseline
metrics and every archived baseline paired delta to remain exactly unchanged.
No approval-phrase recognizers, generative calls, downloads, model initialization,
new model calls, training/test data or output-driven resampling.

## Artifact inputs and reproduction

Private root: `/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`.
Frozen vector capture SHA256
`62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c`;
frozen bootstrap analysis SHA256
`b31098e45cec9d521690e67fc395c90819ca7aad01dbc2b98672cb20dd587ec6`.
The source/model/version pins are inherited from those immutable inputs, not
rewritten to pretend an old run used new source code.

```bash
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  research/topics/multiview.py \
  --root /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001 \
  --capture /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/context-controls-final-first.json \
  --analysis /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/intent-bootstrap-first.json \
  --output /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/multiview-new.json
```

Captures refuse overwrites and retain candidate vectors, exact gate choices,
token counts, complete pair truth/scores, all bootstrap draws/deltas, policies
and source/code pins. A fresh repeat must match every non-timing field exactly.
Pass `--compare` with the first capture when repeating the command above.
Original intent-analysis repeat code is available at commit `b85cd38`; the shared
summarizer now takes explicit controls while preserving original default behavior.

## Results and decision

All old baselines reproduce their metrics and every archived paired replicate
delta exactly. The token-count gate selects joined context for 35 of 206 anchors,
and current text for 171. No support changes, root fallback or new model calls.

| Control | Service AP | Intent AP | Within-service intent AP | Within-service AUC |
| --- | ---: | ---: | ---: | ---: |
| Current | 0.464235 | 0.399102 | 0.802038 | 0.583550 |
| Mean | 0.534925 | 0.415504 | 0.752176 | 0.512996 |
| Joined | 0.555472 | 0.417893 | 0.737828 | 0.490344 |
| Equal two-view | 0.548260 | 0.453731 | 0.782023 | 0.544571 |
| 75% current two-view | 0.506202 | 0.429569 | 0.794722 | 0.567061 |
| Short gate | 0.500671 | 0.416599 | 0.787152 | 0.563922 |

| Candidate | Service paired AP range | Intent paired AP range | Within-service paired AP range |
| --- | --- | --- | --- |
| Equal two-view | [0.052826, 0.113489] | [0.023029, 0.086973] | [-0.033345, -0.002413] |
| 75% current two-view | [0.027509, 0.055349] | [0.015439, 0.045708] | [-0.013612, 0.001444] |
| Short gate | [0.014504, 0.063744] | [-0.003382, 0.042159] | [-0.037355, 0.006607] |

These are empirical 2.5/97.5 percentile ranges of paired changes versus current,
not calibrated confidence coverage or formal significance. All 1,000 replicates
support both classes in every primary slice. Equal two-view within-service AUC
range is [-0.071408, 0.002277]; 75%-current is [-0.033069, 0.003730]; short gate
is [-0.064634, 0.024293]. Within-service positive prevalence remains 72.16%, so
high AP is not evidence of strong fine-intent discrimination.

Point per-domain AP (supports as in the previous experiment):

| Domain | Current | Joined | Equal two-view | 75% current two-view | Short gate |
| --- | ---: | ---: | ---: | ---: | ---: |
| Attraction | 0.223221 | 0.226880 | 0.236796 | 0.231332 | 0.238883 |
| Hotel | 0.281696 | 0.381683 | 0.358152 | 0.317853 | 0.303645 |
| Restaurant | 0.155075 | 0.232215 | 0.191747 | 0.168462 | 0.160521 |
| Train | 0.489262 | 0.566958 | 0.598126 | 0.548905 | 0.554680 |

The candidate controls preserve more fine-intent separation than full joined
context, but all remain below current text on the within-service point score.
None demonstrates broad improvement with strict within-service nondegradation.
Do not invent a tolerance after seeing these outcomes or treat a range crossing
zero as proof of noninferiority. Current-dominant two-view is a candidate for wider
testing, not a selected default. The length-only gate is insufficient evidence of
general reference resolution and does not identify which context passage is relevant.

## Verification, cost and next experiment

All 176 research tests pass, including exact frozen cosine weights, opposing views,
normalization/dimensions, token boundary, malformed features, original support and
expanded-summary baseline invariance. The runner also requires active NumPy/
scikit-learn versions to match the archived analysis rather than merely reporting
its old version strings. No Rust or production search behavior changes.

Two fresh captures agree exactly on every non-timing field, including all candidate
vectors, gate choices, complete pair grids and all paired resampling deltas:

- `multiview-first.json`, SHA256
  `b5e373e3f18d8b4d94921755119b9e050dbd8b6e56f5725163e0568d9705c34f`.
- `multiview-second.json`, SHA256
  `bf2ed9bb3322b113c098e3f635f35b40aacdfe1af8d42be33ace7097972cadc5`.

NumPy 2.4.6/scikit-learn 1.9.1. Derived feature construction takes 18.54/18.60 ms
for 206 anchors and three candidates; 37,023 candidate pair dots take
49.63/48.71 ms. Total analysis after imports is 83.71/83.18 seconds, including
source/capture validation, all primary summaries and 18,000 resampled method
evaluations plus baseline/domain point evaluations. Serialization is excluded.
These are descriptive cached-vector measurements, not per-query search latency.

Feature construction and candidate pair scoring are measured separately from the
offline resampling analysis. These cached-vector costs exclude original contextual
inference, clustering, label maintenance and persistence; no production ingestion,
RSS, p95 or Python/Rust parity gate follows from them. The 768-dimensional views
require double vector coordinates per unit, before any index/storage overhead.

Next test representations that retain finer lexical/aspect evidence, or keep
separate broad/fine views rather than collapsing discovery into one mixed metric.
Freeze new controls before outcomes and require independent coding-session aspect,
label, semantic hierarchy and guided-search checks. No annotation-dependent model
input selection, approval phrase lists or generative topic calls.
