# Two-view turn-cluster ensemble: prospective comparison

Frozen after the completed structural study, before computing combined
predictions. The user previously suggested that different algorithms/views may
have complementary strengths. This is a bounded turn-clustering follow-up,
separate from the retired topic-model ensemble bead.

Keep the cohort, labels, encoder and existing cached vectors unchanged. Score
the same 160 development histories only; confirmation remains sealed. No new
inference, parameter tuning, supervised training, weighting or segmentation.

## Fixed parents and four cells

Parent A is pooled whole-context BIRCH radius .60/cutoff .75 with centroid
memberships. It was the only pooled-context cell satisfying the overall quality
floors/gain; long precision is .855, recall .564. Pin its report SHA256:
`d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e`.

Parent B is heading-aware structural BIRCH radius .60/cutoff .75 with centroid
memberships unioned per turn. It dominates plain structure at that cutoff and
has long precision .901, recall .490. Pin its report SHA256:
`50d7ef826e4d21196dc4e3cef5c33d59a5f2884561c26cb2e2ec723d3ca28555`.

Choose these parents from the already published aggregate results, without
inspecting which reference pairs their errors overlap on. Neither is accepted
alone. Run exactly four cells:

1. A alone, unchanged control.
2. B alone, unchanged control.
3. Union: preserve A and B groups with distinct view namespaces. A pair matches
   when it shares any group in either parent. Do not merge overlapping groups
   transitively. Duplicate-looking groups can remain a product limitation.
4. Intersection: use membership pairs (A-group, B-group), retaining only group
   intersections supported by two distinct turns. A pair matches only when
   both parents connect it. This is a precision diagnostic, not a silent vote
   threshold sweep.

Parent source/point support rules remain unchanged. Namespacing/intersection
must be tested independently of arbitrary cluster-number assignments. First
score the four cells from frozen pair predictions, reproducing each parent.
If a final choice is eligible, recompute its full memberships from cached
vectors and require exact reference-prediction reproduction before confirmation.

## Gates and stop rule

Preserve overall P>=.85/R>=.70/F1>=.75 and F1 gain>=.10 over chronological-four,
plus the same absolute floors on 8+ turn development histories. Choose highest
eligible F1, then conservative summed parent preparation+fit cost and name.
Use summed parent costs for both combined cells; do not claim parent fits are
free because this diagnostic reads recorded predictions. Require p95<=10s and
the measured scoring/recomputation process peak RSS<=4GiB. Base construction
costs remain those already measured for the fixed parents.

Report all four cells, provider/node/length/sampling-stratum/batch breakdowns,
paired thread-bootstrap uncertainty and fixed nearby (<=4) unrelated FPR/distant
(>4) related recall. No new combination, voting rule or parent may be selected
after these scores. This closes the current heuristic screen: a failed result
will motivate a representation-quality assessment, not another unrestricted
cutoff/window sweep. At most one eligible final method may advance to the
unopened confirmation set once. No passing method means no promotion; do not
relax gates after the result.
