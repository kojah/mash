# Programming-question source inventory and frozen-cut transfer

The dialogue-calibrated cuts do not transfer usefully to these programming titles.
Centroid `.6` leaves every question separate; average `.4` recovers only 3 of 51
shared-tag pairs in the reserved source sample. Raw title cosine retains ranking
signal, so this rejects threshold transfer, not embeddings or online topic
modeling generally. No production method is selected.

Tracked by `mash-c0d.19.2.33`. Python-only, no generative calls, paid APIs, model
downloads, application behavior changes or Rust parity claims.

## Source and provenance

Stack Overflow defines tags as question-topic descriptors and permits multiple
tags per question. They offer externally supplied programming metadata, unlike
the existing single-assistant coding-history reviews. They are incomplete,
editable language/library/task proxies, not full agent-session primary/secondary
topic gold. Absence of a shared tag does not establish semantic unrelatedness.
The [official tagging guide](https://stackoverflow.com/help/tagging) documents
their intended use. Prior multilabel title/text tag prediction includes
[Mehta and Sodhani, 2015](https://arxiv.org/abs/1512.04092); we do not reproduce
their supervised classifier or use tags as training features.

Preregistered in Beads before source inspection: two unauthenticated Stack
Exchange API 2.3 `/questions` requests, site `stackoverflow`, `sort=creation`,
`order=asc`, UTC June 2024 inclusive `[1717200000,1719791999]`, pages 1 and 2,
`pagesize=100`, built-in `withbody`. No tag, vote, answer or model-based filters;
no replacement sampling. See the official
[questions endpoint](https://api.stackexchange.com/docs/questions) and
[filter documentation](https://api.stackexchange.com/docs/filters).

Both requests succeeded, with 100 records each, `has_more=true`, quota
299/298 of 300 and no backoff. Captured at October 1, 2026 22:11:57/22:12:06 UTC
(October 2 in Berlin). Raw responses and provenance/attribution stay outside Git
under `topic-public-stackoverflow-20261002`:

- Page 1, 313,955 bytes: `c4b9b5201844097667671199caec22b8437c614b7b1a9e46e440a1bfb629a3e6`.
- Page 2, 355,332 bytes: `076b0660cf9547154b4c9c68298ea3354bc27632ff720059a68aee3d571e5a18`.

This is the first 200 surviving API-visible questions in the interval, all posted
June 1 between 00:00:54 and 08:54:40 UTC. It is not a random June sample or a
historical snapshot of the original posts: 111 have edit metadata, 17 closure
metadata and 105 are answered. Tags/text may have changed since publication;
generative assistance and encoder pretraining overlap are unknown. No answers,
comments, provider histories or complete dialogue context are collected. Mutable
API requests may not reproduce these exact bytes; immutable captures are needed
for this experiment, and a public dataset release has not been prepared.

There are 200 unique content groups, no exact duplicates or alias-tag conflicts.
Raw attribution links and owner records remain in captures. `content_license`
is supplied as `CC BY-SA 4.0` for 183 records and absent for 17; absence is
explicit `None`/unknown, never an invented license or sampling exclusion.
Tag counts are 17/42/56/45/40 questions with 1/2/3/4/5 tags respectively.

`stackoverflow_source.py` validates source hashes before decoding, exact page
counts, creation range/order, literal IDs/booleans/quota, distinct bounded tags,
title/body strings and attribution links. Unknown unused metadata does not enter
model input. `QuestionInput` and `TagReference` are separate frozen dataclasses;
grouping uses only decoded title and original body HTML. HTML is never executed.

Hash-rank groups with seed `mash-topic-stackoverflow-v1-20261002`: first 64
calibration-source, next 64 reserved-source, remaining 72 background. Selection
uses no tags, scores or model predictions. Insufficient groups fail without
resampling. The background is retained but unused in this title-only experiment.

| Source phase | Questions | Pairs | Shared-tag pairs | No-shared-tag pairs | All-connected precision |
| --- | ---: | ---: | ---: | ---: | ---: |
| Calibration-source | 64 | 2016 | 97 | 1919 | .0481 |
| Reserved-source | 64 | 2016 | 51 | 1965 | .0253 |
| Background | 72 | 2556 | 159 | 2397 | .0622 |

These natural prevalences are much lower than the preceding dialogue-thread
intent union (.7917). Do not balance/select convenient negatives or treat tag
non-overlap as independent human judgments. No tag synonyms or language tags
are removed. Inventory repeats are byte-identical SHA256
`2e6ec9385886941d38cde9edb938de5e9b6d12c4c337766c011b57e3ad2977d6`.

## Frozen transfer experiment

Preregistered after inventory but before model outcomes. `question_transfer.py`
requires the original precision-calibration artifact SHA256
`c0dcfd2f4db61702af8692db98ec2a61763f0ab11f4755962944f6833f17140f`.
It verifies its intent winners, algorithm/encoder code pins and dependency
versions before inference. Transfer exactly current-average `.4` and
current-centroid `.6`; no new cuts, tuning, outcome-based method choice or
supervised tag learning.

Input is one question title with a single `html.unescape` of API-safe text.
Original body HTML is retained but unused. Tags/score/owner/license/answers never
enter embeddings. Installed pinned MiniLM, singleton one-thread CPU inference,
attended-token mean pooling, symmetric signed-byte rounding and normalized
float64 directions are unchanged. Reject 2–512-token window violations without
padding/truncation; exclude exact calibration-title inputs from reserved fitting.
All 128 titles are supported, have distinct cross-phase hashes and need 128
unique calls; maximum 48 tokens. Each question receives one cut membership:
this does not test overlapping unit topics, body context or semantic inheritance.

Fit both cuts on both 64-question phases in ascending question-ID, reverse and
seed-17 shuffled orders: 12 trees. Report every result, raw cosine pair AP/AUC,
all-connected/singleton baselines, support, clusters and canonical partition ARI.
Both source phases are transfer outcomes; neither selects a new setting. They
are now exposed development results for future methodology.

| Phase | Setting | Clusters | Singletons | TP | FP | FN | TN | Precision | Recall | F1 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Calibration-source | Average `.4` | 59 | 54 | 1 | 4 | 96 | 1915 | .2000 | .0103 | .0196 |
| Calibration-source | Centroid `.6` | 64 | 64 | 0 | 0 | 97 | 1919 | undefined | .0000 | .0000 |
| Reserved-source | Average `.4` | 61 | 58 | 3 | 0 | 48 | 1965 | 1.0000 | .0588 | .1111 |
| Reserved-source | Centroid `.6` | 64 | 64 | 0 | 0 | 51 | 1965 | undefined | .0000 | .0000 |

Every tested order has the same partitions/counts and ARI 1.0. This is mostly
singleton stability, not robust useful topics. Average has only five/three
predicted pair links, below the original calibration criterion's minimum 20;
reserved precision 1.0 rests on just three matches. Centroid predicts none.
Specificity is .9979/1.0 for average and 1.0 for centroid, but high specificity
with negligible recall does not establish quality. All-connected F1 is .0918
calibration-source and .0493 reserved-source; singleton recall/F1 zero.

| Raw title cosine | AP | ROC AUC | Positive prevalence |
| --- | ---: | ---: | ---: |
| Calibration-source | .1166 | .6429 | .0481 |
| Reserved-source | .1898 | .7415 | .0253 |

Ranking retains signal on the proxy. AP and partition F1 have different meanings
and must not be compared as equivalent scores. Pairs share questions, so they
are dependent; no significance, uncertainty guarantee or guided-search gain is
claimed. Title-only inputs, sparse source support and mismatched tag granularity
limit conclusions. Need source-grounded body/episode units and calibration on
the actual target representation, followed by a newly reserved evaluation.

A descriptive check of saved leaf similarities helps explain fragmentation:
shared-tag median cosine is .0704 calibration-source/.1156 reserved-source,
with 90th percentiles .2375/.3236. Only 2/3 positive pairs and 7/0 negative pairs
reach `.4`; no leaf pair reaches `.6` (maxima .5200/.4724). Partition merges
need not equal independent leaf-threshold edges: calibration average retains
one positive and four negative pair links, rather than every above-cut edge.
Corpus, input unit and tag granularity change together, so these distributions
cannot isolate a single cause or justify lowering cuts after reserved exposure.

## Cost and verification

Two full runs take .7915/.7640 seconds, with 189.67/175.22 ms inference and
286.39/263.28 ms across all 12 tree fits. Totals include source/contract loading,
model construction, pair scoring and tree evaluation; output serialization is
outside the timer. No RSS audit, persistence, edits/deletes, full ingestion or
≤10% ingestion overhead gate is measured.

All non-timing output fields reproduce exactly, including 128 vectors,
source/model/code pins, every partition and operation count. Separate
recomputation verifies 200 disjoint source groups, title/source hashes, unit
vector norms, all 4032 pair truth/dot scores and 12 partition confusion counts,
and independently calculates tie-aware AP/AUC. Tests verify tag changes cannot
change inputs/group selection, missing-license handling, malformed known fields,
source-hash/pagination failures, sparse baselines and absent proxy classes.
The full research suite passes 214 tests; no Rust application changes.

- Transfer first SHA256: `a77d5efeb2ce4a816419bf6ed3efc1c5bb0dfcde26988c91c6280293db619333`.
- Transfer repeat SHA256: `100eb33027d221d414fb14be6b23a6d206a9affea5d5a2784effdf01c728c8fe`.

Keep `mash-c0d.19.2.33` open for genuine granular thread discrimination,
independent coding/provider judgments, overlapping tags, labels, ancestors,
guided search, lifecycle, performance and Python/Rust parity gates.
