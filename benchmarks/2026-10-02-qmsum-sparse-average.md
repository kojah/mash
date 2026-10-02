# Sparse online average-link source-block controls

Decision: advance **rotation-only lexical average linkage** to a stable-online-
feature evaluation. It passes the provisional recurring-topic loss target in both
orders and has lower reference insertion cost than grafting. This is a candidate
for further testing, not a production method or demonstrated semantic quality.
Broad-topic quality remains weaker than the offline reference; no target is
silently waived and no production gate passes.

## Frozen procedure and reuse

[SPARSE_AVERAGE_PROTOCOL.md](../research/topics/SPARSE_AVERAGE_PROTOCOL.md) was
written before outcomes. Same162 QMSum training meetings/88,336 source/84,434
common-ready/77,892 singleton-known turns/8,196 immutable blocks. Fixed18,316dim
original sublinearTF-IDF lexical primary, unchanged384dim dense control. All
unknown/multi stay in geometry; all/far/crossspan known-source LCA class fractions
exclude same-block positive pairs. No new inference/download/LLM, projection,
parameter/cut grid or heldout. Internal108/54 partitions already explored.

`SparseAverageTree` adapts CSR input/storage/means/scores/nearest lookup/validation,
reusing unchanged historical `online_tree.Tree` rotate/graft/restructure operations.
Internal means are **not renormalized**: mean(A).mean(B) equals mean original
unit-leaf cross-pair cosine. No majority-source labels, geometric semantic ancestry,
or author-GRINCH equivalence claim. Dense controls use unchanged
`Tree(Linkage.AVERAGE)`. Greedy, rotation-only and GRINCH-style graft, both arrival
orders:1,944 complete source trees. Max2,000leaves/20,000 sparse dimensions;
exact nearest searches, no operation caps, strict comparisons/arrival ties.

Validate every fixture prefix and each final corpus tree. Capture all source
identities/tree ownership, aggregate/mean fingerprints/counts, operation counters,
numeric-buffer footprint, source metrics and source/helper/protocol pins. Leaf
normalization is repeated by each prototype; parity fixtures compare actual
normalized leaf inputs, continuous non-boundary and exact binary-rational ties.
Full-training vocabulary/IDF and per-meeting frames are still offline, not a
stable global node-group/provider contract.

## Results

| View / method | All forward/reverse | Return forward/reverse | Return loss forward/reverse vs .622208 |
| --- | ---: | ---: | ---: |
| Lexical greedy | .556657 / .539962 | .592639 / .618573 | .029569 / .003635 |
| Lexical rotation | .528326 / .533018 | .624317 / .603018 | -.002109 / .019190 |
| Lexical graft | .534008 / .528979 | .623869 / .601902 | -.001661 / .020306 |
| Dense greedy | .501655 / .483196 | .577864 / .579604 | .044344 / .042604 |
| Dense rotation | .463926 / .461634 | .579633 / .560276 | .042575 / .061932 |
| Dense graft | .457736 / .454897 | .561885 / .546357 | .060323 / .075851 |

All/far/return pairs7,978,214/7,923,483/84,323 unchanged, returns29 topics in21
meetings. Lexical average all/far/return .554138/.553862/.622208; balanced
chronology .815016/.814376/.576852; root .387068/.387531/.529726. Rotation lexical
far .528074/.532782, all loss .025813/.021120. Thus the prespecified necessary
return target<=.02 passes both rotation orders but broad quality is not recovered.
If applying the earlier compression control's all-pair .02 target as an additional
criterion, rotation fails both orders. Do not call this an all-quality pass.

Graft reverse misses return target by .000306; this small threshold distinction
is not statistical significance or evidence of semantic superiority. Greedy is
better broad forward but misses recurring forward. Dense controls all miss return
target. Rotation receives next-stage priority based on this limited quality/cost
tradeoff; independent coding/guided-search evidence is still absent. Do not treat
these source blocks as proven semantic episodes or labels/ancestors.

Rotation exact leaf comparisons299,070/299,274, nearest searches8,034 per order,
rotations18,623/18,651, zero graft/restructure actions. Exact search and ancestor
rotations remain unbounded for future large global streams.

## Repeat and independent audit

Four new fixtures cover dense parity at every prefix (all modes/two orders),
continuous non-boundary parity, pair-average identity, high18316dim sparse input,
invalid rows/bounds and stale aggregate/source detection. Enabled full lab
**509 tests pass in2.857s**. No Rust/app changes or application validation claim.

