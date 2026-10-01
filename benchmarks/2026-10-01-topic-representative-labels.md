# Representative-segment label scoring: reject blanket adoption

This offline Rust ablation changes the semantic vector used to score leaf-topic
labels. It does not change BIRCH discovery, thread assignment, source text,
stored embeddings, or semantic ancestor construction. Keep it opt-in; the
observed phrase regressions rule out making this version the default.

## Method

BERTopic's [representation methods](https://maartengr.github.io/BERTopic/getting_started/representation/representation.html)
motivate separating discovery geometry from representative-document label
scoring. This experiment adapts that idea to supporting user segments:

- Capture segment indices only for retained topic assignments.
- Rank matching segments by lexical cosine against the existing c-TF-IDF
  candidate weights, using a binary segment vocabulary and length normalization.
- Select at most four segments per leaf, with one per canonical source identity.
- Normalize the equal-weight mean of their existing dense vectors, then use it
  for the existing candidate cosine/ranking/diversity checks.
- Fall back to the original centroid when no positive lexical match exists.

Selection is deterministic. There are explicit limits of 262,144 captured
matches and 8,192 selected vectors. Selection validates before modifying names.
It introduces no additional embedding calls. Typed private reports contain
source identities, segment indices/ranges, lexical scores, and selected vectors.
Ancestor names continue to use their original centers in this leaf-only trial.

Enable with `MASH_TEST_TOPIC_REPRESENTATIVE_LABELS=1`. This flag is distinct from
`MASH_TEST_TOPIC_REPRESENTATIVES`, which selects a discovery input mode. Topics
remain an offline test prototype; this adds no production CLI or MCP tool.

## Protocol and verification

Frozen corpus: 225,267 messages in 789 threads (515 Claude Code, 274 Codex),
including 6,714 compatible user vectors. Corpus generation:
`ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`.

Compare centroid and representative scoring across full/reserved-source runs,
forward/reverse insertion, UserRepresentatives/UserAnchored input, and BIRCH
radii 0.4/0.6: 32 corpus reports, plus eight focused case-40 candidate reviews.
Hold the prior discourse masking, support-aware shortlist, singleton candidate
inference, conserved support, context enrichment, and visible-link handling
fixed. Controls reproduce the prior discourse experiment after removing timing
and new representation metadata.

The audit verifies identical memberships, assignment evidence, ancestry and
ancestor labels; unchanged candidate inference counts; canonical source quotas,
reserved-source exclusions, unit vectors and selected means; and candidate
score/probe arithmetic. Original discovery centroids are omitted from serialized
reports, so their alignment with chosen vectors was not independently recomputed
by the external audit. Rust tests cover capture of actual retained matching
indices, overlapping evidence, source deduplication, reserved sources, invalid
inputs, rollback, and numerical bounds.

Reserved runs exclude reserved sources from representative-vector selection.
Existing label c-TF-IDF statistics still use assigned threads: this is **not** a
fully held-out label-training evaluation.

## Results and decision

In the full forward UserRepresentatives/0.4 comparison, labels changed in 39 of
the existing 50 review cases. These are bounded source samples, not independent
full-thread annotations or a blinded quality pass.

| Observed topic | Centroid scoring | Representative scoring | Assessment |
|---|---|---|---|
| Runtime/deadlock investigation | `deadlock`, no `runtime` | `runtime`, `deadlock` | Useful subject recovery |
| Agent/tracing discussion | `agents`, `update` | `agents`, `tracing policy` | More specific |
| Option-selection acknowledgments | `good`, `sounds` | `x20` | Clear regression |
| Repository audits | `repository audit batch`, `repo audit` | `audit`, `batch`, `release lets`, `positive` | Lost specificity; filler remains |
| Test discussion | `tests` | `tests taking` | Fragment without clear benefit |

Case-40 `runtime` becomes selected in the forward full and reserved-source
reviews. Reverse-order variants exclude it from the shortlist, which this
scoring-only change cannot repair. This is not an order-stable recall fix.

Across configurations, 38–409 leaf topics use 87–762 selected vectors. Label
construction/selection adds a median 74.3 ms in these paired runs. Semantic
labeling times include cache and host contention effects; they are not evidence
of an inference speedup. Peak RSS was not measured in this experiment.

Retain the bounded experiment for comparison, with centroid scoring as the
default. Source-based major/secondary recall, phrase specificity, provider/order
stability and semantic parent quality remain required adoption gates. Automatic
coherence or selected attractive examples cannot establish them.

The next priority is the [GRINCH](https://arxiv.org/abs/2001.00076) versus BIRCH
comparison (`mash-c0d.19.2.13`), holding labels and vectors fixed. Evaluate online
hierarchy quality, insertion-order sensitivity, costs and lifecycle behavior;
assess overlapping tags and semantic ancestor validity separately. MMR and
contrastive phrase scoring remain separate label ablations (`mash-c0d.19.2.12`).

Validation: 85 focused topic tests and 556 regular tests passed; Clippy with
warnings denied, doctests, formatting and diff checks passed. Private corpus
reports, comparisons and audit scripts live outside Git under
`/home/james/scratch/mash-artifacts/topic-representative-labels-20261001`.
