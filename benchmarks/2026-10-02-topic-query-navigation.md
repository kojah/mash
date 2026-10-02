# Grounded queries: labels, full group words and direct source retrieval

P0 `mash-c0d.19.2.20.8.3.3.3.9.2`, Python-only research. Previous support
experiment increased label availability without demonstrating quality.
[Frozen protocol](../research/topics/QUERY_NAVIGATION_PROTOCOL.md).
No production method selected; no Rust/app/deployment change.

## Setup and scope

Use all 1,095 specific queries and 1,206 inclusive reference spans in the pinned
162-meeting QMSum **training** source. General queries have no spans and are
outside the declared task. Reference spans enter after discovery, frozen label
ranks and query scores. Never use answers or native topic names as features.
Keep 88,336 original source identities, 84,434 ready sources, 1,296 previously
audited meeting-local groups and both frozen block/message-supported label sets.

Features reuse original marker-cleaned counts, unchanged static stoplist and
frozen full-snapshot word IDF. Sublinear TF precedes IDF, with L2 normalization.
Full groups pool original ready-source counts **before** TF/IDF. Displayed label
bags assign one count per term, without extra topic-score weight. Queries use
the existing two-character tokenizer/marker accounting; no fit from queries.
Zero/OOV queries stay in the denominator. No new inference/model/download/LLM,
custom filler rule, parameter grid, held-out read or membership change.

Four arms: direct source retrieval; navigation by full group words; navigation
by displayed block-supported labels; navigation by message-supported labels.
Topic arms select at most **two** positive-score groups, then rank sources
within them using the **same direct-source scores**. Return at most **20**
positive-score original ready sources. No zero padding or direct fallback.
Exact float descending scores, ascending global ID for ties. All original
reference IDs remain in recall/ideal-DCG denominators, including unsupported
ones; overlap/duplicate spans merge by original ID. Precision divides by 20.

Both meeting-conditioned (known thread analogue) and global candidate scopes
are paired. Global inventory is the union of meeting-local groups, not a
shared discovered taxonomy. The query's annotated meeting supplies reference
IDs; other meetings cannot earn benchmark hits, but are **unjudged**, not
necessarily irrelevant. This limits global interpretation. This is lexical
navigation, not mash hybrid/dense/reranked search or an agent workflow trial.

## Results

Macro metrics over all 1,095 queries:

| Meeting scope | Recall@20 | Precision@20 | MRR | NDCG@20 | Candidate recall | Zero results |
|---|---:|---:|---:|---:|---:|---:|
| Direct sources | .193067 | .171872 | .537946 | .276575 | .951957 | 13 |
| Full group words | .184373 | .164977 | .528084 | .270806 | .769196 | 13 |
| Block labels | .119224 | .099224 | .282983 | .162278 | .390576 | 460 |
| Message labels | .121328 | .101279 | .293875 | .166704 | .398765 | 416 |

| Global scope | Recall@20 | Precision@20 | MRR | NDCG@20 | Candidate recall | Zero results |
|---|---:|---:|---:|---:|---:|---:|
| Direct sources | .050725 | .024886 | .153729 | .060144 | .951957 | 0 |
| Full group words | .054629 | .027763 | .110569 | .062441 | .104317 | 0 |
| Block labels | .049864 | .030457 | .092100 | .056812 | .116855 | 13 |
| Message labels | .046969 | .026393 | .086683 | .052978 | .103083 | 7 |

Compared with direct sources, meeting recall improves/declines/ties on
97/142/856 queries for full group words, 88/565/442 for block labels and
86/551/458 for message labels. Global counts are 127/244/724,
139/256/700 and 124/262/709. No significance or held-out generalization claim.

All 56,494 repeated original reference-source contributions remain accounted
for, including 2,803 unsupported contributions; 53,691 are ready. No query is
entirely outside the frozen vocabulary. Ready-reference macro coverage .951957;
the diagnostic ready-source oracle at the same 20-result budget caps macro
recall at .666561 (not a retrieval feature). Long reference spans therefore
limit achievable recall, but do not account for the large label-gating loss.

