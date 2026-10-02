# Fixed-K online variational LDA control

Bead `mash-c0d.19.2.35.1`; exposed development diagnostics only. The prior
shortlist included [online LDA](https://papers.nips.cc/paper/3902-online-learning-for-latent-dirichlet-allocation.pdf),
a statistical count-mixture model with incremental updates and nonexclusive
memberships. It requires a fixed topic count and does not provide our requested
unknown-topic semantic hierarchy. This experiment uses the pinned
[scikit-learn 1.9.1 implementation](https://scikit-learn.org/stable/modules/generated/sklearn.decomposition.LatentDirichletAllocation.html),
not a reimplementation of its numerical algorithm.

## Frozen procedure

Before examining outcomes, record the protocol in Beads: original exposed 64
question sources, 629 source-only unigram columns and 4,764 raw token occurrences,
verified against graph capture
`a37376e6d9c34e24f24ec5eb9d7aad3664887193fb063b9a6be0362ffb43a0a6`.
No tags enter counts, vocabulary or seeds. The vocabulary remains transductive,
derived from previously exposed sources; this is not a future-unseen vocabulary
or a strictly streaming source-extraction experiment.

Use K=8/16/32, one/ten epochs, mini-batches of 16, alpha=eta=1/K,
learning decay .7, offset 10, fixed model seed 17, one worker, document E-step
limit 100 and tolerance .001. Set total_samples to the nonempty corpus size,
not the library's million-document default. Call partial_fit explicitly;
max_iter does not determine its number of passes. Repeat the original source
order, reverse and shuffle seed 17: 18 fits. These orders are not timestamps.
Ten epochs revisit documents and are an initial-index comparator, not an
append-only promise. Skip empty rows in training and force their memberships
to abstain rather than using a prior-only posterior (none here were empty).

Record each source's first-arrival posterior after its first batch and its
final posterior after all updates. Use final scores for the original cutoffs
.10/.20/.30, relative margin .5 and budgets 3/8: 108 settings. Require .70
precision with at least 20 predicted links across all three orders, no fallback.
Do not consult old reserved outcomes. Labels are top-eight topic-word
probabilities with lexical ties and exact source-support IDs, without an LLM
or additional encoder calls. Labels are descriptors, not independently judged
semantic names. No structural ancestry is inferred.

## Results

No setting passes even individually. Best descriptive precision among budget-3
settings emitting at least 20 links:

| Passes | Configuration | TP / FP | Precision | Recall | First-arrival tags changed |
| --- | --- | --- | --- | --- | --- |
| 1 | K16, .30, reverse | 30 / 299 | .0912 | .3093 | 4 sources |
| 10 | K32, .20, forward | 40 / 377 | .0959 | .4124 | 14 sources |

These are post-hoc descriptions, not selected winners. Both assign all 64
sources (77/81 emitted memberships). First-arrival versus final membership
sets change for 1–8 sources across one-pass settings and 8–18 across ten-pass
settings. Topic indices are compared only within the same fitted model.
Across orders, compare pair predictions rather than incidental topic numbers:
431–817 of the 2,016 decisions differ for matched policies. Thus fixed indices
do not imply stable topic meaning or stable historical assignments.

Some K16 one-pass forward top words are `metadata, transaction, table, id,
install`; `class, string, info, type, id`; and `n, core, process, array, d`.
These source-supported descriptors illustrate mixed/general vocabulary, but
are not a human label-quality score. Small corpus size, count representation,
priors and local optimization may contribute; this experiment does not isolate
their causal effects or disprove online LDA on other representations/corpora.

First/repeat processes match every nontiming field, in 2.698/2.746 seconds after
imports. Per-fit fitting plus transforms range 38.0–109.8 ms; per-fit mini-batch
update p95 ranges 3.71–8.31 ms. These omit production extraction, persistence,
label maintenance and lifecycle, and do not prove the ≤10% ingestion gate.
No peak-RSS or production inference measurement is claimed.

## Validation and disposition

Four tests cover bounds/counts, complete arrival permutations, batching,
deterministic repeats, normalized posteriors, empty-source abstention and a
direct pinned-library replay. A separate outside-Git runner replays all 18
library fits, first/final scores, components, events, word labels/support and
217,728 scalar pair decisions. This checks wrapper/capture fidelity; using the
same library is not independent numerical-algorithm verification or Rust parity.
All 312 research tests pass.

Artifacts live in
`/home/james/scratch/mash-artifacts/topic-public-stackoverflow-20261002`.
Run `lda_questions.py --root ROOT --output FRESH_OUTSIDE_GIT_JSON` in the pinned
graph research environment; repeat with `--compare FIRST_JSON`.
Capture hashes:

- First: `9b6fae52558214924a50dcdbd51f0c7893b4de629f98c5affc5e09348cee2000`
- Repeat: `5251a5410d48286d7904471c86e57cea82a61f201f7ab9d83dae4194c4ccc1fb`

Do not promote this candidate or add it to Rust from these results. Independent
topic judgments, new groups, guided-search utility, semantic parents, stable
topic identity, edit/delete/restart semantics and complete ingestion remain
open gates. No production behavior or configuration is changed.
