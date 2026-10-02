# Fixed CF compression plus population-weighted macro discovery

Scope: full already-explored training corpus, unchanged 8,196 source-only blocks
and 18,316-dimensional sparse sublinear TF-IDF/L2 snapshot. No model/LLM calls,
projection, heldout data or threshold grid. This tests the existing BIRCH operating
point as compression, without treating its CF tree as semantic ancestry.

## Frozen procedure

Singleton-update sklearn BIRCH radius .4 / branching64 / no final global clustering,
forward and reverse original block arrival. Physical CF state retains source-count,
center/linear-sum hashes, radii and squared sums. Complete final nearest-CF
predictions define a canonical snapshot partition: **not historical insertion
ownership**. Full-vocabulary IDF is fixed; no online vocabulary/frame stability
or edit/delete/restart behavior is validated.

For each group, recompute the mean original unit block vector **without L2
renormalizing that mean**. Initial macro distance is `1 - mean cross-block cosine`.
Merge using population-weighted average updates: group populations count original
physical blocks. Round distance to12 decimal places for ordering, then earliest
original block pair; record unrounded distance. This is a constrained weighted
average-link reference rather than treating all compressed centroids as equally
large or changing distance through renormalization. The Python greedy reference
scans all active pairs at each merge; it is unoptimized and cubic, unlike the
preceding SciPy reference. Do not use their fit timers as an optimized algorithm
speed comparison.

Lift compressed groups with a fixed source-order balanced tree solely to retain
all original physical block leaves for diagnostic scoring. This fabricated
within-group shape supplies no semantic ancestry. Retain mixed/unknown sources
as in the prior control. Same known-source weighted physical-block LCA metrics,
same exclusions, all/far/disjoint-native-component scopes and original chronological
controls/reference. Record crossspan pairs that fall inside a compressed group,
so apparent macro success cannot hide that part of the lifting.

## Full-source outcomes

Both orders retain complete support: 162 meetings, 88,336 original turns,
84,434 common-ready / 77,892 singleton-known turns; 7,978,214 / 7,923,483 /
84,323 all/far/crossspan positive source pairs. Sparse recurring-topic support
remains 29 topics / 21 meetings; original54/108 partition already explored.

| Control | Snapshot groups | All purity | Far purity | Crossspan purity |
| --- | ---: | ---: | ---: | ---: |
| Uncompressed lexical average | 8,196 input leaves | .554138421 | .553861722 | .622208107 |
| CF then weighted macro, forward | 8,179 | .554138221 | .553861519 | .622208107 |
| CF then weighted macro, reverse | 8,180 | .554138673 | .553861975 | .622208107 |

Physical CF subcluster counts equal final group counts for this capture.
Only17/16 leaves removed (0.2074%/0.1952%): this operating point delivers almost
no compression and no material quality change. Zero crossspan positives fall
inside compressed groups. Therefore the unchanged recurring-topic score is not
inflated by arbitrary within-group lifting for those positives.

For two unit vectors, merged radius is `sqrt((1-cosine)/2)`: radius.4 requires
cosine at least.68 for that initial pair. This provides a plausible geometric
explanation for the observed near-singletons; the experiment does not establish
that BIRCH generally fails or that another geometry/calibration cannot compress.
Mean partition order ARI .995553, minimum .279631. The high mean is largely
trivial when almost all groups are singletons, not demonstrated topic stability.

Do not select this BIRCH setting for production compression. The broader lexical
candidate remains promising for recurring topics, but still needs effective online
geometry, source-backed labels, coding/session-independent multi-topic/ancestor
gold and guided-search evidence. Next operating-point work needs an explicit
compression/quality/cost budget; do not expand a radius sweep to fit these outcomes.

## Validation and artifacts

`compressed_macro.py`, `qmsum_compressed_macro.py`; five fixtures cover duplicate
ownership in both orders, non-renormalized means, population-weighted distances,
one-group lifting/ties and invalid input/ownership. Full extras/explicit author
reference suite: **491 tests pass in1.606s**. No Rust/application code changed.

Outside Git under `/home/james/scratch/mash-artifacts`:

- First full capture `topic-qmsum-compressed-macro-first-20261002.json`, SHA256
  `0c898a3cdd93236f2de94083e5f116760c10c2f8280df8c81af8cb542cabf0cc`.
- Full repeat `topic-qmsum-compressed-macro-second-20261002.json`, SHA256
  `092a5ec08e53757d3472bd7b2f229e96177a0c2f27f583ae862ede6aec09f6eb`.
  Every non-timing field exact, including partitions, full physical CF state/hash,
  macro merges/topology, all per-topic metrics, order ARI and source/protocol/code pins.
- Immutable block and offline macro input pins are carried in both captures.
- Cost driver `topic-qmsum-compressed-macro-costs-20261002.py` SHA256
  `cf3245e327d01feaea047d19cbdad66a05427448069efbb0afe8bf68ad97f912`,
  report `topic-qmsum-compressed-macro-costs-first-20261002.json`.
- Independent audit `topic-qmsum-compressed-macro-audit-20261002.py`, SHA256
  `08c52fa001eb2bfc2ff6eab369dfac8e48845419632c7c47f71db6e578a54d23`;
  report `topic-qmsum-compressed-macro-audit-first-20261002.json`, verified true.
  No lab helper imports. Reconstruct source membership/counts/lexical features;
  replay pinned library singleton CF updates, complete native predictions and
  physical state hashes; independently recompute every macro cross-leaf mean,
  global minimum/tie, population and lifting, original source-pair LCA scores,
  summaries and scalar contingency ARI. All324 cases/16,035 macro merges/
  31,972,040 pair-scope contributions pass. Integer/state/topology exact;
  maximum metric error1.11e-16, merge error5.55e-16. Shared sklearn CF reference,
  NumPy/SciPy sparse arithmetic and frozen audited count inputs remain; no
  independent CF algorithm, inference or Rust parity/semantic gold follows.

## Scoped costs

One numerical library CPU thread, installed pinned sklearn reference. Offline
after-import wall/CPU first117.174/67.319s, repeat65.385/54.550s;
setup47.778/12.714s. Forward/reverse CF fit first23.945/20.980s,
repeat15.430/15.819s; corresponding fit CPU17.403/14.948 and11.967/12.258s.
Final predictions/state capture first3.046/3.095s, repeat2.877/2.888s;
greedy macro7.527/7.272s and6.570/6.781s. CF-only insert p95 first11.015/10.702ms,
repeat7.463/8.058ms. Snapshot prediction rescans units; CF centroids are dense
even with sparse inputs. These stages do not include full production context,
label maintenance and persistence and cannot establish ingestion overhead/p95.

After-import timer excludes imports/final JSON/repeat comparison; external sampling
includes them. No intentional scientific benchmark overlap in monitored runs;
host load uncontrolled, no speed claim from run variation. Rusage peak
2,076,408 / 2,076,788KiB; external RSS/PSS/status-HWM first
2,077,652/2,069,733/2,077,652 and repeat2,077,292/2,069,580/2,077,292KiB.
2,733/1,953 samples,0/1 vanished reads; median/max gaps35.332/329.759 and
33.525/79.298ms. Target scientific process includes imports/full ledgers,
excludes observer/children; not model-worker scope. Sampling intrusive, maxima
lower bounds/status approximate/pre-exec retained high water possible; no
subtraction. Complete-ingestion≤10%/lifecycle/Rust and semantic quality gates remain.
