# Python topic geometry controls

These twelve runs establish reproducible geometry controls, not a selected topic
method or evidence of semantic quality. They follow the updated research → Python
experiments → selection → Rust workflow. The
[lab protocol](../research/topics/README.md) defines the next quality gates.

## Inputs and setup

Frozen snapshot `search-performance-d62a597/index.sqlite3`; existing source-group
partition from `topic-option-context-clusters-20261001/control-birch-full/`
`UserRepresentatives-0.4-false.json`. Generation:
`ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`.
The partition report supplies identities only, not cleaned vectors.

6,714 stored user vectors; 242 duplicate searchable user texts removed within
threads; 121 canonical duplicate threads removed. Result: 664 independent threads
from Codex and Claude Code, including 126 reserved source groups. Held-out
discovery trains on 538 threads. Inputs are raw normalized user-vector thread
means. No inference, denoising, episodes or overlapping assignments.

Python 3.11.15, NumPy 2.4.6, scikit-learn 1.9.1 on Linux Xenia; dependencies
locked in `research/topics/uv.lock`. Each run used a fresh process and one numeric
CPU thread. Three methods × two discovery scopes × forward/reverse order.
Private outputs: `mash-artifacts/topic-python-baselines-20261001`.

## Results

Forward-run timing and counts below. Order stability is forward/reverse Jaccard
of co-clustered training pairs, excluding noise; cluster IDs themselves need not
match. Largest cluster and noise counts qualify that score: stability can come
from an overly broad cluster or abstention.

| Method | Discovery | Clusters | Noise | Largest cluster | Fit ms | Labels ms | Peak RSS MiB | Order Jaccard |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| BIRCH | Full | 166 | 0 | 104 | 30.8 | 1326.2 | 231.5 | 0.701 |
| BIRCH | Held-out | 141 | 0 | 101 | 34.5 | 1202.3 | 228.7 | 0.933 |
| HDBSCAN | Full | 40 | 225 | 36 | 192.7 | 896.3 | 237.3 | 0.983 |
| HDBSCAN | Held-out | 31 | 205 | 34 | 115.6 | 868.6 | 236.6 | 0.953 |
| Average-link | Full | 112 | 0 | 191 | 62.9 | 1254.7 | 231.0 | 1.000 |
| Average-link | Held-out | 100 | 0 | 155 | 48.7 | 1232.3 | 229.6 | 1.000 |

The common held-out nearest-centroid diagnostic assigns 119/126 reserved threads
for BIRCH, 103/126 for HDBSCAN and 118/126 for average linkage at cosine ≥0.7.
This is coverage, not correctness or native HDBSCAN prediction. It can assign a
thread rejected as noise by the fit procedure. Full-discovery reserved coverage
is contaminated by training exposure and is not a generalization result.

## Interpretation and limitations

BIRCH's cheap fit and order sensitivity both remain relevant. HDBSCAN's smaller
clusters and substantial abstention require checking rare-topic recall. Average
linkage's stability does not establish whether its 191-member largest cluster is
useful. c-TF-IDF labeling dominates fit time in these controls, but this is a
single-run observation on one snapshot, not a latency distribution or an optimized
streaming implementation. Peak RSS includes imports/loading; fit/label timers do
not include either. No end-to-end ingestion measurement is implied.

These controls cannot adjudicate BIRCH versus GRINCH, which was not run here, or
select production defaults. Parameters were fixed for initial comparison, not
tuned to equivalent quality. Offline HDBSCAN/average linkage do not satisfy online
requirements by themselves. Whole-thread averaging can hide multiple subjects.

Next: freeze independent source annotations for episode boundaries and major/
secondary subjects, then compare dialogue units and context selection. Hold units
fixed while comparing discovery; hold memberships fixed while comparing labels.
Review semantic parents and ancestor inheritance separately. Production embedding
inputs and topic behavior remain unchanged.

## Validation

All twelve reports shared the same input/reference fingerprints, generation and
cohort. Independent output checks verified reserved groups excluded from held-out
training, disjoint fit memberships, noise/member counts, valid centroid IDs and
finite cosine bounds. Four synthetic input tests passed for read-only behavior,
canonical duplicates/partition preservation, generation mismatch, malformed
dimension and missing group failure. This checks experiment integrity, not topic
quality. Rust application files were not changed.
