# Cut-free hierarchy quality before frontier selection

The [actual discovery controls](2026-10-02-qmsum-discovery-controls.md) rejected
two operating points, but a poor flat cut does not prove a poor hierarchy. Hold
the complete audited GRINCH-style topology fixed and evaluate broader relationships
without another cutoff grid. This is exploratory training-corpus analysis, not
held-out selection or coding-session gold.

## Protocol frozen before outcomes

All162 original QMSum training meetings,88,336 source turns,84,434 common-ready
leaves and77,892 singleton-known leaves. Both final insertion orders come from
unchanged discovery capture SHA256
`c9587e541ff38478a8c074cf2f28992d187f03ed18c15e024fbebe7d46ee0b3d`.
Source SHA256
`55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`.
No new fitting, cuts, embeddings, LLM, downloads or held-out access. GRINCH-style
is not authors' Scala execution/parity.

Adapt the [GRINCH paper's dendrogram-purity measure](https://arxiv.org/abs/2001.00076):
for each same-topic distinct pair, find its lowest common ancestor (LCA), score
the fraction of that ancestor's **singleton-known** descendants with the pair's
topic, and average over eligible positive pairs. Unknown/multi-annotated sources
remain in geometry, outside this conditional denominator. This differs from
fully labeled standard dendrogram purity; unknowns are neither negative nor clean.

Prespecify scopes: all positive pairs; original ordinal distance>4; and distance>4
plus distinct merged native topic spans. The last uses the prior study's explicitly
post-hoc definition, now frozen before these outcomes. Merge overlapping/adjacent
same-topic spans; all771 original spans already need no merge. Original distance
excludes overlapping five-turn context components. Names never enter features.

Fixed source-only baselines, independent of model insertion order:

- Root-only: each pair receives its topic's per-meeting known prevalence.
- Chronological balanced binary tree: recursive floor-midpoint source-order split.
- Forward comb: successively join the next original ready leaf.
- Reverse comb: successive joins in reversed original ready order.

Report pooled purity, per-topic macro purity excluding zero-pair topics, pair-weighted
LCA known-leaf count, root-LCA fraction and complete explicit zero support. No gold
drives splits; none of these geometric nodes supplies semantic ancestors.

## Complete results

| Hierarchy | All-pair purity | Distance>4 purity | Crossspan purity |
|---|---:|---:|---:|
| GRINCH-style forward | .458737 | .451224 | .552117 |
| GRINCH-style reverse | .445872 | .438348 | .543249 |
| Root-only | .383562 | .385738 | .529642 |
| Chronological balanced | .829216 | .823855 | .557285 |
| Forward comb | .526075 | .529036 | .561211 |
| Reverse comb | .571680 | .575283 | .543966 |

Positive counts:8,455,716 all,8,162,992 distance>4,84,350 crossspan. Supported
topics722/706/30 out of725. The33 multi-span native topics yield only30 supported
return topics across22 meetings after support/distance exclusions. Others remain
explicit zero-pair topics rather than successful abstentions.

All-pair topic-macro purity forward/reverse .406168/.413153 versus balanced
.806953; crossspan .360514/.351384 versus balanced .393958, forward-comb .373845
and reverse-comb .345206. Different scope support changes prevalence: crossspan
root-only .529642 is already high. These are descriptive corpus outcomes, not
independent pair-level confidence claims or generalization. Compare within scopes.

All-pair mean LCA known rows542.66/546.86 versus balanced313.24; crossspan
257.98/258.30 versus316.45. Crossspan root-LCA fraction2.01%/1.15% versus balanced
73.61%. The model brings many returns below the root, but those nodes provide
little purity above background. Lower LCAs alone do not establish useful topics.

GRINCH-style underperforms all three temporal baselines on broad all/far purity;
returns only modestly improve on root-only prevalence. Chronological grouping
scores well partly because gold spans are contiguous; it is not a selected topic
algorithm. Sparse meeting-return gold is not a global coding taxonomy. Evidence
does not justify optimizing a frontier of this topology, or universally reject
GRINCH, another linkage/representation or BIRCH compression. Prioritize bounded
source-only episode geometry, dense/lexical evidence and duration/adjacency controls
before separate macro-topic discovery. No production method or semantic labels
are selected.

## Repeats, audits and reproduction

`captured_hierarchy.py` verifies complete ownership/connectivity/parent/counts.
Postorder class/component counters count same-topic cross-child pairs at unique
LCAs; explicit short-distance pairs are removed at their LCA. Accumulate fractions
with `math.fsum`. `qmsum_hierarchy.py` records all1296 meeting/order/hierarchy cases
and complete per-topic support. Five fixtures cover perfect/interleaved trees,
returns/adjacency, unknown conditional denominators, zero support, invalid topology
and strict whole-capture repeat comparison. Enabled lab suite:472 tests pass.

First repeat comparator attempt failed after completing measurement: the reused
comparator required timing fields absent from this intentionally untimed diagnostic.
No partial repeat output was published. Fix only the driver comparator to compare
every canonical JSON field/type and remove the unused comparator dependency pin.
Two fresh complete final captures are byte-identical SHA256
`39a2b42f48c1d17ff7ba7a0483ae301e04e6383e658f54cfdf8faec9cf325de3`.
Original completed capture preserved SHA256
`de093002bceec5bac630eafb5f4eed7439a34ee2ce48e114c008a94a651f8398`.

Separate original-source auditor imports neither metric nor source helpers.
Direct JSONL memberships, independently constructed temporal trees and cross-child
leaf-pair masks verify all1296 cases,1,349,648 nodes and17,400 topic/scope metrics;
integer counts exact, maximum numeric error2.91e-11. Independently confirms77,892
known rows,771 disjoint spans and33 recurring topics. Auditor SHA256
`5d008b83ee7d857839f5a317959221fac869c754ae657bc3ffbdec41de9e84c6`;
terminal `topic-qmsum-hierarchy-audit-first-20261002.json`.

Separate aggregate audit verifies all24 summary scopes, maximum error5.55e-17,
and the provenance bridge to final captures: **only** driver comparator pin and
removed unused dependency differ; every metric/helper/source/protocol field matches.
Auditor SHA256
`037eb27ce3c7146855aea0f054f9593e59d7c609f58403a4bc6548977c9207e7`;
terminal `topic-qmsum-hierarchy-summary-audit-first-20261002.json`.
Shared source/topology/numeric engines remain; audits do not independently replay
graft insertions or establish semantic quality.

All artifacts outside Git under `/home/james/scratch/mash-artifacts`. Scientific
environment is in the discovery-controls report. Run:

```text
python research/topics/qmsum_hierarchy.py
  --source .../topic-qmsum-author-training-20261002
  --capture .../topic-qmsum-discovery-grinch_style-first-20261002/results.json
  --output FRESH_OUTSIDE_GIT.json
  [--compare FIRST_FINAL.json]
```

No runtime/performance gate here. Complete ingestion, memory, lifecycle,
provider/group namespaces, coding/multi-tag/ancestor quality and chosen Python/Rust
parity remain unverified. Completed bead `mash-c0d.19.2.20.8.3.3.1`, parent active;
P0 follow-up `mash-c0d.19.2.20.8.3.3.2` tests episode geometry before macro discovery.
