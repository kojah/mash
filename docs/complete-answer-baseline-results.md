# Complete-answer baseline rerun

Restoring all assistant segments improves long-thread recall, but does not solve
the remaining quality gap. Keep the source-complete representation as the
corrected experimental baseline. No production message-search behavior changes.

## Controlled comparison

Follow [the frozen protocol](complete-answer-baseline-protocol.md): 160 development
histories, 3,396 turns, 1,051 original blind judgments (930 decisive, 121 ambiguous).
Long histories have at least eight turns; 62 histories contain 557 decisive pairs.
Confirmation remains sealed. Both variants use the same installed quantized
MiniLM with singleton inference, equal-role pooled and heading-aware chunk views,
smoothing alpha .10/window four, BIRCH radius .60/cutoff .75, and membership union.
Only selection of the current turn's assistant segments changes. Bounded ancestor
context remains unchanged; no denoising or threshold tuning is added.

The `last` control reproduces all preceding singleton uniform matrices and owners
bit-for-bit across 160 histories. Every pair prediction and confusion count also
matches the preceding token-downweighting control. This matched control differs
slightly from older partly batched results; do not compare the new result against
those as if the only difference were answer coverage.

## Quality

| Scope | TP | FP | FN | TN | Precision | Recall | F1 |
|---|---:|---:|---:|---:|---:|---:|---:|
| All pairs, last segment | 528 | 99 | 89 | 214 | 84.21% | 85.58% | 84.89% |
| All pairs, all segments | 543 | 102 | 74 | 211 | 84.19% | 88.01% | 86.05% |
| Long histories, last segment | 231 | 53 | 83 | 190 | 81.34% | 73.57% | 77.26% |
| Long histories, all segments | 246 | 58 | 68 | 185 | 80.92% | 78.34% | 79.61% |

With 2,000 paired thread-bootstrap resamples, the overall F1 gain is 1.17
percentage points, 95% interval **[-0.12, +2.65]**; it does not establish an overall
gain. Long-history F1 improves 2.35 points, interval **[+0.14, +4.99]**. These
intervals describe this purposively sampled development benchmark with provisional
agent-authored labels, rather than an independently held-out production estimate.

Of long related pairs, 20 earlier misses are recovered and five successes become
misses: a net recovery of 15/314 pairs, or 4.78 recall points. For the 144 long
related pairs separated by at least four turn positions, misses fall from 67 to
57; recall rises from 53.47% to 60.42%. Across all judgments, 67 predictions change
(including ambiguous judgments, which stay excluded from quality counts).
Long false positives increase by five, while short false positives decrease by
two. Precision therefore remains below the registered 85% acceptance threshold,
both overall and on long histories. Neither variant passes the absolute gate.

## Coverage, cost and verification

| Quantity | Last segment | All segments |
|---|---:|---:|
| Assistant source segments represented | 3,350 | 15,975 |
| Decisive pairs with an omitted evidence quote | 312 | 0 |
| Unknown quote locations | 0 | 0 |
| Pooled-view input windows | 8,231 | 9,534 |
| Structural chunk points | 19,950 | 34,105 |
| Summed input tokens across both views | 3,866,215 | 5,400,740 |
| Newly encoded distinct cache entries | 8,213 | 18,269 |
| Summed per-history build wall time | 460.25 s | 995.02 s |
| Summed clustering fit wall time | 4.05 s | 7.21 s |

Every selected non-whitespace assistant source character is checked in the
corrected pooled view. Structural coverage checks validate every selected prompt
and assistant source. All 320 artifacts pass provenance, matrix, normalization
and owner checks. Twelve fresh singleton encoder probes of newly included inputs
from four histories verify cached vectors bit-for-bit.

Total build wall time was 24:29.81, peak RSS 1,060,528 KiB. Scoring, source-quote
audits and bootstrap together took 59.17 s, peak RSS 1,059,608 KiB. The build ran
under substantial shared-host disk contention, including another GoHawk benchmark;
these wall times are not isolated inference throughput measurements. This live
build retained the original eight-vector durable cache transactions. The later
128-vector cache optimization was not injected into the run. Its six pinned
builder sources were archived and hash-verified before that optimization.

The earlier `turn-window-followup-results.md` full-answer trial used raw window
points and a different pipeline. It found some gains without passing its gates;
it did not evaluate the current smoothed pooled/structural union. This new result
supports a partial explanation for the long-recall gap, not the claim that omitted
answers caused all of it. Source completeness is retained independently of scores.
Pre-embedding denoising remains a separate follow-up, not part of this comparison.

Private artifacts, original source snapshots, two fixed Optuna trials, reports
and timing logs live under
`/home/james/scratch/mash-artifacts/complete-answer-baseline-20261006`. Protocol,
cohort, model, controls, annotations, code and output hashes are recorded in the
contract and reports. Original histories and quoted evidence stay outside Git.
