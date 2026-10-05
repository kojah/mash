# Two-view ensemble: completed

The [frozen protocol](turn-ensemble-followup.md) compared pooled whole-context
BIRCH (A), heading-aware structural BIRCH (B), and their union/intersection.
Both use radius .60, cutoff .75 and centroid memberships. We scored the same
160 development histories from immutable parent predictions; no new inference,
parameter changes or confirmation judgments were used.

**Decision: no eligible configuration.** Union helps, but long-history recall
remains below .70. Intersection improves precision at a substantial recall cost.

| Method | P | R | F1 | Long P | Long R | Long F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A: pooled context | .887 | .724 | .798 | .855 | .564 | .679 |
| B: structural chunks | .917 | .611 | .734 | .901 | .490 | .635 |
| Union | .881 | .801 | .839 | .850 | .666 | .746 |
| Intersection | .932 | .535 | .680 | .924 | .389 | .547 |

Union has TP=494, FP=67, FN=123 overall and TP=209, FP=37, FN=105 on
long histories. Long precision is 209/246=.84959, also narrowly below .85;
the displayed rounded value does not constitute a pass. Intersection has
TP=330, FP=24, FN=287 overall and TP=122, FP=10, FN=192 long.
Union's paired thread-bootstrap F1 interval is .808–.869, with gain over the
chronological-four baseline .126–.208. These intervals do not override the
registered point-estimate gates. Nearby unrelated FPR is .500/.200 and distant
related recall .378/.156 for union/intersection respectively.

The parents reproduce their recorded counts exactly. Unit tests verify view
namespacing, no transitive merging, supported intersections and equivalence
between membership links and pair operators. Combined full-corpus memberships
were not recomputed because neither combination is eligible for confirmation.
Their group counts and coverage are therefore unavailable, not zero.

Scoring completed in 4.86s with externally measured peak RSS 1,056,764 KiB.
Historical summed parent preparation/fit p95 is approximately .130s/history
for either combination. This is a conservative diagnostic cost estimate,
not measured end-to-end ensemble construction. Parent embedding-build costs
remain those published in the preceding studies.

Private report:
`/home/james/scratch/mash-artifacts/turn-ensemble-followup-20261005/development.json`.

- Protocol SHA256: `991ad49eda8e0bdc7948e113669289263f7236dc851ad5cee0e4e6e5d7098ea6`.
- Development SHA256: `1dc8a1985e3fa86c95cb6e36ffc85bd9318ed28df64cc7b99004d4e7cf5ff8b2`.

Confirmation remains sealed. This closes the registered heuristic screen.
The next step is to assess semantic representation quality and failure causes;
there will be no further tuning of these parents or combination rules.
