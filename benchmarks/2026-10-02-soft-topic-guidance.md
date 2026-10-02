# Nonexclusive parent-rank guidance on grounded queries

P0 `mash-c0d.19.2.20.8.3.3.3.9.3`, Python-only research.
[Frozen protocol](../research/topics/SOFT_TOPIC_GUIDANCE_PROTOCOL.md).
No production topic method selected; no Rust/application/deployment change.

## Method and evidence

The preceding experiment rejects displayed labels as mandatory automatic
navigation gates. Here **all direct positive-score sources remain eligible**.
Use the same frozen groups, original counts/static stoplist/full-snapshot
TF/IDF/normalization, 1,095 specific training queries/1,206 spans, meeting and
global scopes and maximum 20 returned sources. No answers/native topic names,
new inference/model/download/LLM, cutoff/weight grid or held-out data.

[Cormack, Clarke and Büttcher (2009)](https://cormack.uwaterloo.ca/cormacksigir09-rrf.pdf)
combine document ranks with reciprocal contributions, fixing k=60 before their
validation. Adapt this as `1/(60+source_rank)+1/(60+parent_group_rank)`.
Ranks start at one; the parent term is zero without a positive full-group
cosine. Both weights/60 fixed before outcomes. Unlike their document
permutations, the same parent rank is broadcast to several sources. This is
a **parent-rank adaptation**, not a reproduction of their RRF experiment or
proof the weight is right for topic guidance. Group size/duplicated vocabulary
can bias the bonus. Do not add zero-source-score candidates. Full rank pools
are retained, with exact descending float/global-ID ties and no topic gate.

Direct IDs/scores/metrics exactly replay the preceding frozen control. Original
ready coverage and all 56,494 repeated reference-source contributions stay
unchanged, including 2,803 unsupported ones; 53,691 ready. Every query,
unknown/OOV case and source ID remains accounted for. Global candidates union
meeting-local groups; other meetings are unjudged, not known negatives. This
is exploratory lexical retrieval, not mash hybrid/reranked search, a shared
learned taxonomy, causal online inference or a guided agent workflow.

## Results

| Scope / arm | Recall@20 | Precision@20 | MRR | NDCG@20 | Zero results |
|---|---:|---:|---:|---:|---:|
| Meeting direct | .193067 | .171872 | .537946 | .276575 | 13 |
| Meeting parent bonus | .195939 | .173470 | .542060 | .280952 | 13 |
| Global direct | .050725 | .024886 | .153729 | .060144 | 0 |
| Global parent bonus | .078522 | .042237 | .197217 | .090787 | 0 |

Paired recall improves/declines/ties on 45/12/1,038 meeting queries and
238/13/844 global queries. Ready-candidate reference coverage .951957 and
returned counts remain identical in every query. Mean returned-source counts
17.008/19.982 in meeting/global scope. No significance or held-out coding
generalization claim. The global annotation caveat prevents treating all
other-meeting hits as false positives.

**Decision:** retain nonexclusive full-group guidance as a candidate for
held-out and bounded-cost validation. Do not promote it from these training
gains. It preserves access better than mandatory display-label gating at these
tested settings, but does not establish useful semantic labels, multi-topic
membership, hierarchy/ancestor meaning or a production clustering method.

## Reproducibility

Artifacts outside Git, prefix `/home/james/scratch/mash-artifacts/`:

- `topic-qmsum-soft-guidance-first-20261002.json`, SHA256
  `1a4c6c992a2bd18213ae5a53bf20235d934db2e16eea4de7c167359f7b1ea0dc`.
- `topic-qmsum-soft-guidance-second-20261002.json`, SHA256
  `42967d5147c0c32291267546ea47f72b861979561cf5342c440ce4c1efe962be`.
  Complete non-timing repeat exact; begin/end source pins unchanged.

- `topic-qmsum-soft-guidance-audit-20261002.json`, SHA256
  `f4bc7a24920bf44d3829e04f77f0a8dd4a1f92174b091a627b387d4a6c3ad677`:
  terminal independent audit
  passes all 1,095 queries/4,380 cases/81,008 selected source records. Checks
  568,026 feature coordinates against scalar count/TF/IDF/fsum normalization,
  maximum discrepancy 2.22e-16. Independently reconstructs original spans,
  query/parent ownership, complete ranks, parent bonus and selected metrics.
  Exact rank/selection uses separate formula replay with shared NumPy/SciPy
  IEEE kernels; this is **not** independent scalar equivalence of tie winners.
  Tiny scalar normalization differences can change rank ties and need a chosen
  cross-language contract before porting. Shared frozen counts/IDF/groups and
  sklearn stoplist, source tokenizer previously audited. Auditor source SHA256
  `903cfdd2d0c588f0bf820c6128229ef94798089c34817bfdfcc07a8973dccaa5`;
  audit-only reusable reference SHA256
  `a8335735133093502aed4115905b3bdc2c2fcbe81413d89d406127f6751d55a7`.

Four new fixtures cover eligibility/zero preservation, a parent-induced reorder,
missing parent evidence, invalid parent scope and scalar original-count TF/IDF
normalization. Full enabled research suite 564 tests passes.

## Measured costs and remaining gates

First/repeat after-import wall 97.973/92.608s, CPU 92.879/88.428s;
setup 15.639/14.966s, feature construction 65.324/73.407ms. Parent bonus
ranking plus metric/accounting median/p95 0.208/0.436 and 0.205/0.409ms in
meeting scope; 47.382/81.317 and 45.669/77.119ms globally. Global prototype
sorts all positive source candidates and allocates per-source Python records;
this is not a bounded production query path or one complete request's latency.
Includes evaluation/accounting, excludes model/HTTP/backend/reranking/
persistence. Large setup validates unused raw embeddings/context ledgers.
Main excludes imports/final JSON/repeat comparison; external target includes
them. Scientific runs sequential; brief first-timing inspection overlaps repeat
setup. Host uncontrolled; no production performance or ingestion claim.

Rusage peaks 2,059,428/2,059,552KiB. External RSS/PSS maxima
2,060,316/2,052,457 and 2,060,416/2,052,534KiB; approximate status HWM
2,059,936/2,060,416KiB. Samples 2,693/2,639, vanished 1/1;
median/max gaps 35.422/82.748 and 34.392/71.893ms. One target excludes
observer/children; monitoring intrusive, sampled maxima lower bounds, status
approximate, launcher rusage may retain high water. No subtraction or deployed
worker attribution.

Before promotion: frozen held-out evaluation and independent coding-session
quality/agent utility, numerically robust Python/Rust rank ties, bounded global
query cost, semantic labels/multitag/ancestors, shared online frame/lifecycle,
edit/delete/restart, complete ingestion <=10% loss and selected executable
Python/Rust parity. Current production readiness remains unproven.
