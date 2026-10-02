# Cut-free lexical-block macro reference

Scope: offline quality diagnostic on the already explored QMSum author training
corpus. No production topic method selected. This tests representation before
calibrating a frontier or implementing an online compressor.

## Frozen procedure

Reuse all 162 meetings / 88,336 original turns, 84,434 common-ready turns,
77,892 singleton-known turns and 8,196 immutable source-only blocks from the
audited 16-turn / 512-original-token / unsupported-gap policy. Do not use topic
annotations for blocks, features or discovery. Keep unknown/multi-topic sources
and mixed blocks; no majority-label conversion. There are no zero lexical blocks
in this capture; the implementation retains them if present.

Compare unchanged pooled dense directions with original full-training sparse
sublinear TF-IDF / L2 block features. No projection, extra inference, model download
or LLM calls. For each meeting, use SciPy average linkage on clipped
`1 - cosine` with zero diagonal and original block order. This is the mean distance
between all leaf pairs, rather than centroid linkage, as specified by the
[official SciPy reference](https://docs.scipy.org/doc/scipy/reference/generated/scipy.cluster.hierarchy.linkage.html).
Fixed block chronological balanced / forward-comb / reverse-comb trees and root
class prevalence are controls. No cut, threshold grid, external heldout access or
semantic ancestry claim. Sparse full-vocabulary features are an offline snapshot;
online vocabulary/IDF stability remains a separate requirement.

Each eligible positive source pair contributes its native topic's known-source
fraction at the LCA of its **physical blocks**. Counts preserve all singleton-known
members of mixed leaves. Exclude every same-block source pair in every scope.
`all`: remaining same-topic pairs; `far`: also original ordinal gap >4;
`crossspan`: also different merged native topic span components. Unknown/multi
sources remain in geometry, outside the conditional gold denominator. This is
source-weighted conditional purity, not accuracy or evidence of meaningful labels.
Do not directly compare to prior turn-leaf purity: eligible pairs have changed.

Full source support: 7,978,214 all pairs / 7,923,483 far pairs / 84,323 crossspan
pairs; 708 / 698 / 29 supported native topics. Crossspan support is only 21 meetings.
Internal 54 reserved / 108 development partition is already explored, not pristine
heldout; IDF uses the full training snapshot as in preceding controls.

## Results

| Tree | All purity | Far purity | Crossspan purity | All topic-macro | Crossspan topic-macro |
| --- | ---: | ---: | ---: | ---: | ---: |
| Lexical average | .554138 | .553862 | .622208 | .463387 | .472117 |
| Dense average | .475705 | .475791 | .561768 | .372722 | .453131 |
| Chronological balanced | .815016 | .814376 | .576852 | .743769 | .401117 |
| Forward comb | .537396 | .537900 | .569462 | .456035 | .364747 |
| Reverse comb | .582651 | .583338 | .552094 | .423612 | .367556 |
| Root only | .387068 | .387531 | .529726 | — | — |

Lexical blocks improve the aggregate broad and recurring-topic hierarchy scores
over dense blocks. Their recurring-topic score exceeds the fixed chronological
controls, but broad topic structure still trails balanced chronology and the
reverse comb. Broad annotations are largely contiguous, making temporal controls
especially relevant. This is evidence to continue a lexical-block candidate with
separate local episode and recurring macro-topic behavior; it does not justify
selecting this tree as a complete production topic method. Sparse return support,
label quality, multi-tags, semantic ancestors, coding-session generalization,
guided search and online lifecycle remain unverified. No significance/winner claim
or a new post-hoc parameter search follows from these aggregate scores.

## Validation and capture ledger

`block_hierarchy.py` / `qmsum_block_hierarchy.py` implement the bounded reference;
immutable prior helpers retain their original code pins. Six meaningful fixtures
cover mixed-leaf fractions, within-block exclusion, near/span accounting, unknown
and zero-vector leaves, exact semantic merges/ties, invalid ownership/asymmetry,
and JSON node-ID key types.

The first repeat attempt failed in comparison, after all meetings completed:
numeric in-memory tree object keys sorted differently from string JSON keys.
Freeze JSON key types explicitly before storing/comparing. Preserve original first
capture and failed run's memory observation; produce two fresh final captures.
This affects serialization comparison, not any feature, tree or score.

All captures are outside Git under `/home/james/scratch/mash-artifacts`:

- Geometry input: `topic-qmsum-block-geometry-first-20261002/results.json`, SHA256
  `3f0b2c8632b8dda5ad34f3d058032cb24afb99d776998060e603b92cac63fdb1`.
- Original first: `topic-qmsum-block-hierarchy-first-20261002.json`, SHA256
  `44627aa983b1162fad99e5ba18ca04bffb00fc61581ffffed4eb1f766bda53d5`.
- Final first: `topic-qmsum-block-hierarchy-final-first-20261002.json`, SHA256
  `8d11deaf3abde425b1ed972bb63460d5a54a59c04aaa28c26e35fea7d3246e1c`.

- Final repeat: `topic-qmsum-block-hierarchy-final-second-20261002.json`, SHA256
  `97dc62df415c1bfd2258013be812bd78231249a3b7b1c94dff30bd1d5b17feff`.
  Every non-timing field matches exactly, including complete topology, merge
  heights/counts, support, per-topic metrics, all summaries, source/protocol/code pins.
- Independent original-source pair/LCA/global minimum average-distance audit:
  `topic-qmsum-block-hierarchy-audit-20261002.py`, SHA256
  `b98ec91c7699472762a960133558ab15a96ac90a2e398254dfed4afd18798e9d`;
  report `topic-qmsum-block-hierarchy-audit-first-20261002.json`, verified true.
  No lab helper imports: reconstruct original memberships and counts, block
  lexical weights, every LCA and eligible source pair; recompute every average
  merge's cross-leaf mean and global active minimum, chronological topologies
  and all split summaries. All 810 cases / 79,930,100 pair-scope contributions /
  16,068 average merges pass. Integer counts/topology exact, maximum metric error
  `2.91e-11`, average-distance error `7.77e-16`. Also bridge original to final
  first: all non-timing fields and all code pins except the serialization driver
  unchanged. Shares NumPy/SciPy sparse arithmetic and previously audited frozen
  blocks/counts/inference. No independent encoder or Rust implementation parity.
- Full extras/explicit author reference lab suite: **486 tests pass in 2.239s**.

## Scoped costs

One CPU thread per numerical library, NumPy2.4.6/SciPy1.17.1; actual linkage
implementation/dependency versions and source pins stored in captures.
`topic-qmsum-block-hierarchy-costs-20261002.py` SHA256
`c858ead7e79900baa88de6a7f32d37865ef07afb647f7eff7c51ad4ebaddbfc1`
summarizes final captures and external observations in
`topic-qmsum-block-hierarchy-costs-first-20261002.json`.

| Offline stage, complete162 meetings | Final first | Final repeat |
| --- | ---: | ---: |
| After-import wall / CPU, s | 34.647 / 14.140 | 23.103 / 13.324 |
| Full capture setup, s | 32.055 | 20.935 |
| Lexical block features, ms | 346.594 | 294.367 |
| Lexical distances + linkage/topology, ms | 221.613 | 154.656 |
| Dense distances + linkage/topology, ms | 170.308 | 111.980 |
| Lexical / dense gold evaluation, ms | 321.947 / 318.429 | 279.304 / 269.016 |

These are offline scientific stage totals, not ingestion overhead, production
query latency or an online runtime gate. Average linkage is quadratic in memory
and time in its input units per the cited reference; reducing to blocks does not
establish network-scale suitability. Timers exclude scientific imports, final
JSON serialization and full-repeat comparison; external sampling includes them.
No other intentional scientific benchmark overlapped final monitored runs;
external host load is uncontrolled and no speed claim follows from variation.

Linux process rusage peaks 2,048,280 / 2,048,276 KiB. External sampled
RSS/PSS/status-HWM: 2,049,640/2,036,894/2,049,640 and
2,049,740/2,037,584/2,049,740 KiB. 682/534 samples, one vanished read each;
median/max gaps 52.843/246.542 and 42.430/143.170ms. Scientific imports/full
capture ledgers dominate this process scope; it is not model-worker memory.
Observer/children excluded, monitoring may perturb timings, sampled RSS/PSS peaks
are lower bounds, status counters approximate, launcher high water may persist;
no subtraction. Complete ingestion ≤10% overhead, p95, edits/deletes/restart,
stable online feature frames and selected Rust parity CI remain open.

Close the bounded offline child `mash-c0d.19.2.20.8.3.3.3.1`. Continue its P0
parent with online compression/separate macro discovery and source-backed label
quality, rather than promote this reference to production.
