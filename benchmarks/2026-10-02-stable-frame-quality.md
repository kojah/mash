# Fixed online lexical-frame quality control

This operating point is unsuitable for promotion. Neither marker-only nor static
noise-filtered TF preserves the necessary recurring-topic target in both arrival
orders. Collision-free TF also fails; increasing hash capacity alone cannot fix
the observed deficit. This is a result on exploratory meeting annotations, not a
judgment of all feature hashing, all online clustering, or coding-agent topics.

## Frozen setup

`research/topics/STABLE_FRAME_QUALITY_PROTOCOL.md` extends the unchanged encoding
contract frozen at `bee25b2`. No threshold/stoplist/hash-capacity grid was run.
Use all 162 QMSum training meetings, 88,336 source turns, 84,434 ready source
turns, 77,892 singleton-known source turns and all 8,196 immutable physical
blocks. Unknown and mixed-topic sources remain in geometry. Neither topics nor
query annotations become features or determine cuts. No new inference, model
downloads, LLM calls, held-out data or production changes.

Four frames: raw/filtered SHA256-16,384 nonnegative hash TF and corresponding
distinct-term TF controls. The latter use the old vocabulary solely as a term
identity oracle, without learned weights; a coordinate permutation leaves their
geometry unchanged. The filtered control retains 118 flagged raw-geometry
fallbacks. Every tree uses rotation-only exact-neighbor average linkage, forward
and reverse source order: 1,296 completed trees. Tiny-norm guard repair from
`bee25b2` is pinned; historical offline-IDF online controls retain `1d2a35b`.

Evaluate each original positive source pair at its physical-block LCA, with
singleton-known source counts as conditional class fractions. Exclude same-block
pairs. The all/far/crossspan scopes retain exactly 7,978,214 / 7,923,483 / 84,323
pairs. Recurring scope uses original gap >4 and distinct native topic spans;
it supports 29 topics in 21 meetings, not an exhaustive multi-topic hierarchy.
The immutable offline TF-IDF average-link reference is .554138 all / .553862 far /
.622208 recurring. Its learned full-training IDF is an offline comparator.

## Tree quality

| Frame | All forward / reverse | Recurring forward / reverse | Recurring loss forward / reverse |
| --- | --- | --- | --- |
| Raw hashed TF | .474494 / .483193 | .598382 / .585379 | .023826 / .036829 |
| Raw identity TF | .478710 / .483865 | .604296 / .581059 | .017912 / .041150 |
| Filtered hashed TF | .509198 / .499559 | .604523 / .590431 | .017685 / .031777 |
| Filtered identity TF | .509377 / .498971 | .608917 / .593786 | .013291 / .028422 |

Every frame fails necessary recurring loss <=.02 in both orders. All-scope loss
is .044761–.079644, also not a broad-topic pass. Filtering improves these
aggregate scores versus raw TF, but there is no independent coding-agent or
statistical winner claim. Hashed-versus-identity differences are small in the
aggregates and are not consistently signed. We must not infer collision safety
from that: across 246,924 distinct physical-block pairs, mean absolute cosine
error is .002158 raw / .001623 filtered, maximum .235702 / .408248. The collision
surplus statistics from preflight alone would not reveal those tail errors.

The previous offline-IDF rotation-only controls scored .624317 / .603018 on
recurring topics. Comparing them with this TF control suggests weighting is
material in this corpus; it does not isolate all causes or prove a suitable
online IDF strategy. Their all-scope losses remained .025813 / .021120, so
restoring offline IDF is not sufficient evidence for selection.

## Labels and representatives

Capture all trees and all display groups, not a selected favorable subset.
For reproducible inspection only, split the largest leaf-population node until
min(8, block count) groups; ties use original membership lists. Never use gold
topic count. Existing c-TF-IDF labels use original unhashed source counts after
the fixed English/acknowledgement exclusions, within that meeting and cut.
Geometry fallbacks grant no invented label evidence. Rank top eight positive
terms with lexical ties. Select a representative original block by maximum dot
product with unnormalized group mean, earliest source block on a tie.

The first meeting's filtered hashed forward tree has a broad 64-block group
whose terms include `just`, `gap`, `think`, `right`, `say`, `like`, `remote`,
`gonna`. Other groups include singleton `consumerism`, singleton `fair`, a
two-block `screen` / `bigger` / `dial` / `make` group, and three groups without
label terms. These actual outputs exhibit generic language, highly uneven
groups and insufficient labels; they are not robust user-facing topics.
The eight-group display cut is a diagnostic, not a calibrated semantic cut.
Do not add a bespoke stopword for each displayed failure or present inherited
geometric ancestors as meaningful semantic parents.

## Reproducibility and limits

Executable runner: `research/topics/qmsum_stable_frame_quality.py`.
Independent source audit: `research/topics/audit_stable_frame_quality.py`.
Four new fixtures cover empty/fallback ownership and unchanged source counts,
nonzero hash collision errors, orientation-independent complete display cuts,
and label abstention with original representative identity. Enabled lab suite:
520 tests pass in 2.474s. No Rust/application change or application test claim.

