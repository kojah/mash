# Held-out topic guidance: prespecified benefit gate fails

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.5`. The complete held-out run, repeat and
independent audit pass. **The candidate does not pass its quality gate.**
Do not promote automatic topic ranking or select a production topic method
from this experiment. This is a result, not a reason to retune this split.

## Procedure and original support

Follow the unchanged, previously frozen
[protocol](../research/topics/GUIDANCE_VALIDATION_PROTOCOL.md), SHA256
`729e1bf96b57b37fd967a054297e38a37b29d6312928228df6dfb060fe65744f`.
Author validation contains35 meetings/20,123 original turns,237 specific
queries/248 original inclusive spans. Count35 general queries separately;
they have no native source-span relevance judgments. Train/validation ordered
whole turn-text fingerprints are disjoint; shared individual utterances remain
as documented in the source contract. Test remains untouched.

Reuse the pinned training vocabulary18,316 terms/IDF and independently audited
source units:18,916 ready/1,207 unsupported/2,073 physical blocks. No held-out
fit, answer or annotated topic-name features, query-driven inventory, grid,
case removal, new embedding inference, LLM/API calls or model downloads.
Build the entire source inventory before scoring native queries. Original
raw exact-marker counts produce lexical-average hierarchy/fixed-eight cuts;
source/query/parent retrieval uses the original static-noise mask. This is the
same geometry/search distinction as the training reference, not a new mask.
There are280 topic parents and35 whole-thread parents. Source raw counts have
2,338 OOV unique-term occurrences across rows/2,507 token occurrences,1,281
zero-known rows; queries have33 OOV unique-term occurrences and no zero-known
query. Original identities, zero vectors and all query references remain.

Run direct, topic-parent and thread-parent arms, each exact-float and frozen
numeric diagnostic, in known-meeting and global scopes. Complete positive
source/parent pools, fixed parent bonus RRF60 and top20; no mandatory topic
gate, cap, padding or fallback. Pooled counts precede TF/IDF/L2. Known-meeting
sole-thread-parent ranking/metrics equal corresponding direct arms exactly.
All13,443 original relevant references remain across queries, including795
unsupported references;12,648 are in candidate pools. Macro candidate recall
is.949765. Global other-meeting sources are **unjudged**, not known negatives.

## Results

| Scope / exact arm | Recall@20 | Precision@20 | MRR | NDCG@20 |
|---|---:|---:|---:|---:|
| Known meeting, direct | .168453 | .148734 | .486606 | .240699 |
| Known meeting, topic | .170503 | .149789 | .489299 | .244520 |
| Known meeting, thread | .168453 | .148734 | .486606 | .240699 |
| Global, direct (diagnostic) | .062416 | .041561 | .218167 | .084830 |
| Global, topic (diagnostic) | .088332 | .055907 | .249173 | .110297 |
| Global, thread (diagnostic) | .076647 | .051055 | .252521 | .103337 |

Known-meeting topic guidance improves recall for8 queries, declines for3,
ties226;705→710 relevant hits. Both have2 zero-result queries and3,753 returned
sources. Global topic/direct changes are48 improvements/3 declines/186 ties,
but incomplete cross-meeting judgments prevent treating these as deployment
quality. Global thread MRR exceeds topic MRR; different metrics favor different
context granularities. Do not choose a winner after seeing held-out outcomes.
Numeric diagnostic arms have the same aggregate metrics here; that is not
universal membership/numerical parity or grounds for switching the primary arm.

Primary known-meeting paired macro recall delta is+.00205048. The prespecified
10,000 whole-meeting bootstrap (PCG64 seed20261002, resample35 meetings,
preserve all selected meetings' queries, pooled per-query mean) gives95%
interval **[-.00082126,.00602959]**. Its lower endpoint is not positive:
**primary gate fails**. This does not establish absence of all topic utility.
MRR delta+.00269272, interval[-.01101259,.01597830], passes the provisional
noninferiority lower-bound>-.02 safety gate. Safety alone is not success.

## Reproducibility and independent audit

Complete non-timing repeat is exact. Producer code pins are checked before/
after the run; source/material input pins at load and original source/unit
pins again in the independent post-run audit. The producer dependency list
does not include indirect `captured_hierarchy.py`; the auditor separately
records its hash. No source/dependency changes occurred between these runs.

The separate auditor imports no producer functions. It reconstructs native
source counts, frozen features, all hierarchy/cuts/ownership, native queries/
original spans, every complete rank pool, selected IDs/scores/ranks, metrics,
summary, and bootstrap via expanded selected query lists rather than the
producer's bincount reduction.35 meetings/237 queries/2,844 cases/50,958
selected sources and294,227 feature coordinates pass; maximum compared
numeric error4.44e-16. Already audited source unit readiness/bounds are shared,
as are frozen training frame/stoplist, tokenizer assets, SciPy linkage and
NumPy/SciPy IEEE score kernels. Scalar feature checks are not proof of scalar
rank winners or cross-platform/Rust parity. Fraction arithmetic and bootstrap
generator/percentile implementation are shared. These limitations matter.

Private artifacts remain outside Git under `/home/james/scratch/mash-artifacts`:

- First `qmsum-validation-guidance-first-20261002.json`, SHA256
  `149d5b71028f8e393a6065d91c35cf914b186157f0e79928b8b7fab7e96555d2`.
- Repeat `qmsum-validation-guidance-second-20261002.json`, SHA256
  `1fcd567b01f4e5884369827efb58a1cbacfc9369b4b3ebc3826ad1429b623586`.
- Audit `qmsum-validation-guidance-audit-20261002.json`, SHA256
  `0d43783c642fc2ddd8de122cc5412c84aaf107e1a1df763bd03ae5c0dfe0d066`;
  auditor SHA256 `f9b925138cecbef655dc5e97fedccb2f83c35361f24c66365bd7243745886c8d`.
- Audit-only scalar/IEEE feature reference SHA256
  `a8335735133093502aed4115905b3bdc2c2fcbe81413d89d406127f6751d55a7`;
  indirect topology dependency SHA256
  `6d497ec502db7e94c4cf16fff8ef15fae83d26a04f4ff7e432aa2c709c51edad`.

Producer: `qmsum_validation_guidance.py --artifacts ROOT --output FIRST`, then
fresh second output with `--compare FIRST`. Both targets wrapped by
`memory_audit.py`; finish each authoritative process before launching the next.
Audit: `audit_validation_guidance.py --artifacts ROOT --capture FIRST
--output AUDIT`. Science Python3.11.15, NumPy2.4.6/SciPy1.17.1/sklearn1.9.1,
single-thread BLAS/OpenMP, pinned research environment. No Rust/app/deployment
change. Full enabled research suite593 tests passes; remote CI unverified.

## Scoped cost, not ingestion or application latency

First/repeat after-import wall20.524/24.119s, CPU20.265/22.365s,
setup2.447/2.851s, parent feature preparation27.313/52.932ms. Main excludes
imports/final serialization/repeat comparison; external target includes them.
Global exact direct/topic/thread retrieval-plus-metrics median1.694/3.237/
2.972ms first,1.991/3.582/3.209ms repeat. Numeric counterparts12.506/17.227/
18.378ms first,14.154/20.793/21.162ms repeat. Projection is separately timed.
Fixed arm order, deallocation, caches, monitoring and host activity are not
controlled; these are Python reference costs, not Rust search/E2E latency.

Rusage416,112/415,956KiB. External target RSS/PSS414,600/406,934 and
415,896/408,078KiB; approximate HWM416,112/415,956KiB. Samples864/941,
vanished reads0/0, median/max gaps25.537/39.982 and27.423/60.891ms. One target
only; children/observer excluded. Monitoring intrusive; sampled maxima lower
bounds, status approximate, launcher high water may survive exec, no peak
subtraction or worker attribution. Repeat comparison retains an extra ledger
outside main timing. Scientific targets sequential; fixture/inspection work
around runs and host effects are uncontrolled. This is not the full-ingestion
<=10% throughput-loss gate.

## Method decision and next evidence

Do not promote this candidate to automatic ranking. Evaluate topic taxonomy
and browsing separately from rank guidance: the test addresses source retrieval,
not meaningful topic labels, multiple tags, ancestor inheritance or exploration.
Training context controls and this validation suggest broad context matters;
they do not identify the best topic algorithm. BIRCH remains a candidate with
previously recorded shortcomings, not an architectural requirement.

Prioritize a coding-agent evaluation contract with credible label/assignment/
ancestor and exploration relevance evidence before further clustering tweaks.
Compare a simple thread/episode lexical-profile baseline against discovery;
then evaluate bounded online assignment with periodic discovery as a separate
hypothesis. No chosen algorithm yet. If this validation motivates a revision,
call it exploratory and freeze a new final protocol before accessing untouched
test data. Do not retune thresholds, labels, block budgets or bonus on this split.
Production selection still requires coding/agent usefulness, shared multi-provider
online edit/delete/restart, full ingestion costs and executable Python/Rust parity.
