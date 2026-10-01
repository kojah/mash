# Frozen external online clustering: ranking gains do not establish topic groups

Development diagnostic, not production selection. Bead `mash-c0d.19.2.31` remains
open. No generative calls, new inference, topic names or annotations in fitting.

## Frozen procedure

Use the same 32 hash-selected MultiWOZ development dialogue groups and 206 common
current/mean/joined supported USER anchors. Validate source/model inputs with
`external_analysis.prepare`. Frozen vector capture SHA256:
`62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c`.
Frozen lexical capture SHA256:
`21e9b729899037ac8f8680ffcc5b2515f2f76fb3cad2fdfcd2e2723744ea4b48`.
Background vocabulary/IDF were already fit on 480 disjoint development groups,
with exact evaluation text and duplicate background text removed. No refit here.

Five fixed representations: current and joined normalized dense vectors (384D),
lexical (3783D), current/ joined concatenated with lexical (4167D), using
sqrt(.75) dense and sqrt(.25) lexical weights. This has exactly the earlier
arithmetic fused cosine; normalized inputs remain unit length. No PCA or changed
normalization. Lexical coordinates are dense for this small diagnostic, so timings
do not characterize an optimized sparse implementation.

Singleton online [BIRCH](https://doi.org/10.1145/233269.233324) `partial_fit`: radius .4, branching factor64,
`n_clusters=None`. Final `predict` assigns all inputs to nearest CF subclusters;
these are final predictions, not captured insertion-time memberships. Existing
GRINCH-style independent prototype: exact leaf neighbors, rotate/graft,
all-internal-cosines-at-least .7 cut. These reuse **uncalibrated**, different
operating points from earlier experiments; no threshold sweep or outcome-based
choice. GRINCH-style code is not author implementation parity. BIRCH CF groups
and tree cuts are not verified semantic topics or parent relationships.

Fit all 206 anchors, including unknown/inactive proxy annotations; gold does not
control input admission. Arrival orders: lexicographically sorted stable source
identity, reverse, NumPy seed17 permutation. This is a sensitivity control, not
timestamp order. Fit support, complete arrivals and canonical partitions saved.
Evaluate only after fit, on unchanged 12341 cross-original-dialogue-group pairs:
service 3448 positives/8893 negatives; intent2488/9853; within-service intent
3448 pairs, 2488 positives/960 negatives. Predict positive iff two anchors share
a final cluster. Report all TP/FP/TN/FN, precision/recall/F1. Undefined precision
is null, never treated as a good score. Forward-relative ARI evaluates all 206
anchors, including those excluded from pair metrics. No uncertainty estimate
for these cluster diagnostics.

Initial run stopped before output because the existing tree rejected >1024D.
Retain its log. Add an explicit maximum_dimensions setting bounded at8192,
retaining the default1024, and size the leaf buffer to206. Regression test proves
default rejection and explicit opt-in. No failed-run output used. Both fresh
final runs share this code and policy. Old captures retain their original code
pins and require their original commits for exact reproduction.

## Results

Forward arrival only (all other results in private captures):

| View | Method | Groups | Singleton anchors | Service F1 | Intent F1 | Within-service F1 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Current | BIRCH | 170 | 140 | .0104 | .0135 | .0136 |
| Current | GRINCH-style | 144 | 123 | .0520 | .0696 | .0705 |
| Joined | BIRCH | 173 | 141 | .0087 | .0112 | .0112 |
| Joined | GRINCH-style | 156 | 131 | .0618 | .0754 | .0754 |
| Lexical | BIRCH | 202 | 198 | .0012 | .0016 | .0016 |
| Lexical | GRINCH-style | 203 | 200 | .0006 | .0008 | .0008 |
| Current75+lexical25 | BIRCH | 191 | 178 | .0052 | .0064 | .0064 |
| Current75+lexical25 | GRINCH-style | 189 | 177 | .0069 | .0088 | .0088 |
| Joined75+lexical25 | BIRCH | 204 | 202 | .0000 | .0000 | .0000 |
| Joined75+lexical25 | GRINCH-style | 205 | 204 | .0000 | .0000 | .0000 |

All 30 runs have service recall at most .0319 and intent recall at most .0394.
High precision among the few merges does not make the grouping useful. Finer
proxy labels are only declared find/book intents, not independently judged
granular coding topics; matching labels across dialogues is a deliberately
coarse diagnostic, not proof every such pair belongs to one coherent topic.

Forward-relative ARI reverse/shuffled: current BIRCH .468/.635, current tree
.754/.767; joined BIRCH .563/.647, joined tree .697/.662. Current fusion BIRCH
.909/.689 and tree1/1. Lexical and joined fusion have1/1 for both methods, but
almost all anchors are singletons. Stability cannot substitute for useful recall.
Some non-singleton merges occur only within one dialogue and are intentionally
excluded from the cross-dialogue evaluation; no synthetic cross-group success.

Pair-ranking changes therefore do **not** transfer automatically to a fixed
cluster radius/cut. Added lexical coordinates can reduce many absolute cosines
even when useful rankings improve. These controls do not show BIRCH or GRINCH
is inherently poor: the thresholds were not calibrated to these representations.
Do not select a representation or algorithm from this diagnostic. Next calibrate
granularity on separate development groups and freeze it before reserved
evaluation, examining hierarchy and overlapping membership rather than one flat
cut alone. Preserve broad/fine tradeoffs, rare topics, abstentions and fragmentation
checks. Need independent coding/provider, label, ancestry and guided-search evidence.

## Repetition, costs and limits

Two fresh runs agree exactly on every non-timing field: source/code/dependency
pins, arrival sequences, complete memberships, sizes, tree statistics, confusion
counts and ARI. Independent verification recomputed all confusion counts and
12341 fused cosine scores. NumPy2.4.6, scikit-learn1.9.1; one-thread BLAS/fit.
All186 isolated research tests pass.

Total singleton fitting for30 controls:6.968/7.455seconds. Per-control wall range
69.04–524.48 / 65.42–570.99ms; per-insertion p95 range .444–7.119 /
.371–7.847ms. Fits exclude final prediction, validation, cuts, metrics and
serialization; include thread-limit setup. CPU and individual insertion samples
are captured. No new model initialization or calls, no full ingestion/RSS gate,
no labels/persistence/edit/delete/restart cost, no production scalability claim.

Artifacts outside Git under
`/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`:

- `clustering-final-first.json`: SHA256 `95e89471135cf7f9984fad0feb156f341a61467c78329d789b08b646c5b273c3`
- `clustering-final-second.json`: SHA256 `bb8072788bb30d2c543e7b78794ddb1404834fca71f0794d1085e8c09300d201`
- Corresponding `.log` files; failed initial `clustering-first.log` preserved.

Run `research/topics/external_clustering.py` with explicit `--root`, `--capture`,
`--lexical`, fresh outside-Git `--output`, optional `--compare`. Source/model
captures and original manifest remain private artifacts; upstream public corpus
provenance is documented in `EXTERNAL_EVALUATION.md`. No production Rust promotion.