In meeting 0, queries 1--4 have nonzero direct/full-group recall but zero
results from both label arms. Query 0 retrieves label matches but no relevant
source; query 5 preserves the direct result. These examples are source/query
ordinal diagnostics, not hand-picked wording changes or label tuning targets.

**Decision:** reject using the tested short label lists as mandatory automatic
search gates. Added label availability does not prevent relevant-source loss.
Full group words lose less in meeting scope but still trail direct sources;
global average recall/NDCG rise slightly while MRR and many query outcomes
decline. Do not promote full-group gating from that mixed result. Next evaluate
nonexclusive topic guidance preserving direct candidate eligibility, with full
group evidence rather than displayed label vocabulary. This does not reject
all topic discovery or explicit user-requested topic filters, establish label
semantics, or select a clustering algorithm.

## Repeat and independent audit

Artifacts outside Git, prefix `/home/james/scratch/mash-artifacts/`:

- `topic-qmsum-query-navigation-first-20261002.json`, SHA256
  `096ec1a15eebb5b0adbe43656075e89b2a2776daf2c1df0b1e54befe126d56e6`.
- `topic-qmsum-query-navigation-second-20261002.json`, SHA256
  `4bd0166e3f4a5349d296e1267c71157126286d4c6753dba66d8fa1523c9ed7cc`.
  Complete non-timing repeat exact; begin/end pins unchanged.
- `topic-qmsum-query-navigation-audit-20261002.json`, SHA256
  `6f224be98033c4f5d7a6f2066946672f3086cf5bf86f883046e5fcc9be55b9e1`.
  Independent scalar count/TF/IDF/fsum-normalized features and inverted-index
  query scores, original query/spans/ownership, mathematical rank and metrics.
  577,607 constructed feature coordinates, 1,095 queries, 8,760 cases,
  15,330 ranks, 115,936 selected scores. Max numeric error 4.44e-16;
  832 choices differ under scalar re-association, max rank gap 1.11e-16.
  Ranks satisfy frozen 1e-12; exact tie branch/repeat remains separately fixed
  by the executable/fixtures. Metrics independently audit actual returned IDs,
  not substituted scalar tie winners. Auditor source SHA256
  `26b0b030de8e133ec7755c5bab6146d13f428d9dfde9fc40a537415bca767471`.
  Shared pinned original counts/IDF/vocabulary/label-tree capture and
  NumPy/SciPy/sklearn stoplist; source tokenizer previously audited.

Four new fixtures cover zero/OOV abstention, original-ID ties, duplicate/invalid
rank input rejection, no hidden fallback and unsupported references in metric
denominators. Enabled research suite 560 tests passes. No Rust validation claim.

## Costs and limits

First/repeat after-import wall 46.489/41.906s; CPU 34.221/33.492s;
setup 19.438/18.413s, feature construction 194.309/117.925ms. Sum of query
projection plus **all eight** ranking/metric cases: median/p95
22.128/40.644ms first, 19.951/29.434ms repeat. Not one request's latency:
includes reference metrics and candidate accounting, lacks actual mash search
inference/HTTP/worker/backend/reranking/persistence. Large input validation loads
unused raw embedding/context ledgers. Main excludes imports/final JSON/repeat
comparison; external target includes them. Scientific runs sequential; full
fixture suite overlaps first setup, brief first-summary inspection overlaps
repeat setup. Host uncontrolled; no production speed claim.

Rusage peaks 2,016,356/2,016,376KiB. External RSS/PSS maxima
2,017,772/1,983,883 and 2,017,744/2,005,113KiB; approximate status HWM
2,017,556/2,017,744KiB. Samples 1,082/1,060, vanished 1/2;
median/max gaps 41.803/148.451 and 40.297/105.135ms. One target, excluding
observer/children; monitoring intrusive, sampled maxima lower bounds, status
counters approximate, launcher rusage may retain high water. No subtraction
or attribution to deployed workers.

Independent coding/semantic topic labels, multitag/semantic-ancestor quality,
held-out/agent guided-search utility, online/global frame/lifecycle,
edit/delete/restart, complete ingestion <=10% loss and selected executable
Python/Rust parity remain open. This adds a retrieval guardrail, not production
topic readiness.
