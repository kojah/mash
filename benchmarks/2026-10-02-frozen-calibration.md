# Bounded, globally frozen topic calibration

The first run improves several scores over no-IDF TF, but no frame passes the
prespecified recurring-loss target in both insertion orders. This single
bootstrap setting is not ready for promotion. Do not sweep the window size to
make the .020581 filtered forward loss pass a .02 gate, or imply that a small
numerical miss is statistically significant.

## Frozen design

`research/topics/CALIBRATION_PROTOCOL.md` was fixed before outcomes. First32
immutable physical source blocks, in forward captured meeting/source order,
form one bounded initial-index bootstrap. Source-only term DF counts one
occurrence per used term per block; smoothed IDF is `ln(33/(1+DF))+1`, unseen
terms DF0. No later update to weights or previous coordinates. Frozen source
identities/count digest, policy/encoding descriptor and DF table identify a
generation. Canonical ascending CSR indices, scalar term weights, fsum bucket
coalescing/L2 norm. Original marker/filler filter/empty/fallback rules remain.

Same one model across all162 meetings, rather than fitting a model per meeting.
Raw and static-filtered policies, positive SHA25616384 hashed weighted vectors
and collision-free term-identity oracle, rotation-only exact-neighbor average
linkage, forward/reverse source insertion:1296 trees. Calibration is forward
in both orders; reverse is insertion robustness after bootstrap, not a reverse
causal streaming timeline. Archive order is reproducible and arbitrary, not a
verified chronological production node-group ordering. No window/threshold
grid, future weight fitting, new inference, held-out data, LLM or Rust changes.

Every original source remains:88336 turns,84434 ready,77892 singleton-known,
8196 blocks. Full all/far/recurring pair support7978214/7923483/84323. Additional
post-bootstrap pair scope requires both blocks outside the first32, but every
LCA denominator still includes all original singleton-known turns, including
bootstrap. Matched offline and fixed-TF control trees use that same support;
bootstrap mistakes remain in full-support results. Post all/far/recurring pairs
7958914/7904344/84323. Recurring support is unchanged because the bootstrap
meeting contributes no cross-span positives; it is not newly held-out quality.

The loader validates frozen original embeddings/IDF metadata for count/source
lineage. It does not use future IDF or embeddings to compute this candidate's
features. The model only consumes the first32 source count dictionaries.
That extra loaded evidence also affects research setup/memory, not model size.

## Verified results

Offline TF-IDF average reference:all .554138, recurring .622208.

| Frame | All forward / reverse | Recurring forward / reverse | Recurring loss forward / reverse |
| --- | --- | --- | --- |
| Raw calibrated hash | .511644 / .517417 | .624879 / .597167 | -.002671 / .025041 |
| Raw calibrated identity | .519277 / .524224 | .615330 / .598770 | .006878 / .023438 |
| Filtered calibrated hash | .539608 / .527573 | .601627 / .646421 | .020581 / -.024213 |
| Filtered calibrated identity | .538813 / .527926 | .598151 / .646276 | .024057 / -.024068 |

Every frame fails necessary recurring loss<=.02 both orders. Raw fails reverse;
filtered fails forward, including identity oracle. The filtered forward broad
losses .014531/.015326 are lower than the no-IDF losses, but filtered reverse
losses .026565/.026212 and raw losses .029914–.042495 remain visible. This is not
a demonstrated robust semantic hierarchy. Post-bootstrap all/far scores change
slightly; recurring scores are identical on identical positive pair support.

Capture all trees, model generations, weighted matrix files, states, full/post
metrics, matched baseline metrics, hash distortions and labels/representatives.
The eight-group diagnostic display/c-TF-IDF labels are unchanged from the
fixed-TF protocol. This isolates weighting exploration from semantic-cut work;
there is no claim that the cut has been fixed or labels independently judged.
First meeting filtered hashed forward has a61-block group with terms
`just/gap/think/right/say/like/remote/don`, a four-block group with
`believe/point/blank/euros/percent/leave/long/gonna`, singleton `consumerism`,
the two-block `screen/bigger/dial/make` group, and three empty-label groups.
Those actual outputs remain too generic/uneven for promotion.
The two seed DF tables contain508 raw /368 filtered terms; hash versus identity
mean absolute cosine error .002780 /.001675, max .306739 /.392058 across246924
distinct block pairs. Sparse collision tails remain material diagnostics.

## Verification scope

Six fixtures cover immutable seed DF/unseen weights, future arrivals not
changing old rows/model, fallback/empty ownership, seed identity/kind fingerprints,
full original denominators in post-bootstrap LCAs and zero-bootstrap parity.
Enabled lab suite526 tests passes2.366s. Source modules:
`calibrated_frame.py`, `calibration_metrics.py`, `qmsum_calibration.py` and the
independent `audit_calibration.py`. No application/Rust validation claim.
Final post-corpus validation also passes all526 tests in3.118s, run alongside
the manuscript build; those times are not a test-suite performance comparison.

