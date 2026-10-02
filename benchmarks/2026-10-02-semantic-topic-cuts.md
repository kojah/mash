# Frozen coherence cuts and source-supported labels

Exploratory training-corpus diagnostic, not a selected production method.
Protocol: [SEMANTIC_CUT_PROTOCOL.md](../research/topics/SEMANTIC_CUT_PROTOCOL.md).
Bead `mash-c0d.19.2.20.8.3.3.3.8.2` remains open for the full source-pair,
label, representative and summary audit. The geometry audit below is narrower.

## Setup

Reuse unchanged fixed-TF and frozen-32-block calibrated filtered-hashed rotation
trees, both insertion orders. No new inference, downloads, LLM calls, held-out
reads or threshold/window sweep. All 162 meetings, 88,336 original turns,
84,434 ready sources, 77,892 singleton-known sources and 8,196 physical blocks
remain accounted for. Raw cached MiniLM directions provide a separate semantic
view, not a learned projection or replacement embedding model.

Compare fixed-eight and semantic cuts, each with static-noise c-TF-IDF and
source-supported corpus-IDF labels: 16 comparisons, 2,592 meeting records.
The semantic cutoff is meeting distinct-block cosine mean plus population SD,
with the frozen numerical margin. Groups lacking two label terms stay captured
as proposals but do not become usable tags. Corpus DF is initial-snapshot
information, not a causal online label-update implementation. Source-positive
pair denominators stay 7,978,214 broad, 7,923,483 far, 84,323 recurring even when
sources abstain. Unknown/mixed sources remain in geometry/coverage. Precision
is conditional source-pair precision, not label accuracy.

## Usable distinctive-label results

| Frame/order | Cut | Broad precision | Recall | F1 | Ready-source coverage |
|---|---|---:|---:|---:|---:|
| Fixed TF/forward | Eight | .316569 | .734612 | .442465 | .940545 |
| Fixed TF/forward | Semantic | .556887 | .055474 | .100897 | .386290 |
| Fixed TF/reverse | Eight | .311277 | .721317 | .434884 | .937466 |
| Fixed TF/reverse | Semantic | .571430 | .045556 | .084384 | .374671 |
| Calibrated/forward | Eight | .330731 | .697107 | .448621 | .944477 |
| Calibrated/forward | Semantic | .597640 | .053415 | .098065 | .394734 |
| Calibrated/reverse | Eight | .320321 | .648645 | .428859 | .943139 |
| Calibrated/reverse | Semantic | .599730 | .049011 | .090617 | .381493 |

Recurring recall is .041543–.086951 for distinctive semantic cuts versus
.473003–.700307 for eight-group controls. Static labels show the same large
coverage/recall collapse, so this is not simply the distinctive-label filter.
Both proposed and usable metrics and fragmentation are retained in artifacts.
No new quality gate is inferred from these numbers; unchanged uncut gates
remain failed.

First calibrated-forward meeting: corpus IDF changes the generic 61-block
group's words to yellow/criteria/gap/bright/colour/say/l_c_d_/kay. Smaller
semantic groups include prototype/evaluation/criteria and
special/integrated/wheel/colour/scroll, but also pretty/right and
stolen/remotes/yellow/right/don/really. A repeated term is not necessarily a
meaningful label. Private source representatives remain outside Git.

Interpretation: reject promotion of this cut rule. Better conditional precision
does not compensate for losing almost all broad positive pairs. Do not tune
the threshold to these examples or accumulate bad-word exclusions. Revisit
discovery units and potentially separate online assignment from taxonomy
discovery/refinement. Global multi-topic thread tags and semantic ancestry
are not established by disjoint meeting-local cuts.

## Reproduction and audit

Artifacts under `/home/james/scratch/mash-artifacts`:

- `topic-qmsum-semantic-cut-first-20261002.json` SHA256
  `3d61c0a7632906892876f8bd5b15dd1298acf610e76ada1d2f9de9c45b814d47`.
- Repeat `topic-qmsum-semantic-cut-second-20261002.json` SHA256
  `b5d66d1412dcdefc8a5969b24cb38733007d8902cf104fd2b5d659479a2805e4`.
  Every non-timing field matches exactly; begin/end code pins are unchanged.
- `topic-qmsum-semantic-cut-geometry-audit-20261002.json` verifies all
  648 semantic trees, 64,920 node scores and complete cut/unassigned decisions.
  Independent direct-pair means differ at most 1.5543122344752192e-15 from the
  aggregate identity. Auditor source SHA256
  `a172d8d85ec3057d0cd0d7b75c4939296b91da3ba68ed73eafe49a62f18e31b4`.
  This shares frozen vectors/NumPy dots and does **not** verify source-pair
  metrics, label counts/ranks, representatives, summaries or tree construction.
- Full enabled lab suite: 538 tests pass in 2.444 seconds. Scalar label-score
  and aggregation fixtures pass; these do not replace the missing corpus oracle.

Use the scientific Python environment with one BLAS/OpenMP thread:

```bash
env PYTHONDONTWRITEBYTECODE=1 OMP_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 MKL_NUM_THREADS=1 \
  /home/james/scratch/mash-artifacts/topic-graph-python-env/bin/python \
  research/topics/qmsum_semantic_cut.py \
  --artifacts /home/james/scratch/mash-artifacts --output /outside/git/fresh.json
```

Use `--compare /path/to/first.json` for a full non-timing repeat and
`memory_audit.py --output /outside/git/fresh-memory.json -- COMMAND` for Linux
target-process RSS/PSS sampling. The audit accepts `--artifacts`, `--capture`
and fresh `--output` paths.

## Costs and limits

First/repeat after-import wall 58.642/60.862 seconds, CPU 50.177/51.555,
setup 18.445/19.390 seconds. Target loads and validates original large frozen
ledgers/raw-vector evidence, computes block counts/background, all cuts,
labels, pair metrics and representatives. No models are inferred. The main
timer excludes imports, final JSON and repeat comparison; external monitoring
includes these. Repeated `cut_ms` entries describe one shared cut computation,
not independently additive per-profile work. No complete-ingestion claim.

Rusage peak 2,628,820/2,628,436 KiB. External RSS/PSS maxima
2,629,772/2,622,023 and 2,629,136/2,621,049 KiB; approximate status HWM
2,628,820/2,629,136 KiB. Samples 1,479/1,532, vanished reads 2/1;
median/max sample gaps 41.733/86.306 and 41.722/75.586 ms.
External scope is one target, excluding observer/children; monitoring is
intrusive, sampled maxima are lower bounds and kernel status counters are
approximate. Rusage may retain launcher high water; no subtraction or worker
attribution. Runs are sequential; a brief first-result inspection overlaps
repeat setup. Host load is uncontrolled; this is not a timing comparison.

Full corpus label/pair/representative/summary audit remains open, as do
independent coding label/multitag/ancestor/guided-search evaluation, global
online identities/bounded state, edits/deletes/restart, complete ingestion
overhead and selected Python/Rust differential CI. No Rust/application or
deployment behavior changes in this experiment.
