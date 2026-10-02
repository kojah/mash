# Dense compression with separate lexical macro evidence

This frozen operating point is not a selected production topic method. It provides
useful compression, but misses the retention target and loses recurring-topic
quality under reverse arrival. Compression is an optional internal optimization;
the actual requirements remain useful topics, online updates, no LLM, and bounded
complete-ingestion overhead.

## Procedure

Reuse all 162 QMSum training meetings, 88,336 source turns, 84,434 common-ready
turns and 8,196 immutable source-only blocks. Unknown and multi-label sources stay
in geometry. Unchanged 384-dimensional dense directions feed the existing
singleton BIRCH radius .4/branching 64 reference, forward and reverse. No radius
grid, projection, inference, model download, LLM or new heldout data. The original
18,316-dimensional lexical features supply population-weighted average linkage
over unrenormalized group means. Fixed balanced source lifting inside groups
serves only to evaluate a complete physical-block tree; it is not semantic ancestry.

Final native predictions define snapshot groups. Fidelity measures each original
dense unit against its normalized, recomputed snapshot group mean, not historical
insertion ownership or live CF centroids. Cosine within 1e-12 of .90 is uncertain
and excluded from confirmed passing; cancelled means fail. All original known
source-pair scopes and the immutable lexical hierarchy reference are retained.
Same-original-block positives are excluded. Internal 108/54 partitions have
already been explored and are not pristine heldout confirmation.

## Results

| Measure | Forward | Reverse |
| --- | ---: | ---: |
| Snapshot groups / physical CFs | 5,151 | 5,159 |
| Fraction retained (target <=.50) | .628477 | .629453 |
| Confirmed fidelity >=.90 (target >=.95) | .950952 | .948755 |
| All-pair purity | .536322 | .537421 |
| All-pair loss vs lexical .554138 | .017817 | .016718 |
| Far-pair purity | .536156 | .537235 |
| Crossspan purity | .604678 | .583004 |
| Crossspan loss vs lexical .622208 (target <=.02) | .017530 | .039204 |
| Known numeric state lower bound, bytes | 35,103,744 | 35,119,104 |
| Within-CF crossspan positive pairs | 4,669 | 1,933 |

There are 7,794/7,776 confirmed passing blocks, zero uncertain points, and
7,978,214 all / 7,923,483 far / 84,323 crossspan positive pairs. Crossspan supports
29 native topics in 21 meetings. Mean forward/reverse partition ARI is .309918,
minimum -.027468; this is snapshot partition agreement, not semantic stability.

Both orders fail retention. Reverse also fails fidelity and recurring-topic loss.
Known numeric state already exceeds the prorated 13,750,606-byte target. This
lower bound is `(2*physical_CF + merged_CF)*384*8`: singleton centroid initially
aliases linear sum; merged CFs have a separate mean, and prediction centers form
an independent bank. It concerns retaining all per-meeting sklearn model banks,
excludes node caches/scalars/metadata/lexical/macro/labels/ownership/temporaries,
and is neither observed process RSS nor complete persistent state. It rules out
this reference representation under that target, not compact Rust or BIRCH generally.

Post-hoc outside-CF crossspan diagnostics: forward 79,654 pairs purity .599777,
reverse 82,390 pairs .580787; inside-CF 4,669/.688290 and 1,933/.677524.
Different eligibility supports prevent direct comparison with the complete baseline.
The primary difference is not solely arbitrary within-group lifting; this does not
establish causality, semantic topic quality, or generalization.

## Reproducibility and audit

