# Frozen lexical background and contextual cosine fusion

Exploratory development experiment, 2026-10-01, Bead `mash-c0d.19.2.31`.
Follow the [multi-view tradeoff](2026-10-01-multiview-context.md) with lexical
evidence from exact current USER text. No production method is selected.

## Frozen setup

Reuse vector capture SHA256
`62eff98049bd418f359500c5c2d7b9c4bab2f879d48b639dcb3e6dd8e8f74b0c`
and bootstrap capture SHA256
`b31098e45cec9d521690e67fc395c90819ca7aad01dbc2b98672cb20dd587ec6`.
Same 32 selected complete-text groups and 206 original common model-support
anchors. No train/test shard is read; fit background statistics on other development
dialogues. This is an explicit development background fit, not untouched test data
or independent confirmation after earlier outcomes.

Before outputs, freeze `TfidfVectorizer`: Unicode word pattern
`(?u)\b\w\w+\b`, lowercase, unigrams/bigrams, minimum document frequency 2,
maximum vocabulary 8,192, smooth IDF, sublinear term frequency, L2 normalization,
float64 and no stopword list. Do not discard negation or fit weights from intent
annotations. Background uses USER turns from the 480 unselected groups, removes
exact selected USER strings found elsewhere, deduplicates remaining exact strings,
and orders them by source-text hash. Evaluation-only tokens cannot enter the fit.
Vocabulary/IDF and source hashes are captured; texts/annotations stay outside Git.

Lexical vectors use current original USER text only. Three fixed controls:
lexical cosine; `0.75 * current cosine + 0.25 * lexical cosine`; and
`0.75 * joined cosine + 0.25 * lexical cosine`. Fusion is equivalent to separate
normalized dense/sparse views; compute scalar scores without materializing a large
dense concatenation. Zero lexical vectors abstain from **every matched control**,
with original/matched support and source reasons reported rather than fallback.

