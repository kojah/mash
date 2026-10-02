# Source-preserving FISHDBC reference control

Frozen before corpus outcomes, 2026-10-02. This is an exploratory accuracy and
reference-capability experiment, not selection or an ingestion performance gate.
It follows the rejected fixed dense-compression/lexical-macro operating point.
Compression is optional; preserve every original physical block.

Author reference: matteodellamico/flexible-clustering commit
`bf73d1ac6ce8bcdb21232978e2deb99d2c2582c6`, tree
`3b4dd826c4722a0c05935721d6e665aba6da601b`, BSD-3-Clause. Pin every tracked
file plus generated unionfind extension and installed dependency versions before
execution. Paper: https://arxiv.org/abs/1910.07283. No source patches.

Use the immutable 162-meeting/8,196-block QMSum geometry and original lexical
hierarchy reference; retain all unknown/multi evidence in geometry and the same
original-source all/far/crossspan conditional LCA metric, excluding same-block
positives. Original sparse sublinear TF-IDF cosine is primary, unchanged dense
cosine a control. Precompute clipped 1-cosine distances with zero diagonal for
this reference control. Thus distance matrix construction/storage is offline and
timings are not realistic online feature retrieval costs. No LLM/new inference,
projection, radius/cut/weight grid, holdout or gold-aware fit.

FISHDBC: min_samples=5, HNSW m=5, ef=50, m0/default level multiplier,
heuristic=True, balanced_add=True, vectorized=False. Reset Python random seed
20261002 for each meeting/view/order; forward and reverse original-block arrival.
After each 16 additions and final remainder extract the full author clustering;
first extraction only once >=6 points exist. Capture prefix memberships/noise,
probabilities, condensed tree, single-linkage tree, MST, core-distance heaps and
HNSW graphs. Final source tree maps arrival leaves back to original identities.
Extraction defaults EOM, min_cluster_size=5, no single cluster/reference matching.
Density hierarchy/flat groups do not prove semantic ancestry or multiple tags.

Exact diagnostic per final view/order: fifth-other-neighbor distances (matching
the author's self-exclusion), all-pair mutual reachability, independent Kruskal
sorted (weight, arrival_i, arrival_j, original_distance). Compare cores, MST
total weight, and source-weighted hierarchy quality. Equal MST weight does not
imply identical tied topology. Exact control uses the same hdbscan tree labeling
routine for isolation; it is not an independent HDBSCAN implementation.

Store every final source tree/merge/core/MST/graph/prefix assignment and all source/
code/contract/dependency pins; fresh full repeat must match every non-timing field.
Fixtures include disjoint ownership, tied distances, approximate vs exact MST,
prefix schedule, malformed matrices and deterministic repeats. Independent scalar
original-source distance/core/MST/LCA audit is required before conclusions.

Timers distinguish setup/distance matrix, author add, extraction, exact diagnostic
and source evaluation. Include full capture wall/CPU plus external RSS/PSS/rusage;
one target, observer/children excluded, intrusive sampled lower bounds/status
approximate/launcher high water possible, no subtraction. No speed claim from
precomputed distance callbacks, small per-meeting graphs or partial timers.

Provisional usefulness: recurring-topic purity loss <=.02 vs original lexical
average reference in both arrival orders; report all-pair/far, root/chronological
baselines, noise fractions and approximate/exact differences. Explored internal
108/54 split is not pristine heldout. This gate is insufficient for selection:
independent coding label/multitag/ancestor/guided-search evidence, stable global
node-group/provider frame, real distance/state/update/extraction costs, bounded
edits/deletes/restart/persistence, complete ingestion <=10% loss and chosen
Python/Rust differential CI remain mandatory. No implicit deletion support or
semantic hierarchy claim follows from incremental addition.
