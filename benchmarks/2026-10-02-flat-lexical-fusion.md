# Fixed lexical fusion does not reduce flat-topic fragmentation

## Protocol and implementation

Reuse the frozen exposed 128-thread cohort: 1,850 admitted occurrences, complete
compatible stored embeddings and 7,038 separate empty projections. Immutable
database SHA256 `251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`;
selection SHA256 `270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`.
The prior flat control SHA256 is
`9b0abe50883562377e7871d640473cfdd888fb2f407574ddca9f451c3615cb68`.

Three fixed arms, without output-driven retuning: original dense vectors,
source lexical TF-IDF, and normalized equal-weight concatenation. Lexical uses
the existing technical unigram/contiguous-bigram scanner, stoplist, maximum
8,192 features and source IDF `log((1+N)/(1+DF))+1`; TF is `1+log(count)`.
Vocabulary and IDF are fitted on ALL admitted development sources. This is a
transductive fixed-frame diagnostic, not an online lexical-frame contract or
held-out evaluation. Zero lexical rows stay explicit, with no invented words.
Half-fusion normalizes the weighted concatenation, so a zero lexical row keeps
its available dense geometry; its similarity to other hybrid rows still changes.
No inference, downloads, LLM, paid calls, resampling or source removal occur.

`topic_kernel.py` uses a bounded precomputed source Gram matrix. A topic column
stores the sum of similarities to its prior assigned members; its norm uses the
sum-vector norm update. Only already-arrived members contribute to routing,
although the lexical frame itself sees the whole cohort. Keep .55/.8 routing/
relative thresholds and at most three tags; non-singleton own-topic assignment
uses leave-one-out. All flat class labels use the shared source-counting/scoring
path. `topic_flat.py` now separates assignment from flat snapshot assembly.
This offline control needs no artificial projection of sparse lexical dimensions.
O(N-squared) Gram/accumulator memory is unsuitable as an unbounded production
stream design. No edit/delete/restart contract for this new router is selected.

Each arm runs forward and reverse arrivals. Original dense decisions exactly
match the existing vector-router flat control; maximum membership-score error
is 2.33147e-15, within the fixed 1e-12 tolerance. Topics, labels, source versions,
thread tags and diagnostics match exactly for dense. Three synthetic fixtures
cover dense routing/leave-one-out equivalence, explicit lexical zero rows,
normalized fusion and invalid inputs. The full research suite passes 644 tests
in 6.410s. This is not selected-method Rust parity or semantic-quality evidence.

## Results

| Forward diagnostic | Dense | Lexical | Half fusion |
| --- | ---: | ---: | ---: |
| Topics | 870 | 1,209 | 1,229 |
| Singleton topics | 593 | 1,034 | 1,051 |
| Weak-label topics | 698 | 1,087 | 1,098 |
| Multi-tag sources | 241 | 40 | 59 |
| Final untagged admitted sources | 10 | 33 | 0 |
| Unroutable geometry | 0 | 32 | 0 |
| Topics with >=2 supported label terms | 172 | 122 | 131 |
| Sources with at least one such topic | 1,053 | 662 | 685 |
| Forward/reverse co-membership pair Jaccard | .757117 | .923364 | .837299 |

Reverse lexical has 1,214 topics, 1,029 singletons, 1,082 weak topics and 35
untagged sources; half fusion has 1,229, 1,048, 1,096 and zero respectively.
Dense retains the earlier 856/578/673/seven. Pair counts forward/reverse are
23,171/27,130 dense, 5,786/5,558 lexical and 6,883/6,397 fusion. Mean thread
tag counts are 11.52/10.95 dense, 12.34/12.35 lexical, 12.48/12.49 fusion.
All non-runtime result fields repeat exactly.

The independently reconstructed .55 source-pair graph has 16,395 dense edges,
5,330 lexical and 5,586 fused edges. The same numeric threshold does not give
the same selectivity across representations. Fewer qualifying lexical/fused
similarities help explain fragmentation and higher pair stability; these graphs
are not the centroid-routing graph or a full causal decomposition. Higher
stability and 100% fusion tag coverage do not establish better topics. Label
support is a diagnostic, not semantic accuracy or independent source judgment.
The tested settings do not correct fragmentation, but do not rule out lexical
evidence with an independently calibrated operating point. Do not tune this
exposed corpus's threshold until a desired output appears or promote these arms.

The next quality experiment should test phrase-centered flat topic seeds and
source-grounded cross-thread memberships, rather than continuing to use every
small routing leaf as an independently named subject. That is a proposed
experiment, not a selected method or a promise that lexical topics are better.
Independent coding-session label/overlap usefulness and full ingestion costs
remain required before Rust or MCP promotion.

## Audit, runtime and memory scope

The auditor imports no producer modules and reuses the independent flat-output
audit boundary. Six snapshots, 6,607 topics and 4,558 scalar label scores pass;
maximum score error is 1.77636e-15. Source/thread/provider unions, support/DF
limits and repeats pass. It independently reconstructs source lexical weights
with scalar logarithms and norms, then counts pair edges. Shared dependencies
are the English stoplist, sklearn max-feature selection and NumPy/SciPy Gram
products. Routing winners are not independently replayed, and vocabulary/rank
selection, native parsing and semantic usefulness are not independently verified.

Forward routing/through-final-snapshot seconds, first / repeat:

| Arm | Routing | Through snapshot |
| --- | ---: | ---: |
| Dense | .385 / .300 | 1.928 / 2.591 |
| Lexical | .470 / 1.166 | 2.289 / 2.956 |
| Half fusion | .407 / .578 | 2.004 / 2.967 |

Shared setup is 1.010/1.238s after source loading; whole six-route/label experiment
takes 15.568/19.146s wall and 15.294/15.911s CPU before serialization/comparison.
Precomputation moves work outside the routing timer. These values are not
live production ingestion costs or evidence that the O(N-squared) router is an
appropriate optimization. Fixed arm order, caches, GIL, host and observer affect
timing. The complete-ingestion <=10% overhead gate remains untested.

External sampled target RSS peaks are 362,152/361,592 KiB and PSS
351,000/349,991 KiB. Approximate status HWM is 362,152/361,580 KiB;
pre-serialization rusage is 361,356/360,084 KiB, not a final process peak.
The target includes loading the previous flat result, all Gram frames and
accumulator workspaces, snapshots, 3.64 MB serialization and repeat comparison.
Do not subtract counters or attribute them to an isolated production worker.
There are 696/826 samples, one vanished read each and maximum gaps
54.62/204.10ms. Samples are lower bounds, status counters approximate,
observer/children excluded and monitoring intrusive.

Private result SHA256:

- First: `29eb0195b6ed2088e1170df1d559e7fa039f74e07b20b56036ab593839cf9177`.
- Repeat: `7fa4fb0617d49740902bc02f8f03fda1133520456b7a84412a05925368e83a9c`.

Artifacts use `topic-kernel-{first,repeat}-20261002` and `topic-kernel-audit-20261002`
under mash-artifacts, with code pins, logs and external memory sidecars. Raw
sources, vectors and private labels do not enter Git. No production method or
new default is selected.
