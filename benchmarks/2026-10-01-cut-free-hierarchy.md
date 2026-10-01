# Cut-free hierarchy diagnostics: grafting stability is not semantic topic quality

Bead `mash-c0d.19.2.31`, follow-up `mash-c0d.19.2.32` for linkage comparison.
Exploratory diagnostics on already exposed development samples. No new reserved
test, threshold selection, inference, model downloads or generative calls.

## Frozen procedure

Use immutable `calibration-first.json` SHA256
`18fdb773b198d3f592738b5d39aaa90c5339658e0b11e57aa0f7b619cd05da04`
and `calibrated-evaluation-first.json` SHA256
`8819efd81ba90111f56a9cd6b2c79c4202ff71ca4b98fba3b0877c04c89e1954`.
The latter was reserved before the previous experiment, but **its outcomes are
now exposed**. Do not call this new hierarchy analysis a held-out evaluation.

Revalidate original source/split/anchors/declared labels/exact current/joined
input hashes, complete pair grids, installed-model hashes/dependency versions,
unit384D vectors and matched support. Reuse frozen lexical vectors/vocabulary
and 75/25 fusion. No new feature fitting. Five views:current/joined384D,
lexical3648D, fusions4032D. Same99/95 fit anchors and2590/2621 cross-original-group
evaluation pairs; inactive/unknown-proxy leaves remain in fitting and branch size.
Fit all leaves without annotation-driven selection. Labels enter metrics only.

Existing independent tree controls:greedy, rotate and GRINCH-style grafting,
exact leaf neighbors, float64 normalized centroid directions, stable arrival
ties, insert-only. Forward/reverse/seed17 shuffle, both samples:90 fits. Explicit
8192D bound and leaf buffer matched to sample size. No author-code parity claim.

For each evaluation pair, find its lowest common ancestor (LCA); score **negative
number of ALL fit leaves in that ancestor**. Smaller containing branches rank
closer. This fixed cut-free score is a structural retrieval proxy, not a semantic
ancestor label, a best-cut oracle, or standard single-label dendrogram purity.
Unknown/inactive leaves are not removed from branch mass to make purity look
better. Capture all canonical internal clades (including root), every pair's
LCA/hash/mass/score and all AP/AUC. Compare each tree to its own raw-view cosine
on unchanged service/intent/within-service pairs. No threshold sweep.

Topology stability relative to same-phase/view/mode forward tree:1024 fixed
seed20261001 source-identity triplets, closer pair by minimum LCA mass; exact
canonical-clade Jaccard excluding root/leaves. Triplets include all fit leaves,
not only label-eligible pairs. Save complete triplets/choices. Canonical hashes
describe fixed-sample clades for comparison; they are not persistent topic IDs
under updates. Full parent semantics, inherited tags and overlap remain untested.

## Results

Forward GRINCH-style tree versus raw cosine, identical support (AP):

| Sample | View | Raw service | Tree service | Raw intent | Tree intent |
| --- | --- | ---: | ---: | ---: | ---: |
| Calibration | Current | .456187 | .405068 | .395457 | .338070 |
| Calibration | Joined | .587312 | .467144 | .399572 | .352074 |
| Calibration | Lexical | .358725 | .329462 | .267301 | .216048 |
| Calibration | Current75+lexical25 | .452793 | .454998 | .388256 | .353164 |
| Calibration | Joined75+lexical25 | .590386 | .546365 | .411475 | .381590 |
| Previously reserved | Current | .544020 | .460673 | .476435 | .387587 |
| Previously reserved | Joined | .699960 | .583881 | .495495 | .381924 |
| Previously reserved | Lexical | .440251 | .407712 | .366614 | .322956 |
| Previously reserved | Current75+lexical25 | .541536 | .462871 | .478044 | .427439 |
| Previously reserved | Joined75+lexical25 | .699496 | .563002 | .514407 | .392737 |

