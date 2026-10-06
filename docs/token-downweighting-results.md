# Token downweighting: completed development screen

The four fixed Optuna trials completed. All three weighting policies have small
positive point estimates versus their matched singleton control, but every paired
bootstrap interval includes zero and none meets the predefined quality gates.
Keep the frozen experimental baseline and production embeddings unchanged.
This is an exploratory result on reused, agent-authored development labels.

## Setup and safeguards

Follow [the frozen protocol](token-downweighting-protocol.md): 160 development
histories from the 200-history fleet cohort, installed quantized MiniLM, unchanged
input text/attention, and identical hidden states shared across pooling policies.
Rebuild both turn views and both roles using singleton inference; preserve
structural chunking, equal-role pooled views, uniform proximity smoothing and
BIRCH settings. Frequency statistics use only original messages in the same
thread, counting eligible words once per source message rather than per window.
No LLM, model download, production indexing, or confirmation run occurred.

A pre-score edge-case review caught curly-apostrophe negations being split into
pieces. Two incomplete build attempts were retained unscored while protection was
corrected to cover both apostrophe forms and contracted `n't`. The clean corrected
build is the sole source of scored matrices. Its code and protocol are pinned in
the artifact contract. Fenced/inline code, structured identifiers, negations and
special tokens retain full weight; the matcher is a conservative heuristic,
not a semantic entity recognizer.

The artifact audit checks all 160 histories, 12 real uniform-pooling parity probes,
24 fresh-versus-stored vector comparisons, 48 reordered/unrelated-inference
comparisons, and eight membership-isolation checks. All pass. The full Python
suite has 141 passing tests, with Ruff, formatting and `ty` checks passing.

## Turn quality

The unchanged main manifest has 1,051 pairs: 617 related, 313 unrelated and 121
ambiguous. Ambiguous pairs do not enter precision/recall/F1. Long histories use
the existing eight-turn boundary. Trial selection never opens confirmation.

| Policy | Precision | Recall | F1 | Long precision | Long recall | Long F1 |
|---|---:|---:|---:|---:|---:|---:|
| Uniform matched control | .8421 | .8558 | .8489 | .8134 | .7357 | .7726 |
| Fixed function words | .8491 | .8574 | .8532 | .8262 | .7420 | .7819 |
| Thread-local IDF | .8462 | .8558 | .8509 | .8175 | .7420 | .7780 |
| Thread-local SIF-style | .8429 | .8606 | .8516 | .8123 | .7580 | .7842 |

| Policy vs uniform | Delta F1 | Paired 95% interval | Long delta F1 | Long interval | Changed pair predictions |
|---|---:|---|---:|---|---:|
| Function | +.004351 | [-.002080, +.010709] | +.009304 | [-.003149, +.023189] | 25 |
| IDF | +.002052 | [-.005041, +.008978] | +.005388 | [-.009935, +.020985] | 29 |
| SIF-style | +.002769 | [-.004570, +.010275] | +.011609 | [-.002656, +.027630] | 37 |

Intervals use 2,000 paired thread resamples, include negative-only/unjudged
histories, are unadjusted for multiple comparisons, and are exploratory rather
than confirmatory null-hypothesis tests. Changed predictions include ambiguous
references. None passes overall and long-history precision >= .85 with positive
lower delta-F1 bounds. The highest Optuna objective is not sufficient for adoption.

Uniform TP/FP/FN/TN is 528/99/89/214; function is 529/94/88/219;
IDF is 528/96/89/217; SIF-style is 531/99/86/214. On long histories these are
231/53/83/190, 233/49/81/194, 233/52/81/191, and 238/55/76/188.
SIF-style increases long recall by .022293 but also adds two long false positives.
Function weighting removes five overall false positives with one net new true
positive. These are small directional signals, not validated quality wins.

The historical frozen baseline is .854618 F1. This fresh uniform control is
.005743 lower because both roles and both views are rebuilt with singleton
inference; previous studies retained some historical batched parent vectors.
That regime change is shared by all four cells and must not be attributed to
weighting. Historical-difference intervals also include zero for every cell.

