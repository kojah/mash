# Temporal embedding smoothing: prospective five-cell comparison

Frozen after the completed input/context follow-up and before temporal scores.
This tests the user's clarification: nearby vectors move slightly toward each
other **before clustering**, rather than changing membership-score thresholds.

## Fixed starting point

Use the frozen 200-history cohort, annotations and existing vectors from the
input/context follow-up. Score only the same 160 development histories.
Confirmation stays sealed. No model inference, downloads, label changes or LLMs.

The anchor is whole-input ancestral context with BIRCH radius .60, cutoff .75
and centroid memberships. It was the only follow-up cell satisfying the overall
quality floors and chronological-four gain; it still failed long-history
recall and is not an accepted algorithm. Pin the development report SHA256:
`d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e`.

## Transformation

Let each original unit-normalized turn vector be x_i. Neighbors are prompted
turns up to W prompted-turn positions away in the same history. Include both
earlier and later neighbors, but only when one is a structurally resolved
ancestor of the other. Exclude siblings, unresolved branches, explicit roots
without a resolved connecting chain and other histories. Apply the existing
linear-history fallback only when all parent fields are absent and the boundary
is an ordinary user reply. Orphan turns may establish ancestry but are not
clustering points or neighbors.

For eligible j, set weight_j = W + 1 - abs(i-j). Compute the weighted mean of
their **original**, unchanged vectors, normalize it to m_i, and set:

`x'_i = normalize((1-alpha)*x_i + alpha*m_i)`.

If no neighbor exists or its mean norm is <=1e-12, retain x_i. Never include
self, iterate the transformation, use a smoothed neighbor as input, add a
similarity-dependent rule, or modify BIRCH/membership thresholds. The unchanged
control must remain bit-identical. Run exactly five cells:

| alpha | W |
| ---: | ---: |
| 0 | 0 |
| .05 | 2 |
| .05 | 4 |
| .10 | 2 |
| .10 | 4 |

This is symmetric smoothing of the current visible thread, not a causal stream
simulation. A newly arriving turn can change nearby older vectors. In an
implementation, first update base vectors affected by source/context changes,
then invalidate their bounded smoothing neighborhoods and recompute the thread's
partition. The user has allowed offline per-thread recomputation when it changes.

## Quality and stopping rule

Preserve overall precision >=.85, recall >=.70, F1 >=.75 and gain >=.10 over
chronological-four. Also require the same absolute precision/recall/F1 floors
on 8+ turn development histories. Select highest eligible F1, with ties broken
by transformation-plus-clustering seconds and name. The control cannot advance
if it fails those gates. No repeated weight/window tuning after seeing scores.

Report every original provider/node/length/sampling-stratum/batch breakdown,
paired thread-bootstrap uncertainty, vector displacement, changed memberships
and costs. Additionally use fixed pair-distance slices for all five cells:
nearby (distance <=4) and distant (distance >4). Report nearby unrelated-pair
false-positive rates and distant related-pair recall, including changes from
control. Do not assume distant links are preserved by smoothing. These are
diagnostics, not extra thresholds invented after results.

Reuse the measured base construction/memory results and measure the complete
new scoring process peak RSS. Require transformation-plus-clustering p95 <=10s
and peak RSS <=4 GiB. At most one eligible final choice may advance to the
unopened confirmation set, once. No eligible choice means this experiment fails;
do not promote it or silently relax gates.
