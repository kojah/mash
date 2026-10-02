# Source-preserving FISHDBC reference evaluation

Decision: reject this frozen operating point for production topic discovery. It
misses the recurring-topic loss target even with exact neighbors. This does not
reject all density methods, FISHDBC calibrations or its scaling claims. No
production topic method is selected.

## Frozen procedure

[FISHDBC_PROTOCOL.md](../research/topics/FISHDBC_PROTOCOL.md) was written before
corpus outcomes. [Paper](https://arxiv.org/abs/1910.07283) and
[author source](https://github.com/matteodellamico/flexible-clustering) describe
an incremental density hierarchy. Pin unmodified author commit
`bf73d1ac6ce8bcdb21232978e2deb99d2c2582c6`, tree
`3b4dd826c4722a0c05935721d6e665aba6da601b`, BSD-3-Clause. Reference manifest
`bf4d0261f789837206eb752712658e24c4e0033a6da99a4c0079728e160105b3` hashes
all tracked files and generated native unionfind extension. Python3.11.15,
NumPy2.4.6, SciPy1.17.1, sklearn1.9.1, hdbscan0.8.40, Cython3.0.12,
setuptools80.9.0. Source had to be compiled before import; no author patches.
Manifest/native binary are host-specific scientific reproducibility evidence,
not a portable application dependency or chosen Rust parity contract.

Same 162 training meetings/88,336 source/84,434 ready/77,892 singleton-known
turns/8,196 immutable source-only blocks; unknown/multi stay in geometry. Lexical
sublinearTF-IDF primary, unchanged dense a control; no compression/projection,
parameter grid, new inference/download, LLM, or heldout. Both original arrival
orders; author min_samples5/HNSWm5/ef50/defaultm0/level multiplier, heuristic and
balanced_add true, scalar callback. Reset Python random seed20261002 per
meeting/view/order, restore caller state. Extract EOM/min_cluster_size5/no single
cluster after each16 adds/final remainder, first only when>=6. Final single-linkage
source trees use all blocks; condensed/flat noise is reported separately.

Distances are precomputed clipped1-cosine/zero diagonal. Callback timers therefore
omit realistic online vector access/distance computation; feature matrices,
small9–134-block per-meeting graphs and full-training IDF are offline. Exact
diagnostic uses fifth OTHER neighbor (matching author self-exclusion), complete
mutual-reachability graph and independent Kruskal sorted by weight/arrival IDs/raw
distance. Author and exact control share hdbscan tree labeling. Equal MST weight
need not imply identical tied topology; neither tree implies semantic ancestry.

All/far/crossspan original-source conditional LCA purity, excluding same-original-
block positive pairs, matches immutable lexical average reference. Internal108/54
partitions already explored, not pristine heldout. Capture prefix labels/noise,
probabilities/stabilities/condensed trees/merges/MST, final native cores/graphs,
source trees/metrics and all source/code/reference/protocol pins.

## Results

| View/order | Author all purity | Author return purity | Exact return purity | Return loss vs .622208 | Flat noise /8196 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Lexical forward | .447250 | .580574 | .580574 | .041634 | 7,974 |
| Lexical reverse | .447635 | .579924 | .579924 | .042284 | 7,974 |
| Dense forward | .427659 | .536166 | .536166 | .086042 | 8,114 |
| Dense reverse | .427544 | .535876 | .535876 | .086332 | 8,114 |

All/far/crossspan positive pairs remain7,978,214/7,923,483/84,323; returns29 topics
in21 meetings. Reference lexical average all/far/return .554138/.553862/.622208;
chronological balanced .815016/.814376/.576852; root .387068/.387531/.529726.
Lexical FISHDBC slightly exceeds chronology on returns but loses to lexical
average and fails provisional loss<=.02 in both orders. Default flat noise97.29%
lexical/99.00% dense is an additional usability warning, not an independently
labeled noise error rate. Do not hide noise sources or score only retained clusters.

Exact cores maximum excess .020802/.036701 lexical and .050329/.031890 dense;
native total MST weight excess .194324/.245014 and .412468/.295447. Exact return
purity is identical lexical/forward dense, reverse dense differs2.53e-9; all-purity
differences are at most1.05e-5. Approximation is not the primary deficit in this
specific control; exact neighbors would not rescue its quality target. Distance
evaluations245,557/245,074 lexical and243,232/243,423 dense, versus246,924 unique
per-meeting possible pairs. These small graphs do not demonstrate ANN scaling.

## Reproducibility and independent boundary audit

`fishdbc_reference.py`, `qmsum_fishdbc.py`; four fixtures cover tied exact MST and
self-exclusion, mapped source ownership/malformed trees, invalid distances,
prefix schedule/full determinism/caller randomness and native edge payload
semantics. Author fixture requires explicit `MASH_TOPIC_FISHDBC_REFERENCE` and
otherwise skips. Enabled complete lab **505 tests pass**. No Rust/app changes.

Outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-fishdbc-first-20261002.json`, SHA256
  `e55ec5a067396b66f381921c6e5a34acb688aac5edfedc88c13665a4733c911c`.
- Fresh repeat `topic-qmsum-fishdbc-second-20261002.json`, SHA256
  `2aceb8a5774e11b1e6ffe7ab204f851543d28f2bcf89b9dd70ee2ebab7c3d7b7`.
  Every non-timing field exact, including all prefix outputs, native cores/graphs/
  MSTs, exact controls, complete trees/source metrics/split summaries and pins.
- `topic-qmsum-fishdbc-audit-20261002.py`, SHA256
  `ea299ba9932ca9e8a9a76b8da9055882321664468e4b4a870ab2e9086e1d72ad`;
  `topic-qmsum-fishdbc-audit-first-20261002.json` verifies true.
  No lab helpers: reconstruct original-source memberships/counts/lexical vectors,
  exact scalar neighbor cores/complete Kruskal, graph distances/native edge weight
  bounds, source mappings/prefix coverage, direct positive source-pair LCAs and
  split summaries. 1,296 tree cases,127,888,160 pair-scope contributions,
  379,513 graph/MST checks, maximum numeric error0. Shared frozen inputs and
  NumPy/SciPy sparse arithmetic; not independent HNSW or hdbscan-label replay.
- `topic-qmsum-fishdbc-costs-20261002.py` and
  `topic-qmsum-fishdbc-costs-first-20261002.json` preserve detailed cost evidence.
  Driver SHA256
  `a5e70d88da015f68ab91a8dea2395ea102204111000959c485fa87b659347fa5`.

The auditor initially assumed native MST fourth field was raw distance, as
described in the author comment, and failed on .763913 versus .772852. Source
review shows pending core-distance decreases can store reachability in this
payload. Correct audit checks payload>=raw and weight>=payload/current cores;
actual graph distances remain separately checked against raw. A deterministic
fixture reproduces four such payloads. Original capture remains unchanged; this
is an audit-assumption correction, not a scientific or author-code repair. A
future Rust port must not decode that payload as raw distance.

## Costs and limits

One numerical-library CPU thread. Offline first/repeat after-import wall54.459/
40.022s, CPU38.397/34.089s, setup27.165/16.386s; feature-distance356.609/315.617ms.
Lexical forward/reverse author3.179/2.883s first,2.740/2.476s repeat; dense2.883/
3.147s and2.422/2.664s. Lexical add sums1.907/1.947s first,1.665/1.697s repeat;
extract sums.893/.897s and.720/.745s. Each view/order587 extractions, lexical
addp95 .492/.501ms first,.408/.421ms repeat; extractionp95 3.672/3.639ms and
2.652/2.980ms. Author timers include prefix output capture; exclude validation/
manifest imports and final exact diagnostics. These are precomputed-distance,
small-graph reference timings, not complete ingestion or large global updates.
No intentional scientific benchmark overlap, uncontrolled host load; no speed
conclusion from variation. Imports/final JSON/repeat comparison outside stage
timers, inside external observations.

Rusage first/repeat2,077,192/2,076,552KiB; external RSS/PSS/statusHWM
2,078,728/2,070,961/2,078,728 and2,077,632/2,069,853/2,077,632KiB.
1,343/1,275 samples,0/1 vanished reads, median/max gaps42.719/258.233 and
37.651/89.183ms. Target includes scientific imports/large source ledger, excludes
observer/children; intrusive sampled maxima lower bounds, status approximate,
launcher high water possible, no subtraction. No persistent-state/worker claim.

No coding label/multitag/semantic-ancestor/guided-search gate or global provider/
node-group/lifecycle/persistence/full-ingestion/Python-Rust parity is established.
Reject this operating point without spending on production integration. Next
test an online average-link lexical block tree, preserving the successful
lexical-reference linkage rather than density single-linkage. Freeze a new
protocol; earlier small exposed-anchor grafting studies are not this corpus/
source-block control. Sparse features must be handled without future-dependent
projections or changing the frozen old prototype contracts.
