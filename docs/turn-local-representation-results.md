# Thread-local representation results

All six preregistered Optuna trials completed on 160 development histories (3396
prompted turns), with the unchanged 1051-pair benchmark and a separate 62-pair
targeted blind diagnostic. Confirmation remains sealed. This completes the three
requested exploratory levers; it does not establish a production clustering
algorithm that meets the acceptance gates.

## Controls and validation

Before scoring, a label-free probe found quantized batch sensitivity (cosine
.9908990864 for identical text alone versus beside another text). The initial
batched build was stopped unscored. The amended six-cell experiment uses singleton
inference for all new candidate/assistant vectors, retaining immutable own-history
pooled/user parent vectors. The new none/off cell is the matching inference
control; historical-baseline differences are reported separately.

The audit checked all 960 representation artifacts, hashes, unchanged attention
text, bit-identical unmasked vectors and user-vector equality across context
modes. Fresh-cache real-model probes for short history 91 and long history 109
passed 24 comparisons: alone, after another history, and after unrelated changed
text. Selected inputs, vectors and cluster memberships matched. This does not
test independent regeneration of the immutable historical parent embeddings.

## Quality

Boilerplate on/off produces identical benchmark counts within each context mode.

| Context | Overall P | Overall R | Overall F1 | Long P | Long R | Long F1 |
|---|---:|---:|---:|---:|---:|---:|
| None, singleton control | .8431 | .8622 | .8526 | .8153 | .7452 | .7787 |
| First 80 prompt tokens | .8479 | .8671 | .8574 | .8293 | .7580 | .7920 |
| Semantically selected prompt span | .8433 | .8720 | .8574 | .8197 | .7675 | .7928 |

First80 versus singleton control gains .004808 F1, paired thread-bootstrap 95%
interval [-.002319, .012051]. Selection gains .004806, interval [-.003093,
.013657]. Selection versus first80 is -.000001277 F1, interval [-.006593,
.007802]: essentially a recall/precision tradeoff, not a demonstrated win.
Selection raises long recall by .009554 over first80 while reducing long
precision by .009540. All six cells fail the existing precision gates.
The historical frozen baseline remains .854618 F1; singleton none/off adds
three false positives with unchanged TP/FN. Do not attribute that inference-
regime effect to either representation treatment.

On the purposive diagnostic, TP/FP/FN/TN are 34/15/8/5 for none,
30/13/12/7 for first80, and 33/12/9/8 for selection. Reviewer-substitution
sensitivity changes one TP to FP in each cell. These counts cannot estimate
population precision/recall and are not combined with benchmark scores.
The 2000-resample intervals reuse development data and are unadjusted;
they are exploratory, not confirmatory significance tests.

## Boilerplate interpretation

The conservative rules match only nine greeting spans: one `hello!`, three
`hi!`, three `hello.`, and two `hey!`. They downweight 18 tokens and change nine
vectors in each context mode, leaving 19,941--19,983 other points bit-identical.
No original benchmark prediction changes. A full comparison of all 133,724
possible development turn pairs finds zero changed links for none/selection
and four added links for first80. None of those four occurs in either reference
manifest. Thus zero scored gain is real, but not zero population effect, and
the tiny greeting-only coverage cannot establish whether broader agent-specific
scaffolding suppression is helpful. Do not expand rules post hoc to rescue this
preregistered comparison.

## Cost and decision

There are 45,643 unique new vectors. The six-cell construction required 8:56.39
before a controlled interruption plus 10:06.79 after resumption. Host filesystem
journal contention prompted grouping eight cache writes per transaction while
keeping every inference single-input; unused legacy-cache imports were removed.
Completed artifacts/cache entries were retained. Timings mix cold and resumed
work under I/O contention and are not an isolated cold-performance benchmark.

Observed construction p95 is at most 18.23 seconds/history, max 210.01 seconds;
fit p95 is at most .148 seconds. Scoring all six trials takes 67.45 seconds.
Peak measured RSS across build/audit/scoring is 1,083,668 KiB (about 1.03 GiB).
Observed runtime/memory budgets pass. Selection accepts context for 10,226
original answer units, resulting in 10,253 conditioned output chunks, with
19,977 total points versus 19,950 for none and 19,992 for first80.

Retain the frozen experimental baseline. Neither new treatment earns promotion;
selection has more work and nearly identical F1 to the simpler first80 control,
and the present boilerplate detector has inadequate coverage for a broad claim.
No production configuration, index or model is changed, and no further trials
or confirmation run are started by this study.

Private artifacts are under
`/home/james/scratch/mash-artifacts/turn-local-representation-20261006`.
Protocol SHA256: `9e71c1b35d976dafa1e14e747232293e695dbdd9d747ec3d433b4adea1375383`.
Typed consolidated result SHA256:
`94d2fb1ac0a531a77164c943e3a1e01105bb8bfcf74bdd840e04b786e13eb63f`.
