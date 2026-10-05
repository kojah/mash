# Representation diagnostic after the closed heuristic screen

This is an exploratory explanation of failed quality gates, not another
clustering candidate or a threshold-selection protocol. Confirmation remains
sealed. Use the same development-only pair labels and immutable cached vectors.
Compare exactly three scores: raw separate-view pooled turn cosine,
whole-context pooled turn cosine, and maximum heading-structural chunk cosine
between two turns. Normalize vectors before cosine; reject zero/nonfinite
vectors and changed parent-report/matrix/annotation hashes.

Report ROC AUC and related/unrelated score medians for all, short, long,
nearby (distance <=4), and distant pairs. Also report the largest empirical
recall achievable at precision >=.85 by a single scalar threshold, treating
equal scores together. This uses the evaluation labels to choose the threshold,
so it is optimistically fitted descriptive evidence, not deployable performance
or an upper bound on all possible clustering algorithms. Do not publish an
accepted threshold or run confirmation from this diagnostic.

Structural maximum similarity is affected by the number of chunk comparisons;
report median comparison counts for positives/negatives. AUC differences across
length/distance slices may reflect pair composition, not a causal length effect.
The result should distinguish weak pairwise separability from losses introduced
by grouping, and motivate a separately registered next representation study.