Provenance correction: the original first job reached terminal zero, but an
unused NumPy import was removed while it was live and then restored. Its
end-of-run helper fingerprint therefore did not match the next run. The second
job correctly rejected the complete repeat, terminal one; all saved feature
matrix files were identical, but full repeat was not proven. Original artifacts
remain untouched. Do not waive or remove code pins from comparison.
The driver now captures pins before its main experiment and rejects changes at
the end. Two fresh final runs under frozen source both reached terminal zero;
complete non-timing repeat matches, including every bank file, generation,
tree/state/label/representative and full/post/control metric. This source guard
changes provenance validation, not thresholds, weights or quality gates.

Independent audit reached terminal zero:1,868,302 weighted coordinates,
1296 calibrated trees plus1458 matched baseline trees,542,871,217 source-pair
contributions across full/post scopes,69179 label terms and10368 representatives.
It reconstructs original bootstrap DF/count digests, scalar weighting/hash
coordinates, original native source memberships, physical LCAs, post-bootstrap
pair masks with full original denominators, matched controls and summaries,
source-word label scores/ranking and representative support. Coordinate errors
must be <=1e-12; maximum other numeric discrepancy2.91e-11 is in accumulated
pair sums, within the declared sum tolerance (not a cosine tolerance relaxation).
Exact representative IDs replay the frozen mean operation, with separate
pairwise max/score checks.415 reassociation winner differences retained,
maximum gap2.11e-15. Shares frozen count ledger, NumPy/SciPy/static stoplist;
not independent source tokenizer/rotation choices or node aggregate fingerprint
audit, no semantic or chosen Rust parity claim.

Final artifacts under `/home/james/scratch/mash-artifacts`:

| Artifact | SHA256 |
| --- | --- |
| `topic-qmsum-calibration-final-first-20261002/results.json` | `d99fe9802bb958211a6055c1222b26546da9d2914db2cb0a0f0c843385c64305` |
| `topic-qmsum-calibration-final-second-20261002/results.json` | `ec349cb750eb3efab2818b3a2d940203ec495f84e3d51dcc8d7e28f11f8eecef` |
| `topic-qmsum-calibration-audit-final-first-20261002.json` | `bab764002368359c190fee34821e6ef2c93926b4acc3e6fe789e83c0d1f0bd61` |
| Audit source | `6056153ae488a44e267cfb424d355eb39db3fb175f87b075310feac6366a4759` |
| `topic-qmsum-calibration-final-costs-20261002.json` | `e59cb391078c8ce27949a625f0876339ae0f68cd2a47ca1c8c245046a916084e` |

## Cost scope and reproduction

Final first/repeat after-import wall182.689 /175.714s, CPU155.407 /157.178s,
setup15.339 /14.824s. Two model builds12.077 /11.614ms. Four full feature banks
encoding/persistence13.448 /20.545s wall,13.416 /16.036s CPU; not single-production
frame or end-to-end ingestion. Raw hashed fit CPU9.912 /9.835s forward/reverse
first and9.805 /9.755s repeat; insert p95 wall2.721 /2.722ms and2.641 /2.515ms.
Filtered hashed CPU8.481 /8.526s and8.557 /8.588s; p95 wall2.040 /1.987ms and
2.108 /2.157ms. Batched completed-tree labels1.807–2.062s per hashed full-corpus
frame/order first,1.673–1.726s repeat, not online maintenance.

Target includes imports, large source/count/label and old full-control ledgers,
frozen embedding validation, all feature banks and tree states, original-source
metrics, labels and final JSON/repeat comparison. Main timer excludes imports/
final JSON/repeat comparison; external includes them. Rusage2354308 /2354496KiB;
first sampled RSS/PSS/statusHWM2355596 /2347615 /2355596KiB; repeat2355128 /
2343905 /2355128KiB. Samples4271 /4222, vanished1 /0, median/maxgaps39.296 /
220.248ms and40.540 /169.107ms. Includes target only, excludes observer/children;
intrusive sampled lower bounds, kernel status approximate, launcher high water
possible, no peak subtraction or model-worker/topic-state attribution. Final
benchmarks sequential with no intentional concurrent scientific benchmark;
uncontrolled host load, no run-variation speed claim. Source mistake cost two
additional original attempts; final metrics/costs retain only the verified pair,
and original attempts remain separately preserved and disclosed.

```sh
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-graph-python-env/bin/python \
  research/topics/memory_audit.py --output FRESH_MEMORY_JSON -- \
  /home/james/scratch/mash-artifacts/topic-graph-python-env/bin/python \
  research/topics/qmsum_calibration.py \
  --artifacts /home/james/scratch/mash-artifacts --output-directory FRESH_DIRECTORY
```

Repeat adds `--compare FIRST_DIRECTORY`. Audit takes `--artifacts`, `--capture`
and fresh `--output`. All scientific/source artifacts remain outside Git.

No production method selected. Bounded bootstrap adds a warm-up requirement;
server-coordinated group generations/order, calibration privacy/forget,
incremental edits/deletes/restart and bounded global costs are not implemented.
A calibration change requires explicit new generation/rebuild/publication,
not reweighting existing aggregates. Independent coding-agent labels/multi-tags/
ancestors/guided search, semantic topic cuts, complete-ingestion<=10percent loss
and chosen Python/Rust differential CI remain separate promotion gates.