## Coverage and runtime

The clean build infers 24,057 distinct per-history inputs, representing 3,400,461
token positions. Function weighting touches 421,741 positions; IDF/SIF touch
1,340,793 each. Among 23,346 final pooled/chunk points, respectively 21,245,
21,914 and 21,914 vectors differ from uniform. Mean cosine to uniform is .998064,
.997248 and .994053; minima are .886002, .940958 and .768777.
Coverage is therefore substantially broader than the greeting trial's 18 tokens.

The clean four-policy build takes 6:52.66 wall time, observed per-history p95
9.86 seconds, peak RSS 1,057,512 KiB. Scoring takes 1:03.36, peak RSS 1,078,708
KiB; per-policy grouping p95 is .126-.153 seconds/history. Earlier discarded
builds are excluded from clean-run timing. These are shared-host observations,
including loaded Python corpus/report/model state, not isolated production
throughput or worker-memory measurements. One forward pass serves all four
pooling policies; no hidden-state matrices are persisted.

## Message retrieval pilot

Use raw documents and queries from the three existing frozen cleanup corpora,
with uniform versus fixed function-word weights and no cross-document statistics.
All inputs fit the budget; no truncation occurs. Corpus hashes and source-finding
rankings are saved independently. Neither corpus cleanup nor turn clustering is
applied to these message vectors.

| Corpus | Queries | First-rank targets, uniform -> function | MRR | Recall@5 | Changed top-five orderings |
|---|---:|---|---:|---:|---:|
| Light synthetic | 21 | 20 -> 20 | .9643 -> .9643 | 1 -> 1 | 3 |
| Heavy synthetic | 21 | 19 -> 19 | .9444 -> .9444 | 1 -> 1 | 7 |
| Reviewed requests | 17 | 17 -> 17 | 1 -> 1 | 1 -> 1 | 10 |

Function weighting changes 24/25/114 document vectors and touches 92/132/599
token positions across documents and queries, respectively. No target rank
improves or regresses. The real-request set has a ceiling and only source-finding
labels, so this is primarily a regression pilot. It does not establish quality
for full message search, hybrid fusion or reranking. Its matched Python controls
are not interchangeable with earlier Rust pilot absolute scores.

## Interpretation and reproducibility

Downweighting is active but perturbations are usually small, and only 25-37 judged
turn predictions change. Contextual states already mix information across words;
post-encoder weighting does not erase a word's influence from all states. Also,
within-thread frequency measures repetition rather than irrelevance, so broad
IDF/SIF may suppress a genuine recurring subject. These are plausible explanations,
not isolated causal findings. This screen does not test fixed external unigram
statistics, component removal, attention masking or learned pooling.

Retain the baseline. Do not add trials or relax gates after inspecting these
results. A broader independently judged message-retrieval study would be needed
before adopting a production projection; neither token erasure nor attention
modification follows from this screen.

Private artifacts are under
`/home/james/scratch/mash-artifacts/token-downweighting-20261006`, with scored
matrices in `artifacts-corrected`, four reports/Optuna SQLite in `results`, the
audit, corrected message pilot, protocol snapshots and run logs. The corrected
build contract SHA256 is
`f5c3ccafdabb76c965ed2cc1d23075b3caec526c1fbb90901dc52b0cfc352c58`.
Reports pin annotation, source, matrix and scoring-code hashes. The paper records
procedures, correction, outcomes and limitations.

Commands (from `experiments/message_clustering`, with private paths substituted):

```sh
uv run python build_token_weighting.py --parent ROOT --protocol PROTOCOL \
  --model INSTALLED_SNAPSHOT --output ARTIFACTS
uv run python audit_token_weighting.py --parent ROOT --artifacts ARTIFACTS \
  --model INSTALLED_SNAPSHOT --output AUDIT_JSON
uv run python run_token_weighting.py --parent ROOT --artifacts ARTIFACTS \
  --output RESULTS
uv run python message_weighting_pilot.py --corpus LIGHT --corpus HEAVY \
  --corpus REVIEWED --model INSTALLED_SNAPSHOT --output MESSAGE_RESULTS
```
