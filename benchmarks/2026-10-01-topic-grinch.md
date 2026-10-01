# Exact GRINCH versus BIRCH: first real-history comparison

The bounded Rust GRINCH prototype runs on real histories, but this trial does
not justify replacing BIRCH. Exact fitting is slower, order stability changes
direction across settings, and bounded source reviews show assignment changes
without an established semantic-quality advantage. Keep it opt-in.

## Algorithm and boundaries

The [GRINCH paper](https://arxiv.org/abs/2001.00076) describes online hierarchical
clustering with rotations, non-local grafts and post-graft restructuring. Its
order-independent recovery theorem assumes a linkage that separates the latent
ground-truth graph; conversation embeddings do not establish that assumption.

This independent Rust implementation uses cosine linkage between normalized
subtree sums and exact leaf nearest-neighbor search. Insertion rotations follow
the [author implementation's](https://github.com/iesl/grinch/blob/master/src/scala/grinch/core/Grinch.scala)
pre-insertion ancestor climb. Grafts require mutual preference; rejection on
ties prevents loops. Restructuring follows the paper's ancestor-sibling search.
When the old sibling is the removed parent, restructuring is skipped, as in the
author implementation. A zero subtree sum has zero linkage, avoiding undefined
cosine on exact cancellation.

Detached parent slots are reused, leaving exactly `2*n-1` live nodes. Limits are
8,192 input points, 2,048 dimensions and 100 million linkage scores. A score-budget
failure poisons the experiment and prevents publishing a partial cut or
snapshot. Invalid input is rejected before mutation. No ANN, height cap, single
elimination, or cached-neighbor approximation is used in this initial baseline.

The maximal subtree cut uses the same RMS radius as BIRCH: for normalized
points, `radius² = 1 - ||mean||²`. This aligns the threshold's meaning, not the
resulting number of topics. Subtree radius and centroid linkage need not be
monotone. Overlapping thread assignment, support weighting, label formulas,
and semantic ancestor construction remain the existing pipeline. The native
binary dendrogram is **not** automatically published as a semantic taxonomy.

Enable `MASH_TEST_TOPIC_GRINCH=1` in the offline benchmark. This is not a
production configuration or MCP feature. Cut identities use the earliest
insertion ordinal; they are not stable production topic identities.

## Protocol and evidence

Frozen corpus: 225,267 messages, 789 threads (515 Claude Code, 274 Codex),
6,714 compatible user vectors, generation
`ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`.
Representative selection yields 704 unique full-corpus discovery inputs;
whole-thread anchoring yields 484. Reserved-source runs use 602/405 inputs.
This is not a large-scale raw-vector ingestion benchmark.

Compare two algorithms, two input modes, radii 0.4/0.6, forward/reverse insertion,
and full/reserved-source training: 32 corpus reports plus eight focused label
reviews. Fix singleton candidate inference, support-aware labeling, discourse
masking, conserved support, visible-link handling and context enrichment.
Use original centroid label scoring, not the rejected representative-scoring
ablation. Label formulas are fixed; their vocabularies/statistics naturally
change with assignment.
Reserved-source exclusion applies to discovery fitting. Existing label
statistics still use assigned threads, including reserved sources; this is not
a fully held-out label-training evaluation.

The external audit verifies that BIRCH controls reproduce the previous trial,
both algorithms receive identical training-vector fingerprints, and eligibility,
canonical source identities and embedding generation agree. It checks every
GRINCH snapshot for connected acyclic binary topology, reciprocal parent links,
counts, earliest-point identities, exhaustive disjoint leaf coverage and a cut
matching reported topics. It also verifies direct/inherited provider counts,
semantic forest relationships, focused selected labels and score arithmetic.
Original vectors/centroids are not serialized in topology snapshots; external
auditing does not independently recompute linkage or radius from raw vectors.
Rust tests verify subtree sums and centers after operations.

## Initial results

Full-corpus representative discovery takes 1.32–2.50 seconds to fit GRINCH versus
0.39–0.50 seconds for BIRCH across the four configurations. Whole-thread anchors
take 0.61–0.83 seconds versus 0.16–0.22 seconds. These are sequential measurements
on a busy host, not latency percentiles or CLI search timings. Cut/snapshot
extraction is reported separately.

For full representative forward insertion, GRINCH performs 3,285,433 linkage
scores, 3,036 insertion rotations and 746 grafts; reverse insertion performs
1,766,557 scores, 3,832 rotations and 542 grafts. Restructuring checks execute,
but accept no swaps in these corpus configurations. A targeted structural test
exercises an accepted restructuring swap. Native tree depths range 76–146;
they are not semantic topic depth.

Canonical-source co-assignment Jaccard measures forward/reverse stability:

| Training | Input | Radius | BIRCH | GRINCH |
|---|---|---:|---:|---:|
| Full | Representatives | 0.4 | 0.9185 | 0.6137 |
| Full | Representatives | 0.6 | 0.3965 | 0.8507 |
| Full | Anchors | 0.4 | 0.8981 | 0.6198 |
| Full | Anchors | 0.6 | 0.5779 | 0.7213 |
| Reserved-source | Representatives | 0.4 | 0.9213 | 0.9613 |
| Reserved-source | Representatives | 0.6 | 0.8543 | 0.6958 |
| Reserved-source | Anchors | 0.4 | 0.8911 | 0.9668 |
| Reserved-source | Anchors | 0.6 | 0.5515 | 0.6697 |

This is not precision, recall or dendrogram purity against annotated topics.
Large broad clusters contribute many pairs. In the full representative/0.4
trial, GRINCH's largest forward deadlock topic covers 193 canonical sources,
versus 136 in reverse; BIRCH covers 133/135. These differently sized clusters
help explain the stability change without proving either grouping is correct.

At radius 0.4, both algorithms assign all 778 eligible full-corpus threads.
At radius 0.6, GRINCH assigns 778 versus BIRCH's 766/768, but also makes more
topics (262/257 versus 237/240). Coverage alone is not evidence of better quality.

Fourteen preselected cases were inspected using the existing bounded source
excerpts and labels, with all 50 cases retained in a private comparison.
cBioPortal and runtime-investigation labels remain largely unchanged. Some
resource-lifetime and exact-workflow secondary assignments disappear or change;
others are added. Generic acknowledgment labels remain. This is not a blinded
full-source review or proof of major/secondary recall.

Whole benchmark-process peak RSS: BIRCH 1,092,492/1,100,216 KiB, GRINCH
1,109,884/1,111,200 KiB for full/reserved runs. This includes corpus loading and
labeling; it does not isolate tree memory. The two dense arrays per node alone
require about 8.2 MiB for 704 points at 384 dimensions, excluding allocations,
node metadata, corpus storage and models.

## Decision and remaining gates

Retain this exact opt-in baseline. Neither its theoretical guarantee nor higher
stability in selected settings establishes a default-switch case. Before
adoption, independently review full-source major/secondary assignments and
semantic parents, assess comparable topic granularity and alternative linkage,
and measure bounded approximations against this exact reference. Larger-corpus
growth, edit/delete/restart behavior, stable identities and publication remain
unimplemented. The comparison bead `mash-c0d.19.2.13` stays in progress.

Validation: all 90 focused topic tests, 561 regular tests, Clippy with warnings
denied, doctests, formatting and diff checks passed. Durable private artifacts
are under `/home/james/scratch/mash-artifacts/topic-grinch-20261001`.
