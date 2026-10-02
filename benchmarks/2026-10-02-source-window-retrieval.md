# Frozen source windows: better first hits, weaker broad recall

Bead `mash-c0d.19.2.30.4.10`. Complete source-window matching is a useful
representation diagnostic, but these mixed development outcomes do not select
a topic method or justify changing production search.

## Prospective procedure

Preserve all 32,176 original candidates and 77,915 title/body windows from the
verified complete encoder capture. Use the same 64 development query **field**
vectors, not raw singleton vectors. Verify original identities, field hashes,
contiguous Unicode/UTF-8 spans and window hashes against the pinned preflight
and candidate-only original corpus. No new inference, projection, source
rewriting, omitted windows, query/qrel features or reserved scoring occurs.

Compare four fixed scorers: balanced title/body document cosine (exact archived
record replay), payload-weighted normalized body-field cosine, maximum signed
cosine across all original windows, and body-only maximum. Negative scores
remain negative; every source needs evidence. Exact window ties choose the
earliest original encoded row (titles then bodies, source ID then ordinal);
document ties use lexical original ID. Preserve full scores, winning rows and
source metadata. Mechanical encoder windows are not annotated dialogue episodes.

PARADE motivates studying how localized versus distributed evidence changes
aggregation effectiveness; this bi-encoder cosine control does not reproduce
its learned passage aggregator. [Primary paper](https://arxiv.org/abs/2008.09093).
Position-sensitive evaluation also matters, but winning-window position is not
a gold relevance span. [Positional-bias study](https://aclanthology.org/2025.ijcnlp-long.46/).

## Outcomes

| Fixed scorer | MRR@10 | nDCG@10 | Recall@10 | Recall@100 |
| --- | ---: | ---: | ---: | ---: |
| Balanced document | .3445 | .3519 | .4889 | .7397 |
| Body field | .3475 | .3506 | .4474 | .6620 |
| All-window maximum | .3602 | .3452 | .4722 | .6628 |
| Body-window maximum | .3723 | .3567 | .4308 | .6643 |

Body maximum improves first-hit ranking over body-field averaging, while
Recall@10 decreases. All-window maximum loses 7.69 percentage points of
Recall@100 versus balanced averaging. Neither is a clear replacement. These
are exposed development duplicate-link outcomes, not semantic topic quality,
statistically established gains or held-out agent navigation results.

Body-window-count strata were fixed before outcomes. Population counts are
22,358 / 9,548 / 270 for 1 / 2–4 / 5+ windows. Explicit positive support is
55 / 18 / 1 queries and 119 / 40 / 1 links respectively; a query can support
multiple strata. All scorers miss the sole 5+ positive at ranks 10 and 100.
That one example cannot support a general claim about long-document retrieval.

| Conditional positive Recall@100 | 1 window | 2–4 windows | 5+ windows |
| --- | ---: | ---: | ---: |
| Balanced document | .7121 | .6944 | .0000 |
| Body field | .6445 | .5764 | .0000 |
| All-window maximum | .6415 | .6412 | .0000 |
| Body-window maximum | .6433 | .6111 | .0000 |

Among each query's top 100, mean body-stratum counts are 67.20/32.25/.55
balanced, 66.03/33.39/.58 body-field, 64.31/35.02/.67 all-window, and
59.03/39.69/1.28 body-window. All-window winning evidence is a title in 62.84
results and a later body window in 9.52; body-only selects later body windows
in 21.55. Maximum scoring gives longer sources more opportunities to match;
these composition changes neither identify false positives nor establish that
later winning spans are relevant. Unjudged candidates are not negatives.
Every query has a declared positive; no-match behavior is unevaluated.

## Verification and cost

Two fresh complete executions match every decoded nontiming field and all seven
array hashes. The independent checker reconstructs complete original spans,
4,986,560 unit scores, maximum/winner reductions, 256 complete rankings, all
duplicate metrics, conditional support/recall and top-100 composition. Alternate
component reduction differs by at most 6.66e-16; metric error is zero. Exact
ties use the declared full GEMV stage. This is Python evidence, not Rust parity.
The lab suite passes 389 tests with the graph extra and pinned Links reference;
without those optional inputs, corresponding tests explicitly skip.

The two window policies share one full source-unit scan: 77,915 vectors versus
32,176 per document-centroid scan (2.42 times as many). The experiment additionally
scores both centroid baselines. Combined four-policy scoring/reduction/ranking/
metrics p50/p95 is 76.57/83.68 ms first and 77.34/84.63 ms repeat. After-import
total is 11.646/12.215 s, setup 6.004/6.569 s, CPU 11.024/11.288 s. Final JSON
serialization is outside this experiment timer. No independent per-policy
latency is fabricated from the shared scan; no query encoding occurs.

Rusage peaks are 1,250,164/1,250,212 KiB. External 498/436 samples observe smaps
RSS maxima 1,231,764/1,250,604 KiB and PSS 1,224,195/1,242,840 KiB; approximate
VmHWM is 1,250,164/1,250,212 KiB. Both processes exit zero. Requested interval
20 ms; maximum observed gaps 51.23/59.78 ms, vanished reads 1/0. Sampled peaks
are lower bounds; counters differ by sampling and accounting. Scope includes
imports, source validation, retained unit/field/score arrays and serialization,
excludes children/observer, and monitoring can perturb runtime. These are not
production worker memory, end-to-end search or complete-ingestion measurements.

## Reproduction and decision

Run `cqadup_window_retrieval.py --semantic FIRST_SEMANTIC --semantic-repeat
SECOND_SEMANTIC --preflight COMPLETE_PREFLIGHT --source ORIGINAL_SOURCE
--output-directory FRESH_DIRECTORY`, externally wrapped by `memory_audit.py`.
Repeat adds `--compare FIRST_DIRECTORY/results.json`. Use the pinned science
environment and one-thread CPU settings. Artifacts remain outside Git:

- First `topic-cqadup-window-retrieval-first-20261002/results.json`, SHA256
  `a18941c4533a0afe04ff945c28cd25421a1ac963b02b9691f4d04c50b9481e29`.
- Repeat `topic-cqadup-window-retrieval-second-20261002/results.json`, SHA256
  `70fa65c6bf791ccb8d89ac8060fe5408dd7148a89a3e899955ea1b248c277339`.
- Independent `topic-cqadup-window-retrieval-audit-20261002.py`, SHA256
  `265a6d130a1e8df142c2aeaaab00010b425d70c4180d9d6d37c640fab5380607`,
  and verified `topic-cqadup-window-retrieval-audit-first-20261002.json`.
- External `topic-cqadup-window-retrieval-memory-first-20261002.json` and
  `topic-cqadup-window-retrieval-memory-second-20261002.json`.

Complete coverage prevents truncation from explaining these differences. The
next unit-quality experiment needs source-grounded episode/context evidence
and an explicit granularity/calibration protocol, not a maximum-score default
or another exposed threshold grid. Public question structure differs from
agent sessions; possible embedding pretraining overlap remains unknown.
Semantic assignments, labels, ancestry, lifecycle, full-ingestion overhead and
Rust differential gates remain open. Production behavior is unchanged.
