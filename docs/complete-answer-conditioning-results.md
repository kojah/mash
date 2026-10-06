# Complete-answer prompt conditioning: results

The two registered Optuna trials completed and are recorded in local MLflow.
Adding bounded prompt context to every complete-answer chunk has a small positive
F1 point estimate, but both paired intervals include zero. Retain the frozen
complete-answer-v1 baseline. No production or confirmation changes follow.

## Design

Follow [the prospective protocol](complete-answer-conditioning-protocol.md).
The same 160 development histories contain 3,396 turns and 1,051 original blind
judgments: 930 decisive and 121 ambiguous. Reuse the frozen pooled/user vectors;
restore all 15,975 own-turn assistant segments in both arms. The treatment prefixes
every assistant chunk with up to 80 current-prompt tokens, then pools only the
answer-excerpt token states. Structural overflow splits retain original body text.
Keep MiniLM, singleton inference, original proximity smoothing, BIRCH radius .60,
cutoff .75 and union fixed. The comparison jointly changes prompt attention and
answer-only pooling; it does not isolate those mechanisms. No new tuning, labels,
denoising, external statistics or model download. Confirmation remains sealed.

## Quality

| Scope / arm | TP | FP | FN | TN | Precision | Recall | F1 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Overall, frozen baseline | 543 | 102 | 74 | 211 | 84.19% | 88.01% | 86.05% |
| Overall, first80 conditioning | 547 | 105 | 70 | 208 | 83.90% | 88.65% | 86.21% |
| Long, frozen baseline | 246 | 58 | 68 | 185 | 80.92% | 78.34% | 79.61% |
| Long, first80 conditioning | 251 | 62 | 63 | 181 | 80.19% | 79.94% | 80.06% |

Long means at least eight turns. Overall F1 gain is **+0.16 percentage points**,
paired 95% thread-bootstrap interval **[-0.61, +1.02]**. Long F1 gain is **+0.45
points**, interval **[-1.14, +2.15]**. Use 2,000 paired resamples. Neither interval
establishes a gain, and neither arm satisfies the existing precision gate.
This exploratory screen reuses agent-authored development labels; it is not a
held-out result or evidence of equivalence.

Long related pairs recover nine misses and introduce four new misses; long false
positives increase by four. Across the whole benchmark, there are four net new
true positives and three new false positives: only one fewer classification error.
Twenty-nine predictions change, including ambiguous judgments excluded from
quality metrics. Among 144 long related pairs with position gap >=4, misses fall
57 to 53, recall 60.42% to 63.19%. No decisive pair has an omitted evidence quote
in either arm. The earlier incomplete-answer experiment's small effect persists
in this specific complete-answer comparison; it does not rule out other context
representations.

## Construction and verification

All 160 original control prediction maps/counts reproduce exactly. Every pooled
and user-chunk vector remains bit-identical. Source, role, prompt ownership and
non-whitespace answer-coverage checks pass across all histories. Candidate matrix
hashes, owners, dimensions, finite unit norms, and 160 conditioning sidecar hashes
pass. Twelve fresh selected-pooling encoder probes from four histories reproduce
saved vectors exactly. The initial standalone preflight accidentally retained the
tokenizer file's default padding; it failed before inference. Rerunning with the
encoder's explicit no-padding/no-truncation settings passes, without algorithm or
protocol changes. Both logs remain private.

| Construction quantity | Frozen baseline | Treatment |
|---|---:|---:|
| Assistant segments represented | 15,975 | 15,975 |
| User chunks | 4,251 | 4,251 |
| Answer chunks | 29,854 | 29,899 |
| Total structural points | 34,105 | 34,150 |
| Total input tokens, pooled + structural | 5,400,740 | 6,479,444 |
| Summed clustering fit time | 7.85 s | 7.30 s |

All 29,899 answer chunks receive conditioning. The extra prefix induces 45
additional chunks; 249 prompts exceed 80 tokens. Construction infers 14,096 new
distinct selected-pooling inputs, reusing only matching singleton cache entries
from prior experiments. Total builder wall time is **5:57.13**, CPU user/system
time 178.29/2.21 seconds, peak RSS 1,124,972 KiB. Summed per-history construction
is 326.23 seconds. Scoring, audits, bootstrap, initial MLflow database setup and
logging take **1:54.16**, peak RSS 1,195,064 KiB. Shared-host filesystem contention
returns during the run. These cache-assisted measurements do not isolate inference
throughput or the causal speedup from larger transactions.

## Tracking and decision

Use MLflow 3.16.1 with local SQLite at
`/home/james/scratch/mash-artifacts/mlflow/tracking.sqlite` and a private local
artifact root. Experiment: `complete-answer-conditioning-v1`.

| Arm | MLflow run |
|---|---|
| Baseline | `2b64292b8a1e4233b43a5f7e1748d887` |
| first80 | `115c48104b114ddbbea1188087f2579a` |

Both runs are `FINISHED`; parameters, aggregate metrics and typed summary artifacts
were read back and verified. Optuna trial records link to their MLflow run IDs.
The baseline/treatment log 25/31 metrics and 19 parameters each. MLflow contains
the protocol and aggregate summaries, without conversation text, vectors,
per-thread predictions or annotation payloads. Detailed data, code contracts,
matrix/source/audit hashes and logs remain in private research files under
`/home/james/scratch/mash-artifacts/complete-answer-conditioning-20261006`.

The full Python suite passes 150 tests, including real local MLflow persistence,
artifact round-trip and failed-run status. Ruff, formatting and ty pass.
Procedures/results are recorded in the LaTeX paper. The frozen baseline stays
unchanged; no selected80 arm or CPU-concurrency change is added to this study.
