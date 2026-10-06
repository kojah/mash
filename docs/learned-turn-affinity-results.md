# A candidate meeting the simultaneous point-estimate targets

The calibration-selected **Voyage affinity logistic model** clears precision >82%, recall >90%, and distant-turn recall >80% on the existing 52-thread development evaluation subdivision. It is a supervised research candidate; production and the frozen baseline are unchanged.

| Configuration | Precision | Recall | F1 | Distant recall |
|---|---:|---:|---:|---:|
| Frozen Voyage 4 joint baseline | 83.66% | 87.56% | 85.57% | 70.37% |
| Previous high-recall BIRCH ensemble | 76.60% | 93.26% | 84.11% | 88.89% |
| Learned affinity, grouped out-of-fold calibration | 84.77% | 91.20% | 87.87% | 80.95% |
| **Learned affinity, development evaluation** | **82.63%** | **91.19%** | **86.70%** | **83.33%** |

Evaluation counts are TP=176, FP=37, FN=17, TN=66, with 50 ambiguous judgments excluded. Distant recall is 45/54; the frozen baseline recovers 38/54. Thus the margins are small and the distant denominator is limited. The exploratory 95% paired thread-bootstrap F1-gain interval against the reference is [-1.50,+4.10] percentage points. Passing the point-estimate targets is not a statistically confirmed superiority claim or a guarantee on new users' data. The labels are agent-authored and this evaluation subdivision was previously consumed; confirmation remains sealed.

## What changed

Calibration error inspection found false matches with high dense cosine but different concrete subjects: release versions, UI components and team-building targets. We registered a hybrid affinity experiment, inspired by the graph formulation in [Kummerfeld et al., ACL 2019](https://aclanthology.org/P19-1374/). Their reply-structure task differs from our within-thread subject affinity; this is methodological inspiration, not evidence of transfer performance.

The experiment considered 64 Optuna trials over logistic regression and small histogram gradient-boosted trees, with dense-only or dense-plus-thread-local lexical features. Five folds are grouped by calibration thread, and each fold fits scaling only on its training rows. Only decisive calibration judgments are used. The scope contains 104 threads; five have only ambiguous reference judgments, so fitting involves 99 threads and 597 decisive pairs (409 related, 188 unrelated). No evaluation thread appears in any fold's training or validation set. One setting is frozen before evaluation.

The selected model uses **dense-only** features; the lexical/identifier hypothesis did not yield the selected winner. For two turns, compute raw Code 4 cosine, raw Voyage 4 cosine, and their cosines after the existing .10/window-4 branch-aware smoothing. The chosen regularization is C=.1. Fold-fitted scaling is absorbed into four public coefficients and a bias, producing:

```text
score = sigmoid(
    -0.6671140419546744 * code_cos
    +2.3369736267330268 * general_cos
    +5.2313078016870636 * code_smooth_cos
    +7.269326718344237  * general_smooth_cos
    -9.077585907893686
)
```

Connect pairs with score >=.525. This score is an affinity, not a calibrated probability guarantee. Correlated predictors mean coefficient magnitudes should not be read as causal importance. Negative raw-Code weight also means this is not simply a positive weighted embedding concatenation. It is a symmetric learned relation used by a graph algorithm.

## Groups and practical limits

Build deterministic greedy cliques on the accepted-edge graph, then cover remaining edges with two-turn groups. Every pair in an emitted group must have an accepted edge. A turn may belong to multiple groups, and unsupported transitive links are never introduced. Full graph reconstruction verifies that co-membership decisions equal thresholded affinities for **every pair**, not just sampled labeled pairs.

The 52 evaluation histories produce 3,704 groups: 3,068 (82.83%) are two-turn groups; 636 contain at least three turns, and the largest has 41 turns. This fragmentation matters. The method establishes a promising related-turn affinity, but pair-level gates do not establish convenient thematic groups, stable topic labels, or representative thread vectors. This is a batch per-changed-thread prototype, not yet a production or incremental implementation.

Runtime needs both fixed Voyage vector spaces, which increases embedding work relative to the single-model baseline. No raw text from another thread is consulted at runtime, and no LLM/generative inference is used. The experiment reuses existing vectors and costs $0 additional API inference. Recorded driver time is 26.7 seconds for feature construction, 64 grouped-CV trials, model fitting and evaluation graph projection, excluding final Optuna persistence and previous embedding work. It is not an ingestion benchmark.

## Reproducibility

The [registered protocol](learned-turn-affinity-protocol.md) and [candidate manifest](learned-turn-affinity-candidate.toml) define the selected algorithm. Preserve this as a candidate, not a silent replacement for `voyage4-joint-v1`.

Verification checks source/input/matrix/annotation hashes; rebuilds all 156 development feature matrices; verifies fold separation and ambiguous-only exclusions; refits the selected model from calibration data; compares the portable JSON formula with sklearn predictions; reconstructs all evaluation groups and confusion counts; and checks all 64 completed Optuna trials and the finished MLflow run. Raw data, per-thread predictions and fitted objects stay private.

The original CLI-created pickle needs its original module names to reload; its hash and executed source are preserved rather than rewriting history. The candidate's typed `linear-model.json` removes that dependency, matches the fitted scorer within 1e-12, and contains only feature names, coefficients and bias. Future runs save native sklearn objects separately from typed settings, with a portable linear export when logistic regression is selected. Ruff, ty and all 171 Python tests pass. The paper records the procedure, result and limitations.
