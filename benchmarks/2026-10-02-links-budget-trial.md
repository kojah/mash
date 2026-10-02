# Budgeted Links reference: default subcluster growth fails complete coverage

Bead `mash-c0d.19.2.30.4.9`. The pinned reference with frozen defaults reaches
8,192 active subclusters after only about 26% of the corpus. Structured failures
retain all covered sources and unprocessed counts. This rejects this configuration
under the stated budget, not Links in general. No topic method is selected.

## Prospective procedure

Follow the [primary-source assessment](2026-10-02-links-assessment.md), unchanged
`Tc=.5, Ts=.8, Tp=1`, empirical sum centroids and exact original 384-dimensional
balanced unit vectors for all 32,176 candidates. Prevalidate unit directions and
disable reference input renormalization to preserve the original vector stage.
Fresh model per forward/reverse canonical original-ID arrival order; no projection,
inference, generative/paid calls, query/qrel features or reserved scoring.

Stop before creating a node beyond 8,192 active subclusters. The wrapper performs
the same nearest scan before the author call, so measured cost includes duplicate
scans. The creation branch cannot immediately merge that new node; a joining
branch remains allowed at capacity. Preserve attempted identity, insertion and
graph mutation traces, sums/counts/source ownership, historical versus final
labels, and full unprocessed counts. Do not silently call a prefix complete or
repair thresholds/caps. All semantic quality/ancestry fields remain null.

Instrument inherited graph mutation methods without changing topology decisions.
Validate source sums, complete unique ownership, active centroid storage, component
registries/connectivity, symmetric edges and edge thresholds every 256 insertions
and at final state. Reference one-edge creation, contraction and cut/rejoin
transitions preserve a forest: active node bounds limit component repair searches
and possible contractions; check 16,384 edges at snapshots. This is a bounded
research state/validation policy, not an exact internal dot-work counter or
production p95 contract. Independent final replay confirms the forest invariant.

Synthetic fixtures exercise an actual node merge, permanent component split,
alternate partner repair, current-storage exact ties, reverse source ownership,
source corruption rejection and capacity failure before mutation. Historical
answers remain intact while final membership changes after a split. Six explicit
reference tests pass before corpus execution; these are additional to the author
tests. The [paper](https://arxiv.org/html/1801.10123v1) and
[pinned implementation](https://github.com/wq2012/LinksCluster/tree/33faf171cf145a1e0a8b35d75730db5aca22baf1)
remain the algorithm sources, not evidence of mash semantic quality.

## Outcomes

| Prefix diagnostic | Forward | Reverse |
| --- | --- | --- |
| Complete discovery | No | No |
| Covered / unprocessed documents | 8,351 / 23,825 | 8,403 / 23,773 |
| Corpus coverage | 25.95% | 26.12% |
| Active subclusters | 8,192 | 8,192 |
| Singleton subclusters | 8,093 (98.79%) | 8,079 (98.62%) |
| Maximum subcluster source count | 7 | 21 |
| Graph components / edges | 20 / 8,172 | 28 / 8,164 |
| Maximum / median component source count | 3,251 / 21 | 6,681 / 12.5 |
| Historical versus final numeric label differences | 5 | 20 |

Each failed attempted source is unprocessed and leaves the model unchanged.
Almost one active subcluster per inserted source explains the budget failure.
The largest components contain 38.93%/79.51% of their respective prefixes;
component connectivity does not establish a semantic grouping or parent label.
Few historical ID changes do not prove semantic quality or order stability.
The prefixes are different, disjoint sets of original sources; no full-corpus
ARI, coassignment, multi-topic recall, labels or navigation outcomes are reported.
Do not compare component counts directly with complete-corpus PCA/online cosine
studies as if source coverage matched.

## Verification and measured costs

Fresh complete diagnostic repeats match all decoded nontiming fields and eight
array hashes. 384 lab tests pass with the explicit pinned reference enabled;
without it the six reference tests skip, rather than claim reference coverage.
The audit replays 16,754 inserted events with the uninstrumented pinned model,
uses alternate component reduction for nearest scores, independently reconstructs
all graph mutations/components, source ownership/sums, edge validity and labels.
It covers 16,384 final subcluster records and 16,336 retained edges; maximum
score/source-sum differences both 8.88e-16. Event dynamics use the same author
implementation, not a fully independent algorithm implementation or Rust parity.
No invariant failure is observed; process exit zero means successful failure
reporting, not successful complete discovery.

First forward/reverse guarded discovery is 16.374/13.149 s; repeat 14.065/12.419 s.
Insertion p50/p95 is .502/1.958 and .434/1.398 ms first; .561/1.801 and
.475/1.520 ms repeat. These per-insertion timers exclude periodic snapshot checks;
guarded discovery includes those checks and duplicate nearest scans. First/repeat
after-import experiment total 31.743/28.502 s, CPU 30.032/28.466 s; setup
1.221/1.016 s. Final JSON serialization is outside that experiment timer.
Repeat overlaps auditing; monitored runtimes are not controlled speed comparisons,
author-only performance, complete indexing or ≤10% ingestion evidence.

Separate rusage peaks 574,620/575,668 KiB precede final JSON output. External
1,092/1,100 samples yield smaps RSS and approximate status VmHWM
575,504/576,080 KiB, PSS 567,692/567,873 KiB. Requested sleep 20 ms; actual
median/max gaps 29.92/106.94 and 27.93/47.86 ms, zero/one vanished reads.
Both monitored processes exit zero. Sampled RSS/PSS maxima are lower bounds;
status counters approximate, target-only scope excludes children/observer. Imports,
original vector arrays, retained graph/member/event state, serialization and
monitoring contribute. No deployed worker-memory or update/restart guarantee follows.

## Reproduction and decision

Run `cqadup_links_trial.py --semantic SEMANTIC_FIRST --semantic-repeat SEMANTIC_SECOND
--author PINNED_AUTHOR --output-directory FRESH_DIRECTORY`, externally wrapped
by `memory_audit.py`; repeat adds `--compare FIRST_DIRECTORY/results.json`.
Use the pinned science environment, single-thread CPU settings and verified author
checkout from the assessment. Author source and all captures remain outside Git:

- First `topic-cqadup-links-first-20261002/results.json`, SHA256
  `954ee2a7277297a54e6a61c1230f3535157b045ce703ab2ba649cdd1ba32403c`.
- Repeat `topic-cqadup-links-second-20261002/results.json`, SHA256
  `72bc20d12ce3c3ac440d2a8dfa74abed27c5a861275e884ed687a73d7b36be41`.
- `topic-cqadup-links-audit-20261002.py` and verified
  `topic-cqadup-links-audit-first-20261002.json`.
- `topic-cqadup-links-memory-first-20261002.json` and
  `topic-cqadup-links-memory-second-20261002.json`.

Do not enlarge the exposed cap or tune defaults to erase this outcome. The
preflight established formulas, not text-domain calibration; an uncalibrated
reference default is not a fair test of every possible Links configuration.
Further discovery work needs an evidence-backed unit/granularity/calibration
protocol, independently annotated topic evidence and new reserved evaluation.
Keep ordinary direct search intact. Online lifecycle, ownership/publication,
semantic ancestry, complete ingestion and Rust differential gates remain open.
