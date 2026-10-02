# Provisional compression acceptance contract

Freeze before the source-only feasibility experiment, 2 October 2026. These are
engineering targets for comparing candidates, not user-mandated constants or
demonstrated production guarantees. Any revision needs a new recorded protocol
and justification; do not silently relax a target after seeing an outcome.

- Retain at most 50% of original closed-block units as compression groups over
  the complete matched corpus. Every original block still has explicit ownership.
- At least 95% of blocks must have cosine >=0.90 to their assigned **unit**
  centroid. Zero/cancelled centroids cannot count as passing. This is a geometric
  fidelity guard, not a test of semantic topic quality.
- Conditional source-weighted all and disjoint-native-span hierarchy purity may
  lose at most0.02 absolute versus the immutable lexical-block reference.
  Preserve complete support and separately report internal partitions and rare
  topic support; existing training/internal partitions are already explored.
- Incremental compression alone: mean CPU <=0.5ms / closed block, p95 wall
  <=1ms / closed block on a specified matched workload. Additional persistent
  clustering state <=16MiB per10,000 active blocks, separate from existing source
  index/model assets. Context, labels, persistence and recovery costs are additional.
- The existing complete-ingestion <=10% throughput-loss target, matched initial
  indexing/incremental edit-delete/p95/CPU/RSS measurements, independent coding
  labels/multi-tags/ancestors/guided-search quality and chosen Rust differential
  CI still gate promotion. Passing this contract alone never selects production.

## Feasibility protocol

Use every immutable source-only16-turn/512-token/gap block, no gold for selecting
thresholds, no new model inference/LLM/projection/heldout access. Fixed views:
original sparse lexical unit vectors, unchanged pooled dense directions, and an
equal-energy direct-sum lexical+dense view (cosine is the arithmetic mean of the
two view cosines). No view-weight grid. Groups are per meeting as in prior controls;
this does not establish feasibility for a future shared group spanning threads.

For unit x,y and any unit centroid c, if x.c and y.c are both >=t, then x.y >=
2t^2-1. This follows by decomposing each vector into its component along c and
orthogonal residual, taking their most adverse residual dot product. At t=.90,
two passing members of one group require pair cosine >=.62. Build the graph of
possible such pairs, conservatively including a1e-12 numerical margin. Every
isolated block must own a distinct centroid unless it is one of the allowed
outliers. Thus over per-meeting groups a conservative lower bound is
max(number_of_nonempty_meetings, total_isolated - floor(total_blocks/20)).
Allowed groups are floor(total_blocks/2). If the lower bound exceeds that budget,
no per-meeting algorithm using this view can meet both targets. Otherwise report
**not ruled out**, never feasible/proven. Global cross-thread neighbors, different
representations or a revised fidelity budget require new explicit experiments.

Store all neighbor identities, isolated identities, closest cosine values,
Gram hashes, support and code/input/protocol pins; require a full fresh repeat,
independent source/view/pair/threshold/bound arithmetic, scoped costs and paper.
This experiment fits no clusters and cannot measure topic/label quality or actual
online update/ingestion/state cost.
