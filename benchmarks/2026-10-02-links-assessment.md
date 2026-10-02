# Links assessment and candidate-only formula/geometry preflight

Bead `mash-c0d.19.2.30.4.8`. Links merits a bounded reference trial, not production
selection. This assessment establishes source provenance and numerical contracts;
it does not run Links clustering or establish topic quality or performance.

## Primary sources and distinct mechanisms

The [original paper](https://arxiv.org/html/1801.10123v1) describes high-dimensional
unit-vector clustering with connected subclusters, size-dependent link thresholds,
merging and edge repair. Its common isotropic distribution and high-dimensional
approximations are modeling assumptions, not established properties of agent text.
Its two-level representation is not a semantic ancestor taxonomy or overlapping
topic assignment. Subclusters are indivisible; repairing links cannot undo their
internal source merges. The method requires calibrated thresholds rather than
eliminating parameter selection.

The [coauthor-hosted implementation](https://github.com/wq2012/LinksCluster/tree/33faf171cf145a1e0a8b35d75730db5aca22baf1)
is pinned separately from the 2018 publication. It defaults to empirical unit
centroids; theoretical normalization is optional. It distinguishes historical
online labels from final graph labels, retains source-index lists, uses set
iteration for neighbors and swap-and-pop centroid storage. These details require
an explicit deterministic contract before porting. Thirty-five selected upstream
functional tests pass. Hardware throughput tests and advertised throughput are
not adopted as mash performance evidence. Apache-2.0 source stays outside Git;
no package installation or model download occurs.

## Frozen preflight

Use the unchanged implementation defaults `Tc=.5, Ts=.8, Tp=1` solely for formula
assessment, not selected product parameters. Compare 25 size pairs drawn from
`[1,2,10,100,1000]` against the pinned reference. For `alpha=1/Tc^2-1`:

```
s(k,j) = 1 / sqrt((1 + alpha/k) * (1 + alpha/j))
s_tilde(k,j) = Tc^2 + (Tp-Tc^2)/(1-Tc^2) * (s(k,j)-Tc^2)
```

Singleton-pair cutoff is .25. With `Tp=1`, the singleton-to-large-subcluster
limit is .5 and the large-to-large limit is 1. These are formula properties,
not inferred text-topic thresholds. The helper accepts the reference's degenerate
boundary `Tp=Tc^2` explicitly; no such setting is evaluated on the corpus.

Reuse complete pinned balanced vectors from the [semantic study](2026-10-02-duplicate-semantic-retrieval.md).
The same candidate-only hash seed from the discovery study selects 512 sources;
evaluate all 130,816 distinct pairs, original vectors, no projection. No queries,
judgments, reserved outcomes or topic labels enter features. Report pair cosine
quantiles, counts at the two default cutoffs, complete corpus mean direction norm
and mean pair cosine using the complete sum identity. No threshold fitting/grid,
inference, generative call or paid service.

## Results and verification

Sample p05/median/p95 cosine is -.056455/.083977/.295948. Of 130,816 pairs,
11,728 (8.9653%) reach .25 and zero reach .8. Complete 32,176-source mean
direction norm is .308342; mean pair cosine .095046. This is background geometry,
not annotated within-topic distributions. The sample cannot rule out tight pairs
elsewhere or predict dynamic subcluster/link counts, connectivity or quality.
Nonzero empirical mean warrants checking distribution assumptions; it is not
a proof that Links fails or that centering would improve topics.

Fresh preflight repeats match all decoded nontiming fields. Independent component
reduction, extended-precision complete source accumulation and algebraically
equivalent scalar thresholds audit all pairs/quantiles/counts and 25 formula cases;
maximum numeric discrepancy 1.11e-16. 378 lab tests pass, including size dependence,
pair symmetry, anisotropic limits, degenerate boundary and malformed domains.
No Links cluster outcome, speed, memory or ≤10% ingestion gate is measured.

## Fit to mash and next trial

The previous [online cosine baseline](2026-10-02-online-cosine-discovery.md) fits
the corpus but leaves roughly 6,000 order-sensitive microgroups. A mechanism that
separates subcluster statistics from graph membership is worth evaluating against
that failure. It is not evidence that Links improves it. Exact centroid scans
scale with active subclusters and dimension; component checks and repair scans
can add work. Retained member lists add source-count memory. Big-O and advertised
throughput cannot substitute for measured corpus costs.

For mash, publishing irrevocable arrival IDs would conflict with revised topic
membership. Any selected implementation needs revisioned topic/tag publication,
ownership, stable identity/split policy and consistent label updates. Deleting
a source from a sum is insufficient when its earlier insertion caused irreversible
subcluster merges. Edits/deletes need a specified replay or stronger retained
state policy, with bounded costs; restart must preserve or reconstruct graph,
source ownership and revision state. Those capabilities are not demonstrated by
the reviewed insertion APIs. Also retain explicit invalid/zero-vector rejection
rather than adopting permissive reference normalization at application boundaries.

Next run the unchanged default reference under a frozen active-subcluster budget,
both canonical arrival orders, no threshold repair. Capture coverage, live graph
and final ownership consistency separately from arrival labels, exact repeats,
source-sum/edge audits and Linux memory costs. Stop with a structured feasibility
failure if the budget is exceeded. Connected components remain discovery outputs,
not semantic ancestors. Independent name/topic/navigation quality and lifecycle,
complete ingestion and Rust parity are still prerequisites for promotion.

## Artifacts and reproduction

Author checkout `topic-links-author-20261002`, commit
`33faf171cf145a1e0a8b35d75730db5aca22baf1`; 17-file source manifest SHA256
`c30686369754137063394c219edfdec904b1610b34655c55a5bf3b6ba4b0ac05`.
Run `links_preflight.py --semantic SEMANTIC_FIRST --semantic-repeat SEMANTIC_SECOND
--author PINNED_CHECKOUT --output FRESH_JSON`; repeat adds `--compare FIRST_JSON`.
All captures remain under `/home/james/scratch/mash-artifacts`:

- `topic-links-preflight-first-20261002.json`, SHA256
  `216a82b81320a9b2d1158bfe6db703794b1c59ca90f7cddc86f685d75bd4167d`.
- `topic-links-preflight-second-20261002.json`, SHA256
  `9cafd9911e3e8d5997928ad885abdf5194283e880fa3f02063e8a28c5f529cc6`
  (exact nontiming repeat).
- `topic-links-preflight-audit-first-20261002.json` (verified).

This preflight neither rejects nor selects Links. It permits a bounded reference
experiment while preserving all prior failed discovery outcomes.
