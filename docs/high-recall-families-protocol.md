# Clustering-family follow-up

The BIRCH ensemble passed the simultaneous targets in calibration but missed precision on the development evaluation subset: 76.60% precision, 93.26% recall, 88.89% distant recall. This motivates a change of partition rule rather than another BIRCH-only threshold sweep.

Register 768 seeded Optuna TPE trials over average-link, complete-link and online leader partitioning; partition cosine cutoffs .40/.50/.60/.70/.80; the five convex Code 4/Voyage 4 weights; and the membership cutoffs, margins and pooled/chunk/union views from the prior protocol. Partition cutoff is recorded in the existing `radius` parameter field for report compatibility; it is a cosine threshold, not a BIRCH radius, in these arms.

Keep centroid-based overlapping membership, support of two distinct turns, frozen turn inputs and smoothing. Use the same calibration-only simultaneous-target objective and selection rule. Freeze one family/weight/setting before evaluating it. The split remains 104 calibration and 52 reused-development evaluation threads; confirmation stays sealed. No API calls or production changes. This adaptive follow-up cannot establish independent generalization.

If this search misses, report which constraint fails instead of silently widening the search or tuning on evaluation labels. Persist Optuna trials, MLflow aggregates and hashed private provenance; add procedures and results to the LaTeX paper.
