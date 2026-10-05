# Representation diagnostic: completed

The [prospective diagnostic scope](turn-representation-diagnostic.md) used
cached matrices and development labels only. It did not fit another clustering
candidate or open confirmation. Each view contains 617 related and 313 unrelated
decisive pairs; 121 ambiguous pairs are excluded. Long-history subsets contain
314 related and 243 unrelated pairs.

| Scalar score | Overall AUC | Long AUC | Overall optimistic R at P>=.85 | Long optimistic R at P>=.85 |
| --- | ---: | ---: | ---: | ---: |
| Raw pooled cosine | .807 | .767 | .707 | .357 |
| Whole-context pooled cosine | .859 | .843 | .828 | .688 |
| Maximum heading-chunk cosine | .851 | .826 | .838 | .678 |

The recall column scans all observed score thresholds using these same labels,
keeping ties together. It is optimistically fitted descriptive evidence, not
held-out performance, a deployable threshold or an upper bound on arbitrary
clustering methods. Even this favourable scalar diagnostic falls below .70
long recall for both enhanced views. Context helps long-pair separation, but
the remaining gap cannot confidently be attributed only to BIRCH grouping.

Distant-pair AUC is .742/.787/.764 for raw/context/structure. Optimistic recall
at .85 precision is .052/.311/.096. Short and nearby slices have many more
positives than negatives, so their much higher attainable recall must not be
interpreted as evidence of a causal length or proximity effect. Structural
maximum cosine also benefits from multiple comparisons: median comparison
counts for related/unrelated pairs are 24/16 overall and 28/21 long. This bias
limits comparison with single-vector scores.

The first invocation stopped before publishing results because the raw matrix
filename used `raw` instead of the existing `raw_separate` artifact name. The
runner now uses the owning enum. Failed logs are retained; no model, inputs,
labels or diagnostic definitions changed. Restart completed in 6.16s with
peak RSS 1,046,524 KiB. Matrix hashes match the frozen parent reports and
cohort/annotation hashes match both parents. Tests cover score ties and invalid
vectors; all 112 experiment tests pass.

Private result:
`/home/james/scratch/mash-artifacts/turn-representation-diagnostic-20261005/development.json`.

- Protocol SHA256: `caa4ab603825c0998ffb32579a4b24627abbf5de45cfd17d03d98fa1d9a3a30a`.
- Result SHA256: `098986438a4903de30891d7587918d79c8af122298334d6a7224c42cfffcd9fa`.

No algorithm is accepted. Further work should test a stronger semantic
representation/affinity under a separate bounded protocol, rather than tune
the closed screen. The installed local non-generative reranker is one available
option to assess before acquiring another encoder; its search-relevance scores
must not be assumed to express topic equivalence.
