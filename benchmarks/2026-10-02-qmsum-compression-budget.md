# Source-only compression feasibility under an explicit budget

Scope: necessary geometric condition for per-meeting compression of immutable
blocks. No clusters fit, labels generated, inference/model download/LLM calls,
heldout sources or gold-based threshold selection. This is not a general verdict
on BIRCH, lexical topic discovery or shared cross-thread groups.

## Targets frozen before outcomes

[COMPRESSION_BUDGET.md](../research/topics/COMPRESSION_BUDGET.md) records provisional
engineering targets: <=50% groups, >=95% blocks with unit-centroid cosine>=.90,
all/crossspan hierarchy-purity loss<=.02 absolute versus lexical reference,
compression-only meanCPU<=.5ms/p95wall<=1ms per closed block and additional
persistent state<=16MiB/10,000 active blocks. These are proposed comparison
targets, not user mandates, measured guarantees or a replacement for the existing
complete-ingestion<=10% loss, lifecycle, independent semantic quality and Rust
parity CI gates. Any revision must be explicit and justified in a new protocol.

## Necessary bound

For unit vectors x,y,c with x.c,y.c>=t and0<=t<=1, residual Cauchy-Schwarz and the angle
addition formula give x.y>=2t^2-1. At t=.90, any two passing members sharing a
centroid need pair cosine>=.62. Build that possible-pair graph with a conservative
1e-12 inclusion margin. Every isolated vertex needs its own centroid unless it
is among the allowed floor(N/20) outliers. Across per-meeting groups a conservative
lower bound is `max(nonempty_meetings, sum_isolated - floor(total_blocks/20))`.
Group budget is floor(N/2). Lower bound above budget rules out both targets
together for that scope/view; a smaller bound means **not ruled out**, never proven
feasible. No matching, covering, clustering or semantic result is inferred.

Same all162 meetings / 88,336 source turns / 84,434 common-ready turns / 8,196
immutable source-only16-turn/512-token/gap blocks. Include unknown/multi evidence.
Three fixed unit views: original18,316dim sparse sublinearTF-IDF/L2, unchanged384dim
pooled dense directions, and equal-energy direct sum of those views (cosine is
their arithmetic mean). No weight grid/projection. Gram symmetry/unit norms/PSD
are validated; store full neighbors, isolated identities, nearest cosines, Gram
fingerprints and code/source/protocol pins. Full-training vocabulary/IDF is frozen
offline; online stability and group/provider namespaces remain untested.

## Results

| View | Isolated blocks | Allowed outliers | Necessary centroids >= | Group budget | Verdict |
| --- | ---: | ---: | ---: | ---: | --- |
| Lexical | 8,151 | 409 | 7,742 | 4,098 | Ruled out under these targets |
| Dense | 813 | 409 | 404 | 4,098 | Not ruled out; not proven feasible |
| Equal-energy hybrid | 7,231 | 409 | 6,822 | 4,098 | Ruled out under these targets |

Explored development108 meetings/5,459 blocks: lower bounds lexical5,155,
dense209, hybrid4,546, budget2,729; internal reserved54/2,737:2,588/196/2,277,
budget1,368. Same necessary verdicts, not independent heldout confirmation.
Lexical would need to retain at least94.46% of units under this fidelity target;
hybrid83.24%. This is consistent with the preceding lexical BIRCH control's
negligible compression without making an algorithm-wide failure claim. A radius sweep
cannot defeat this necessary condition while preserving the same targets/view.

Next evaluate a dual-view candidate: dense-space compression with original lexical
macro evidence, measuring actual distortion/retention/quality/cost against the
frozen contract. The dense bound does not promise success. Different geometry,
fidelity targets, global cross-thread groups or online feature frames require
new recorded experiments. Topic label/multi-tag/ancestor/coding/guided-search,
complete ingestion and selected Rust method remain unverified.

## Validation and captures

`compression_budget.py` / `qmsum_compression_budget.py`; five fixtures cover
orthogonal impossibility, exact shared-centroid boundary, singleton/global
outlier accounting, open verdict semantics and invalid Gram/ownership.
Full extras/explicit author reference lab suite: **496 tests in1.796s**, passes.
No Rust/application changes or Rust verification claim.

Outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-compression-budget-first-20261002.json`, SHA256
  `0f673703aa9ea524f7c5111130ec38121f8372ba6306e3cb6c7969bf1033ad91`.
- Fresh repeat `topic-qmsum-compression-budget-second-20261002.json`, SHA256
  `adbb3fcdb3248d23aba324794808c7e76d1a829ddabb15705582cf4aaf85ea2c`.
  Every non-timing field exact, including all neighbors/isolated IDs/closest
  values/Gram hashes, all splits/bounds and source/contract/code pins.
- Independent audit `topic-qmsum-compression-budget-audit-20261002.py`, SHA256
  `292e98351ccf4a6d57069b88797901a6281c619d77ac3d7afc4aa84a6bcb2c30`;
  report `topic-qmsum-compression-budget-audit-first-20261002.json`, verified true.
  No lab helper imports. Verify original source/accounting, reconstruct lexical
  block features, scalar sparse/dense/direct-sum pair cosines, all neighbors/
  nearest values/isolates and every split's integer budget/bound. All162 meetings,
  486 views,246,924 distinct block pairs/740,772 scalar-view pair checks pass;
  maximum cosine error1.78e-15. Neighbor identities/bounds/Gram fingerprints exact.
  Shares NumPy/SciPy for Gram hashes and frozen previously audited block/count/
  inference inputs; no independent encoder/Rust/cross-thread/semantic gate.
- Cost driver `topic-qmsum-compression-budget-costs-20261002.py` SHA256
  `cfad0f337ab3c558d602b7554360d0a4194e7310349cee256bbd080c802b96bc`,
  report `topic-qmsum-compression-budget-costs-first-20261002.json`.

## Scoped costs

One numerical-library CPU thread. Offline after-import wall/CPU first16.942/11.435s,
repeat20.456/11.995s; full capture setup16.459/20.052s. Feature/Gram stage
276.705/225.859ms; feasibility validation (including eigenspectrum)/graph stage
178.237/153.987ms. No cluster update or complete-ingestion latency is measured.
Imports/final JSON/repeat comparison outside stage timers, inside external scope.
No intentional scientific benchmark overlap in monitored runs; uncontrolled host
load, no speed claim from run variation.

Rusage2,047,792/2,047,680KiB; external sampled RSS/PSS/status-HWM first
2,048,924/2,041,129/2,048,924 and repeat2,040,916/2,029,954/2,047,680KiB.
437/494 samples,1/0 vanished reads; median/max gaps39.457/101.212 and
43.760/102.185ms. Scientific imports/full ledgers included, observer/children
excluded, not model-worker or persistent cluster state. Sampler may perturb
runtime; sampled maxima lower bounds/status approximate/retained launcher high
water possible; no subtraction. The repeat's sampled RSS below status-HWM is
consistent with missing a transient peak. No budget runtime/state/ingestion gate
passes from these offline costs.
