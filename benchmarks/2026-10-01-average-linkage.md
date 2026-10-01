# Average-cosine linkage improves some tree quality and stability

P0 bead `mash-c0d.19.2.32` remains in progress. This is an exposed-sample ablation,
not fresh held-out validation or production selection. No new inference/refit,
downloads, generative calls, cuts, threshold search or outcome-based method choice.

## Hypothesis and explicit implementation

For unit leaf vectors, `(sum(A)/count(A)) dot (sum(B)/count(B))` equals the average
cross-cluster leaf cosine. Using `1-cosine` gives the familiar average pairwise
distance definition ([SciPy linkage documentation](https://docs.scipy.org/doc/scipy/reference/generated/scipy.cluster.hierarchy.linkage.html)).
We implement the similarity algebra inside the existing exact-leaf online
rotate/graft prototype; **not SciPy's offline agglomerative procedure** or author
GRINCH code. Our original `centroid` enum means normalized-centroid cosine,
not SciPy's Euclidean `centroid` method. No novelty/theory-transfer claim.

Add concrete `Linkage.CENTROID`/`AVERAGE`, retaining centroid default.
`Node.link_vector` replaces the misleading direction field: normalized sum for
centroid, unnormalized mean for average. One helper computes it for creation,
aggregate refresh and validation. Unit leaf vectors, exact neighbor search,
arrival ties, strict graft preferences and bounded buffers unchanged. Average
means preserve cancellation/cohesion magnitude; centroid normalizes it away.
This is an algebraic motivation, not evidence that normalization caused all
previous false merges.

Three tests verify average score against explicit all-leaf cross-cosine matrices
for generated inserts/all modes, exact sum/count refresh, canceling vectors,
old default equivalence and invalid linkage rejection. A separate guardrail test
rejects changed legacy policy, input order, decisions, schema or extra fields.

## Frozen comparison and compatibility evidence

Reuse immutable calibration99/previously reserved95-anchor phases and all five
views/current/joined/lexical/fusions from the previous cut-free experiment; these
outcomes **are already exposed**. Revalidate source/split/anchors/original model
input hashes, unit vectors and complete2590/2621 eligible cross-group pair grids.
Each linkage:five views x greedy/rotate/GRINCH-style x forward/reverse/seed17
arrivals x two samples =90 trees,180 total. Same negative-all-fit-leaf LCA-mass
AP/AUC and1024 fixed-triplet/exact-clade stability metrics; no new score/cut.

The original90 centroid trees reproduce **every** archived input, canonical
clade, pair LCA/mass, triplet choice, metric and statistic exactly against
`hierarchy-first.json` SHA256
`d2dbbb8de3583d4bc362e104bb8fc25e8b9b1a6798bb197a3a0f9b9fb6f96411`.
Only declared schema version1→2, added explicit linkage policy, code pins and
timings differ. `compare_legacy` forbids all other drift. No general normalization
or tolerance excuses changed decisions. The wrapper moves both nested timing
objects into one explicit top-level `timings`, so strict repeat comparison still
excludes only that field. Historical captures remain immutable and require their
original commits for full code/schema reproduction.

## Results

Forward GRINCH-style AP, centroid→average, same pair support:

| Sample | View | Centroid service | Average service | Centroid intent | Average intent |
| --- | --- | ---: | ---: | ---: | ---: |
| Calibration | Current | .405068 | .405070 | .338070 | .337181 |
| Calibration | Joined | .467144 | .527619 | .352074 | .356006 |
| Calibration | Lexical | .329462 | .355807 | .216048 | .269148 |
| Calibration | Current75+lexical25 | .454998 | .418945 | .353164 | .352512 |
| Calibration | Joined75+lexical25 | .546365 | .547115 | .381590 | .363195 |
| Previously reserved | Current | .460673 | .538095 | .387587 | .464991 |
| Previously reserved | Joined | .583881 | .594355 | .381924 | .384344 |
| Previously reserved | Lexical | .407712 | .428525 | .322956 | .354518 |
| Previously reserved | Current75+lexical25 | .462871 | .494085 | .427439 | .447929 |
| Previously reserved | Joined75+lexical25 | .563002 | .607329 | .392737 | .412305 |

All five previously-reserved forward full-intent APs improve; three of five
calibration full-intent APs decrease. Average remains below corresponding raw
cosine intent AP for all five previously-reserved forward views. Within-service
AP is mixed:current there .742618→.788138, joined .651106→.651011,
lexical .749270→.785349, current fusion .838360→.796840, joined fusion
.679810→.681644. Preserve these losses; no universal quality claim.

Across all90 paired controls, service AP improves in43, full-intent in41,
within-service in31. All30 greedy trees are exactly unchanged across linkages
(including every topology/metric/statistic); their insertion uses only nearest
leaves, without aggregate rotate/graft preference. These counts are descriptive
dependent comparisons, not significance tests or votes selecting a method.

Mean stability over five views x reverse/shuffled (10 controls per sample/mode):

| Sample | Mode | Centroid triplet | Average triplet | Centroid clade Jaccard | Average clade Jaccard |
| --- | --- | ---: | ---: | ---: | ---: |
| Calibration | Greedy | .461523 | .461523 | .081154 | .081154 |
| Calibration | Rotate | .486230 | .539063 | .211390 | .261946 |
| Calibration | GRINCH-style | .658008 | .753906 | .253830 | .360156 |
| Previously reserved | Greedy | .421094 | .421094 | .074777 | .074777 |
| Previously reserved | Rotate | .481250 | .544434 | .185219 | .229375 |
| Previously reserved | GRINCH-style | .697754 | .788672 | .221546 | .336359 |

Average improves aggregate stability but exact branches remain weak, at mean
Jaccard .36/.34. Both gains and failures matter: no stable semantic ancestors,
multi-memberships, useful labels, or guided-search gain demonstrated. Next test
the promising linkage with preregistered independent calibration/outcome groups,
false-merge/coverage goals, label/rare-topic/ancestor checks and measured lifecycle
cost. Do not promote from the exposed dataset or simply retune to its best view.

## Repeat and cost evidence

Two fresh comparison captures match every non-timing field exactly. Independent
checks derive every scored LCA from clades, verify all180 trees,184,320 triplet
choices, AP/AUC, unchanged30 greedy trees and preserved90 legacy centroid outputs.
All199 isolated research tests pass. Wrapper includes input/schema/code pins,
complete nested captures and paired deltas; no raw source histories in Git.

Total after-imports15.989/16.041s for both linkages. Centroid90 fits2.785/2.735s;
average2.366/2.420s. Topology/metric work4.154/4.118s centroid and4.145/4.132s
average. Average per-fit6.300–88.965 / 6.499–87.306ms; insertion-p95
.0389–1.8832 / .0399–1.8397ms. Centroid ranges7.248–109.941 /
7.503–105.947ms and p95 .0513–2.4607 / .0542–2.4423ms. Fit includes
thread-limit setup, excludes validation/topology/metrics/serialization; CPU and
insertion samples captured. One-thread BLAS, bounded exact-neighbor prototype.
Faster aggregate fits on these tiny samples do not satisfy the complete-ingestion
≤10% throughput-loss/RSS/p95-update/persistence/edit/delete/restart/parity gate.

Across15 GRINCH-style controls per phase, average has more grafts and comparisons
but fewer rotations:calibration centroid996grafts/373897comparisons/3951rotations,
average1211/408680/2057; previously reserved893/348961/3922 versus1010/373344/2086.
Do not attribute wall-time differences solely to cheaper arithmetic; scheduling
and changed restructuring work also differ. No production scalability claim.

Private root `/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`:

- `linkage-first.json`: SHA256 `10527284a2fb8ee0a66ebe2a1d726a303635383585bf19330a79ad6e427275df`
- `linkage-second.json`: SHA256 `32bc025474f639396861d5b58e2a8d2a6e17e3c4733cabbcb988467b229c85a0`
- Matching `.log` files, approximately89.91MB each, outside Git.

Run `linkage_comparison.py --root ROOT --calibration FROZEN --evaluation FROZEN
--legacy ORIGINAL_HIERARCHY --output FRESH`, optional `--compare FIRST`.
The standalone hierarchy runner retains centroid default, emits explicit version2
linkage provenance, and requires version-matched captures for its strict repeat.