Primary service/intent/within-service AP/AUC reuse the exact 1,000 archived
whole-dialogue resamples, same multiplicity-product pair weights and no same-source
pairs. Baseline full pair truth/scores and original metrics must reproduce before
adding lexical scores. Restricted-support baselines are recomputed if needed;
point domain scores are secondary. No new model initialization/inference, parameter
tuning, generative calls or annotation-dependent feature selection. Prior contextual
topic models combine contextual and lexical information
([Bianchi et al., 2021](https://aclanthology.org/2021.acl-short.96/)); these simple
TF-IDF/cosine controls do not implement their trained neural model.

## Reproduction

```bash
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-python-env/bin/python \
  research/topics/lexical_context.py \
  --root /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001 \
  --capture /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/context-controls-final-first.json \
  --analysis /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/intent-bootstrap-first.json \
  --output /home/james/scratch/mash-artifacts/topic-public-multiwoz-20261001/lexical-new.json
```

Add `--compare` with the first capture for a fresh repeat. Refuse overwrites and
require exact equality of every non-timing field: background hashes/exclusions,
vocabulary/IDF, sparse vectors/abstentions, all pair truth/scores, draws, every
replicate delta and policies/source/code/version pins. Active NumPy/scikit-learn
versions must match the archived analysis. Initial/incremental topic ingestion,
lexical basis evolution, memory, label quality, guided search and Rust parity
remain separate requirements.

## Results

The 480-group background has 3,607 USER turns. Remove six exact evaluation-text
occurrences and 48 background duplicates, leaving 3,553 fit strings. Vocabulary
has 3,783 features, with 64,511 nonzero background entries and 3,704 evaluation
entries. All 206 original common-support anchors have nonzero lexical features:
no abstentions, unchanged 12,341 cross-dialogue pairs and 3,448 within-service
pairs. Full baseline grids/metrics reproduce before scoring. Deduplicating exact
text does not remove paraphrases or template overlap; development/state proxies
still limit independence and interpretation.

| Control | Service AP | Intent AP | Within-service AP | Within-service AUC |
| --- | ---: | ---: | ---: | ---: |
| Current | 0.464235 | 0.399102 | 0.802038 | 0.583550 |
| Mean | 0.534925 | 0.415504 | 0.752176 | 0.512996 |
| Joined | 0.555472 | 0.417893 | 0.737828 | 0.490344 |
| Lexical | 0.363991 | 0.317964 | 0.821366 | 0.643287 |
| 75% current + 25% lexical | 0.462814 | 0.400921 | 0.806329 | 0.591405 |
| 75% joined + 25% lexical | 0.557339 | 0.432407 | 0.752233 | 0.506874 |

Empirical paired AP changes versus current text, 2.5/97.5 percentiles:

| Candidate | Service | Intent | Within shared service |
| --- | --- | --- | --- |
| Lexical | [-0.157502, -0.049807] | [-0.143692, -0.028032] | [-0.009001, 0.043446] |
| Current + lexical | [-0.006951, 0.004481] | [-0.005521, 0.009119] | [0.001322, 0.007438] |
| Joined + lexical | [0.043318, 0.147113] | [-0.014461, 0.087375] | [-0.079620, -0.019214] |

Within-service paired AUC ranges are lexical [0.004073, 0.112089], current fusion
[0.004387, 0.011686] and joined fusion [-0.130171, -0.008225]. All 1,000 resamples
retain both classes in each primary slice. These are uncalibrated empirical ranges,
not formal significance, noninferiority or independent confirmation. Within-service
positive prevalence remains 72.16%; high AP does not imply strong discrimination.

Point domain AP, on the unchanged previous per-domain supports:

| Domain | Current | Joined | Lexical | Current fusion | Joined fusion |
| --- | ---: | ---: | ---: | ---: | ---: |
| Attraction | 0.223221 | 0.226880 | 0.216563 | 0.228449 | 0.239835 |
| Hotel | 0.281696 | 0.381683 | 0.226208 | 0.282862 | 0.384009 |
| Restaurant | 0.155075 | 0.232215 | 0.122134 | 0.156552 | 0.226150 |
| Train | 0.489262 | 0.566958 | 0.253252 | 0.479219 | 0.567582 |

Lexical evidence improves within-service discrimination, especially compared with
joined context, but lexical-only substantially weakens broad service separation.
Current fusion yields a small finer-intent gain with a small broad-AP decline;
joined fusion partially recovers finer intent but remains well below current text.
Effects differ across domains. No fixed control clearly improves both broad and
fine point scores, and no tolerance is invented after seeing the results.

## Decision and limits

Keep lexical evidence in the candidate set; do not select a production topic
method from these exposed task-state proxies. Test whether separate broad and
fine discovery views preserve both types of evidence, then assess actual cluster
memberships, labels and guided search rather than only pair ranking. Require
independent coding-session aspect/provider evidence and a reserved evaluation
before selection. Online vocabulary/IDF changes alter geometry; version/freeze
that basis or define compatible updates before inserting into a persistent tree.
No full initial/incremental ingestion, RSS/p95, semantic-parent, overlapping-tag
or Python/Rust parity gate is established.

All 181 isolated research tests pass: group/text exclusion, exact background
deduplication, evaluation-only vocabulary rejection, preserved negation,
zero lexical rows, L2 norms, fixed fusion arithmetic, malformed inputs and emitted
policy JSON round-trips. The initial repeat rejected a tuple/list policy mismatch;
the output now explicitly emits an array. Initial captures/logs are preserved and
fresh final paths are used, rather than rewriting an old code-pinned run.

Two fresh final captures agree exactly on every non-timing field:

- `lexical-final-first.json`, SHA256
  `21e9b729899037ac8f8680ffcc5b2515f2f76fb3cad2fdfcd2e2723744ea4b48`.
- `lexical-final-second.json`, SHA256
  `8046f743d81486a34f3aeeaf2d69537923a723f7a330d33f9331c199f35451ba`.

The original `lexical-first.json` has SHA256
`85608d4295bf72f31abe6965ca6b201cf347097a90061ca9ea22304b1931f626`;
all decoded non-code/non-timing fields agree with the final first capture. The
failed repeat wrote a diagnostic log, not an accepted capture.

NumPy 2.4.6/scikit-learn 1.9.1. Background selection/hash/fit and evaluation
transforms take 281.79/223.51 ms; sparse Gram construction and candidate scoring
take 68.93/44.03 ms. Complete analysis after imports takes 90.31/87.00 seconds,
including validation and 18,000 resampled method evaluations plus point metrics,
excluding capture serialization. These descriptive offline costs exclude original
embedding inference, clustering, labels and persistence, and establish no
production ingestion, memory or search-latency gate. Zero new model calls.