Artifacts outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-sparse-average-first-20261002.json`, SHA256
  `ebf21316701a36181eb60c99fa3bf2d79ff46c6f9c3b00b204d84935fff7d5b8`.
- Repeat `topic-qmsum-sparse-average-second-20261002.json`, SHA256
  `8dd901f6134e5af22fc5d5f0d3cb456c92dd2dbc811ead90ed70c49a95c1c362`.
  Every non-timing field exact, including all trees/source mappings/aggregate
  fingerprints/counts/operation statistics/numeric footprints/metrics/splits/pins.
- `topic-qmsum-sparse-average-audit-20261002.py`, SHA256
  `3c24f77766b06db73da4531ffad24157016f94229bae38ef7bb06b37a0a985cd`;
  `topic-qmsum-sparse-average-audit-first-20261002.json` verifies true.
  No lab helper imports: original source memberships/counts/vector preparation,
  dependency-postorder subtree reconstruction, all aggregate/mean fingerprints,
  direct cross-leaf average similarity, complete source ownership, direct source
  pair LCAs and all split summaries.1,944 cases/194,760 aggregate fingerprints/
  191,832,240 pair-scope contributions; metric error0, maximum mean/direct score
  error6.66e-16. Shared NumPy/SciPy/frozen inputs, not independent graft algorithm
  replay or author GRINCH proof; numeric footprint itself not independently audited.
- Cost driver `topic-qmsum-sparse-average-costs-20261002.py`, SHA256
  `94a2889e729619c591e60b855c306cf5d0f19fb3b47892a77ceb936c8bd97f78`,
  report `topic-qmsum-sparse-average-costs-first-20261002.json`.

## Scoped costs and state

One numerical-library CPU thread. First/repeat complete after-import wall179.871/
219.590s, CPU170.414/173.644s; setup13.133/14.825s, features252.860/310.941ms.
Full run includes all12 controls and validation/snapshot/quality accounting, not
one selected production pipeline. Lexical insertion costs:

| Control | First fit CPU forward/reverse, s | Repeat fit CPU forward/reverse, s | First p95 forward/reverse, ms | Repeat p95 forward/reverse, ms |
| --- | ---: | ---: | ---: | ---: |
| Greedy | 7.292 / 7.253 | 7.372 / 7.383 | 1.588 / 1.505 | 1.882 / 1.712 |
| Rotation | 9.706 / 9.864 | 9.917 / 10.064 | 2.351 / 2.322 | 3.241 / 3.428 |
| Graft | 37.148 / 38.388 | 37.422 / 39.150 | 11.991 / 13.053 | 15.943 / 15.901 |

Rotation wall fit10.263/10.198s first,13.274/13.250s repeat; validation3.928/3.920s
and4.894/4.878s, snapshot1.154/1.094s and1.461/1.295s. Insertion includes CSR
normalization/storage maintenance and prototype aggregate work, excludes final
validation/snapshot/source evaluation/labels/persistence. This is unoptimized
small per-meeting exact Python reference, not Rust/large-group update or complete
ingestion. No intentional scientific benchmark overlap, host load uncontrolled,
no speed conclusion from repeat variation. Imports/final JSON/repeat comparison
outside stage timers, inside external scope.

Rotation numeric-buffer view footprints101,538,492/102,635,244 bytes when
hypothetically retaining all per-meeting trees. Graft106,601,796/108,189,588;
greedy85,517,964/87,352,932. Dense controls1,095,045,120 each, including reserved
2,000-row matrix capacity per meeting: allocated logical views include untouched
storage, not resident usage. Buffer address-range union avoids alias double-counts,
but omits Python metadata/identities/source bank/temporaries/backing capacity
outside views and is not complete persistent-state accounting. No memory budget
passes; this is distinct from observed whole-process peak.

Rusage first/repeat2,077,664/2,076,820KiB; sampled RSS/PSS/statusHWM
2,078,224/2,070,416/2,078,224 and2,076,036/2,066,148/2,076,820KiB;
4,819/5,105 samples,2/0 vanished, median/max gaps36.883/129.263 and38.112/400.158ms.
Target includes imports/source ledger, excludes observer/children. Intrusive
sampled maxima lower bounds, status approximate, launcher high water possible,
no subtraction. Repeat RSS below statusHWM is consistent with missed transient
peak, not evidence to discard either counter. No worker-memory attribution.

Next bead `mash-c0d.19.2.20.8.3.3.3.7`: freeze stable source-only global lexical
coordinates/weights/unknown-term/version semantics, test rotation candidate without
future-dependent IDF, and inspect actual topic terms/representatives. The
reference-boundary work must also unify leaf norm edge semantics: the sparse
adapter's epsilon cutoff differs from the dense prototype's positive-finite norm
check on tiny nonunit inputs, and overflow/underflow require explicit rejection
before any promotion. The corpus reference uses finite unit directions; these
fixtures do not establish universal malformed-input parity. Use a typed sparse
aggregate contract when selecting an implementation; current reuse is a dynamic
scientific adapter, not the final Rust representation.

Bounded work/state/lifecycle/global provider/node-group, independently assessed
coding label/multitag/semantic-ancestor/guided-search, complete ingestion<=10% loss
and selected Python/Rust differential CI remain necessary. No method selected
for Rust or production.