Both monitored runs and the corrected audit reached terminal exit zero.
Repeat matches every non-timing field, including complete trees, ownership,
numeric node fingerprints, counters, labels, representatives and source text.
The independent audit verifies 1,296 trees, 127,888,160 pair-scope contributions,
67,771 label terms and 10,368 representatives, maximum numeric discrepancy
2.89e-15. It independently rebuilds native source membership/LCA contributions,
cut ownership, scalar label formulas/ranking and pairwise representative scores.

The initial auditor incorrectly required identical representative IDs under
reassociated pairwise summation. The next replay exposed equal integer counts
in different CSR layouts changing a normalized TF value by one ULP: meeting0
raw identity forward blocks66/69 changed winner with score gap2.22e-16.
Correct validation replays the frozen CSR incidence layout and mean-vector
operation for exact IDs, separately verifying equal source-count values and
independent pairwise score/max agreement within the existing 1e-12 numerical
tolerance. It records all347 reassociation winner differences, maximum pairwise
score gap2.33e-15. Original runs/labels were not mutated or rerun to conceal this.
This is shared numerical operation replay, not independent representative-ID
algorithm parity. Canonical sparse layout, reduction and ties must be frozen
before selecting a Rust contract; no semantic/quality gate was relaxed.

External artifacts under `/home/james/scratch/mash-artifacts`:

| Artifact | SHA256 |
| --- | --- |
| `topic-qmsum-stable-frame-quality-first-20261002.json` | `722d9acb930c8e0bd6df163c5ad81687f88df73a826da46b1a0f1e0ea79ee741` |
| `topic-qmsum-stable-frame-quality-second-20261002.json` | `d150d8637f9fe474d39fef9a51ec6e43dc66012a6b0fe15ac8e552351abd8b8d` |
| `topic-qmsum-stable-frame-quality-audit-first-20261002.json` | `c5636300eb41a0f71facf994339bbf899ed598e04e672ab6bde8e48cc829c006` |
| Audit source | `80c4e62b43309ab25d598b0f91864e2aaf3b7c135bc068ce346c5fd76b0e1f3e` |
| `topic-qmsum-stable-frame-quality-costs-20261002.json` | `b3725dbd4ee12d58cf67afb70102973341733da267598a7614a0d91fa4c26eb9` |

The capture pins encoding preflight, geometry, source, offline reference and
all algorithm/label/protocol dependencies. Audit shares frozen source-count and
encoded banks, NumPy/SciPy/static sklearn stoplist. It does not independently
replay tokenization/hashing or rotation choices; separate encoding audit and
tree fixtures address those boundaries. It is not semantic or Rust parity.

## Cost scope

First/repeat after-import wall152.661 /143.189s; CPU148.313 /139.284s, setup8.619 /
9.634s. Raw hashed forward/reverse fit CPU11.449 /11.265s first and10.825 /10.698s
repeat; insert p95 wall3.032 /2.836ms and2.791 /2.670ms. Filtered hashed CPU8.916 /
9.081s and8.432 /8.371s; p95 wall2.091 /2.123ms and1.944 /1.929ms.
Completed-tree label generation sums1.53–1.59s per full-corpus frame/order first,
1.43–1.47s repeat. This is a batched diagnostic label pass, not online label
maintenance. Tree validation adds3.84–3.98s first and3.53–3.72s repeat per case
bank. Full cost ledger also contains identity-control timing.

Target scope includes imports, large captured source/count/label ledgers, all
four feature banks, tree states, source-pair evaluation, diagnostic labels and
final JSON serialization. Main timer excludes imports/final JSON/repeat
comparison; external memory includes them. It excludes children/observer.
Rusage1521528 /1521336KiB; sampled RSS/PSS/statusHWM1512176 /1504671 /1521528 and
1516328 /1504716 /1521336KiB. Samples5070 /4979, vanished0 /2, median/max observed
gaps30.184 /60.080ms and29.405 /70.240ms. Monitoring is intrusive, sampled peaks
are lower bounds, kernel status approximate, launcher-retained high water
possible; no peak subtraction or model-worker/topic-state attribution.
Runs were sequential with no intentional concurrent scientific benchmark. One
short first-result JSON inspection overlapped repeat setup; host load otherwise
uncontrolled. Do not infer a speed gain from run variation. No persistence,
edit/delete/restart or production ingestion throughput gate is passed here.

Reproduction uses the pinned scientific environment and one numerical thread:

```sh
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-graph-python-env/bin/python \
  research/topics/memory_audit.py --output FRESH_MEMORY_JSON -- \
  /home/james/scratch/mash-artifacts/topic-graph-python-env/bin/python \
  research/topics/qmsum_stable_frame_quality.py \
  --artifacts /home/james/scratch/mash-artifacts --output FRESH_CAPTURE_JSON
```

Repeat adds `--compare FIRST_CAPTURE_JSON`; audit takes `--artifacts`, `--capture`
and fresh `--output`. Scientific environment/source captures remain external.

Production remains unselected. Source blocks are representation controls rather
than validated semantic episodes. Global cross-provider topic identities,
bounded retrieval/state, semantic cuts/multi-tags/ancestors, edit/delete/restart,
complete-ingestion throughput <=10% loss and selected Python/Rust differential
CI remain unverified. A necessary recurrent score, even if it had passed, would
not satisfy those requirements. Keep topic work P0 and do not port this failed
operating point to Rust or expose it as finished MCP topic behavior.
