# Exact vectorized topic guidance: matched scope costs

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.1`, Python-only research.
[Frozen protocol](../research/topics/VECTOR_GUIDANCE_PROTOCOL.md).
Optimization of the existing reference, not a new quality experiment or
production algorithm selection. No Rust/app/deployment change.

## Change and contract

`vector_topic_guidance.py` validates integer ownership and maps arbitrary
global IDs through sorted lookup arrays, without allocation indexed by maximum
ID. It vectorizes full positive-rank reciprocal contributions/final ordering
and creates only at most 20 returned Python records. Source and group pools
remain complete; no candidate cap, rounded score, quantization, new parameter,
label gate, new inference/model/download/LLM or held-out read.

Run scalar then vector on identical original source/group cosine arrays for
all 1,095 specific training queries, both meeting/global scopes. Same frozen
counts/TF/IDF/stoplist/full-topic inventory/source IDs, k=60, top 20 and exact
float/global-ID ties. Every vector field must equal both the scalar call and
the prior frozen guidance capture: ID, base score/rank, parent ID/score/rank,
fused score, candidate count and positive-group count. Direct source scores/IDs
also replay the previous control. Original metric ledger is retained because
every returned identity/rank matches; it is then independently re-audited from
original queries/spans. No answers/topic-name features or altered denominators.

## Evidence and timings

All 2,190 scoped pairs match every record field exactly. Complete non-timing
repeat and begin/end code pins pass. The previous gains and losses stay exactly
unchanged; this is not a new independent quality result. Four new differential
fixtures include 200 deterministic generated cases, arbitrary IDs/permutations,
positive/negative/zero scores, exact ties, one-ULP neighbors, empty inventories
and rejected invalid ownership. Full enabled research suite 568 tests passes.

Ranking-only median/p95 milliseconds (matched sequential calls):

| Scope | First scalar | First vector | Repeat scalar | Repeat vector |
|---|---:|---:|---:|---:|
| Meeting | .178/.338 | .291/.490 | .180/.377 | .282/.514 |
| Global | 29.402/54.553 | 18.733/32.995 | 32.401/62.168 | 19.139/41.373 |

Vectorization improves global median in these runs, but regresses the small
meeting scope by about .1ms. No claim that it is faster universally. Both
implementations still rank all positive candidates and validate the full scope;
fixed NumPy overhead/ownership lookup can dominate small inputs. No adaptive
selector/threshold or caching scheme is inferred from these measurements.
The table excludes query projection, direct baseline checks, equivalence checks,
metrics and actual model/HTTP/backend/reranking/persistence. Earlier bonus
timings included metric/accounting work; compare these matched pairs rather
than treating this as a deployed end-to-end speedup.

Complete first/repeat after-import wall 88.623/95.479s, CPU 84.066/88.361s,
setup 16.090/15.541s, feature construction 78.423/68.624ms. Full targets include
BOTH implementations and equivalence/capture work, plus large input validation
of unused embedding/context ledgers. Main excludes imports/final JSON/repeat
comparison; external target includes them. Scientific runs sequential, brief
first-timing inspection overlaps repeat setup. Scalar always precedes vector;
cache/order/host effects uncontrolled. No full-ingestion/performance-gate claim.

## Reproducibility and audit

Artifacts outside Git, prefix `/home/james/scratch/mash-artifacts/`:

- `topic-qmsum-vector-guidance-first-20261002.json`, SHA256
  `7eef914839df5786563f206fcdb23b1bc79243fd2310f3732a0051efe32d1007`.
- `topic-qmsum-vector-guidance-second-20261002.json`, SHA256
  `d001638b9954bea45cdc38b5720fe6462f13aa3431a592f905351005b1203f21`.
- `topic-qmsum-vector-guidance-audit-20261002.json`, SHA256
  `557a6e98c3947836994c9472153393e66fbe0ab4213088938063a4cea6b45849`:
  the existing independent guidance auditor passes all 1,095 queries/4,380
  retrieval cases/81,008 selected source records and 568,026 feature coordinates,
  maximum scalar error 2.22e-16.
  Original counts/query/spans/ownership/formula/metrics are independently
  reconstructed. Exact rank replay shares NumPy/SciPy IEEE kernels; scalar
  fsum geometry agrees within 1e-12, not independent scalar tie-winner parity.
  Shared pinned counts/IDF/groups/sklearn stoplist; tokenizer previously audited.
  Auditor SHA256 `903cfdd2d0c588f0bf820c6128229ef94798089c34817bfdfcc07a8973dccaa5`;
  reusable audit reference SHA256
  `a8335735133093502aed4115905b3bdc2c2fcbe81413d89d406127f6751d55a7`.

Rusage peaks 2,096,072/2,095,600KiB. External RSS/PSS maxima
2,096,828/2,088,967 and 2,096,372/2,088,456KiB; approximate status HWM
2,096,828/2,096,372KiB. Samples 2,418/2,507, vanished 0/1;
median/max gaps 36.041/78.774 and 37.592/88.570ms. One target, BOTH
implementations; no per-implementation memory attribution. Excludes observer/
children, monitoring intrusive, sampled maxima lower bounds, status approximate,
launcher rusage may retain high water. No subtraction or worker-memory claim.

Parent issue remains open. Needed next: plain-thread-context control to isolate
the value of clustering, a robust numerical near-tie contract, frozen held-out/
coding quality and agent utility, semantic labels/multitag/ancestors, bounded
global online lifecycle/edit/delete/restart, full ingestion <=10% loss and
selected executable Python/Rust parity. Faster exact replay is not readiness.