`dual_view.py`, `qmsum_dual_view.py`, five fixtures cover snapshot fidelity,
cancelled means, numerical uncertainty, ownership validation and partial budget
semantics. Enabled full lab suite: **501 tests pass in 2.185s**. No application
Rust changes or Rust verification claim. Existing source/helper contracts remain
unchanged. Artifacts outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-dual-view-first-20261002.json`, SHA256
  `85e0ba3320a08b6e229c072db159960734b3e0714b0218f565e6c634dd62e55c`.
- Repeat `topic-qmsum-dual-view-second-20261002.json`, SHA256
  `7da30475ade55fc9da210e01b1d78c248122ddfa78973331631540fe8f1e8bd4`.
  Every non-timing field matches, including full groups, CF state hashes, fidelity
  identities/cosines, trees/merges, metrics, split budgets and all input/code pins.
- Independent `topic-qmsum-dual-view-audit-20261002.py`, SHA256
  `1a80bb92c016539390ac93007029a066d923aa4a51a396b9f44a1251138afdc8`;
  `topic-qmsum-dual-view-audit-first-20261002.json` verifies true.
  Without lab helper imports it reconstructs source counts/lexical evidence,
  directly replays pinned sklearn CFs, validates alias/payload premises, computes
  scalar mean/cosine/fidelity, every global weighted macro minimum/tie, lifting,
  native source-pair LCAs/scopes/splits, scalar ARI and partial timing gates.
  324 cases, 9,986 macro merges, 31,972,040 pair-scope contributions;
  maximum metric/fidelity error 8.88e-16, macro height error 5.55e-16.
  Shared sklearn CF reference and NumPy/SciPy/frozen audited inputs: no second
  CF implementation, encoder, Rust parity or independent coding gold.
- Cost driver `topic-qmsum-dual-view-costs-20261002.py`, SHA256
  `baf996ac65f281efe2442e3a2cb32b9a70fa9c4d6af1d6a91a237f0e01786ee3`;
  `topic-qmsum-dual-view-costs-first-20261002.json` records scoped costs.
- Geometry SHA256
  `3f0b2c8632b8dda5ad34f3d058032cb24afb99d776998060e603b92cac63fdb1`;
  lexical hierarchy reference
  `8d11deaf3abde425b1ed972bb63460d5a54a59c04aaa28c26e35fea7d3246e1c`.

## Cost scope and decision

One numerical-library CPU thread. First/repeat after-import wall 55.519/45.788s,
CPU 30.270/32.197s, setup 36.003/20.768s. Forward/reverse CF fit wall first
4.848/4.953s, repeat 6.583/6.501s; macro first 3.198/3.171s, repeat 3.833/3.782s.
Mean CF-fit CPU per block first .551/.556ms, repeat .639/.627ms, all above .5ms.
P95 insert wall first .971/1.002ms, repeat 1.602/1.622ms. These omit conversion,
validation, final prediction and snapshots and cannot pass complete compression
or ingestion gates. Greedy Python macro is unoptimized/cubic; no optimized speed
claim. Imports/final JSON/repeat comparison are outside stage timers, inside
external observations; no intentional scientific benchmark overlap, uncontrolled
host load, no speed conclusion from variation.

Rusage peaks first/repeat 2,075,592/2,076,340KiB. Sampled RSS/PSS/status HWM
2,077,592/2,069,775/2,077,592 and 2,077,412/2,064,816/2,077,412KiB;
1,150/1,041 samples, 1/2 vanished reads, median/max gaps 42.637/260.353 and
43.837/137.703ms. Scope is target process including scientific imports/full-source
ledger; excludes observer and children. Sampler intrusive, sampled maxima lower
bounds, status approximate, launcher high water may be retained, no subtraction.
This is not local worker memory or complete additional clustering state.

Do not promote this operating point or silently relax its frozen targets. Next
evaluate a source-preserving online graph/tree alternative against the same
original-source recurring-topic reference. The 50% compression goal is optional
and not a user requirement. A new method needs an explicit protocol; retain
complete-ingestion <=10% loss, lifecycle, coding labels/multi-tags/semantic ancestors,
global node-group/provider behavior and selected Python/Rust differential CI gates.
The next shortlist includes [FISHDBC](https://arxiv.org/abs/1910.07283), which
approximates HDBSCAN* incrementally with arbitrary dissimilarities. The
[author implementation](https://github.com/matteodellamico/flexible-clustering)
separates additions from extraction and returns noise and condensed hierarchy.
This motivates a bounded evaluation, not a selection: measure extraction as well
as insertions and verify lifecycle support rather than inferring it from online
addition. Bead `mash-c0d.19.2.20.8.3.3.3.5` tracks the new protocol.