Most forward tree scores lose service AP; all10 lose full-intent AP. This does not
prove that every cut or other branch score would be poor. Within-service AP is
mixed:previously reserved raw/tree current .794241/.742618, joined .688185/.651106,
lexical .778366/.749270, current fusion .799321/.838360, joined fusion
.707789/.679810. Preserve that exception rather than claim universal loss.
All other orders/modes and AUCs are captured, without selecting their best outcome.
Equal-ranking ties are handled by the standard AP/AUC definitions.

Mean topology agreement over five views x reverse/shuffled orders (10 controls
per sample/mode; descriptive means, no confidence or significance claim):

| Sample | Mode | Triplet agreement | Exact-clade Jaccard |
| --- | --- | ---: | ---: |
| Calibration | Greedy | .461523 | .081154 |
| Calibration | Rotate | .486230 | .211390 |
| Calibration | GRINCH-style | .658008 | .253830 |
| Previously reserved | Greedy | .421094 | .074777 |
| Previously reserved | Rotate | .481250 | .185219 |
| Previously reserved | GRINCH-style | .697754 | .221546 |

Grafting improves triplet agreement over rotation in19/20 comparisons, with one
tie. GRINCH-style triplet ranges .539063–.816406 calibration and .547852–.832031
previously reserved. Exact branches remain substantially unstable, despite broader
triplet improvement. Better stability does not consistently improve AP.

These results expose a problem beyond just a flat cut: this particular tree/score
combination often compresses away useful pair similarity. They do not establish
generic failure of GRINCH, whose author implementation/linkage/theory are not
replicated, or prove no semantic tree can be useful. Next compare linkage forms
that retain pairwise geometry (including vector-mean dot product versus normalized
centroid directions), with independent outcome separation, then multi-resolution
membership, labels/ancestry and guided search. Normalizing a diffuse mean changes
its score; the proposed linkage comparison is an algebraic hypothesis, not an
established quality fix. No production promotion or topic ancestor claim.

## Reproducibility and costs

`external_hierarchy.py` emits explicit JSON arrays for triplets/choices; a schema
round-trip test checks equality. Synthetic tests verify canonical clades/minimal
LCA sizes, retained unlabeled leaves, stability, exact perfect-ranking fixtures
and rejected incomplete/nonfinite inputs. One initial test incorrectly assumed
a greedy tree's LCA was the root; inspecting the actual three-leaf branch fixed
the expected value and confirms the unlabeled leaf is still counted. Algorithm
was not altered to satisfy that expectation. All195 research tests pass.

Two fresh captures agree exactly on all non-timing fields. Independent checks
derive pair LCAs from canonical clades, verify every scored pair, all92,160
triplet choices, AP/AUC and all90 hierarchies. Total after-imports8.309/8.515s;
all90 fits2.865/2.924s; validation/topology/metric work4.501/4.625s. Per-fit
7.814–104.635 / 7.355–120.213ms; insertion-p95 .0568–2.4849 /
.0515–2.9457ms. Fit timings include thread-limit setup, exclude validation,
LCA/metrics and serialization. CPU and insertion samples captured. Total excludes
imports/serialization. No new inference or background fit. No full ingestion,
RSS/p95-update/persistence/edit/delete/restart/parity or production scaling gate.

Private root `/home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001`:

- `hierarchy-first.json`: SHA256 `d2dbbb8de3583d4bc362e104bb8fc25e8b9b1a6798bb197a3a0f9b9fb6f96411`
- `hierarchy-second.json`: SHA256 `df953cfe548a219c848de4c907b7d1557b68294eaf48b8b8ef660a2abd199d4e`
- Matching durable `.log` files. Captures approximately45.46MB each, outside Git.

Run `external_hierarchy.py --root ROOT --calibration FROZEN --evaluation FROZEN
--output FRESH`, optional `--compare FIRST`. Source/model/dependency pins and
complete source input evidence are revalidated; output/code pins and all decisions
captured. No independent human coding judgments, labels, semantic ancestry or
guided-search gains demonstrated.
