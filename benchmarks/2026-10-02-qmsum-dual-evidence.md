# Original-plus-context lexical topic evidence

This follows the [complete causal-context comparison](2026-10-02-qmsum-lexical-context.md).
It tests one frozen representation policy, not a clustering method or production
topic implementation. BIRCH/GRINCH are not commitments; the user permits changing
the approach when quality and performance evidence warrants it.

## Protocol frozen before outputs

Reuse the audited QMSum original-turn/context CSR matrices and complete query
ledger. Required baseline results SHA256:
`e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488`.
Retain all 162 original meetings, 88,336 turns and 41,843 eligible query rows,
including 82 unsupported queries. Use the identical 41,761 supported queries,
same original-turn query vectors, candidate eligibility and +/-four-turn source
overlap exclusion. Preserve unknown/multi-membership support limitations.

For each query, rank the full common candidate set separately by original-turn
and causal-context cosine, with exact ties by ascending original ordinal.
Apply equal-weight reciprocal rank fusion:

`score(identity) = 1 / (60 + original_rank) + 1 / (60 + context_rank)`.

Ranks are one-based, `k=60` is fixed, and no list is truncated before fusion.
Each original source turn remains exactly one candidate. Fused-score ties use
the same source-ordinal rule. Both original baseline rankings and metrics must
match the audited capture for every supported query before reporting a result.
No threshold/grid, annotation feature, inference, download or held-out read.

Report MRR, Hit@1/10, Recall@1/10, plus all per-query improved/tied/worse counts
against each baseline. Resample meetings jointly 1,000 times with seed20261002
for descriptive MRR differences. Retain in-sample, broad-topic and correlated
source limitations; these are not independent coding-topic confidence intervals.

## Results and decision

| Metric | Original | Context | Dual RRF |
|---|---:|---:|---:|
| MRR | 0.644210 | 0.657425 | 0.668580 |
| Hit@1 | 0.500587 | 0.574124 | 0.540456 |
| Hit@10 | 0.920883 | 0.854314 | 0.919782 |
| Recall@1 | 0.004745 | 0.005941 | 0.005314 |
| Recall@10 | 0.033101 | 0.046005 | 0.037707 |

Fusion nearly restores original top-ten hit coverage, with higher MRR and first
hit accuracy than original. It does not dominate: first-hit accuracy and recall
remain below context-only; original Hit@10 is still slightly higher. At equal
top-ten output budget versus original, 1,373 queries improve Hit@10 and 1,419
regress (38,969 tie), a net loss of 46. MRR improves on 11,531, worsens on 9,869
and ties on 20,361 queries. Hit@1 improves on 5,232, worsens on 3,567 and ties on
32,962. Complete per-query directions for all metrics/baselines are retained.

Paired meeting MRR-gain percentiles 2.5/50/97.5 are
0.020880/0.024355/0.027803 versus original, and
0.007730/0.011182/0.014210 versus context. These are descriptive exposed-training
resamples, not independent coding-topic confidence. No noninferiority margin or
statistical equivalence for Hit@10 was prespecified; do not claim either.

Keep both views as a research candidate, not a selected production method. Rank
fusion is retrieval behavior, not evidence that clusters, lexical topic labels
or semantic ancestors improve. The next relevant step is testing source
representations in fixed online topic discovery with label-quality evaluation,
not tuning RRF weights or repeating rank grids on this exposed corpus.

## Reproduction

Driver `research/topics/qmsum_dual_evidence.py`, helper `dual_lexical_evidence.py`.
Four tests check source-identity ownership, full-list contributions, symmetry,
exact ties, and malformed inputs. The full enabled lab suite passes 442 tests.

Use existing science Python and `memory_audit.py --output MEMORY.json -- PYTHON
research/topics/qmsum_dual_evidence.py --source SOURCE --baseline BASELINE_DIRECTORY
--output-directory FRESH_DIRECTORY [--compare FIRST/results.json]`.
Runs and independent audit artifacts stay under `/home/james/scratch/mash-artifacts`.
Baseline dependency/model environment remains pinned; no encoder is invoked.

The separate original-source auditor `topic-qmsum-dual-audit-20261002.py` uses
the pinned successful baseline source/count/context/vector audit, then
reconstructs full candidate ranking, scalar RRF contributions, relevance,
top results, all metrics/directions and paired resampling without importing the
new helpers. Terminal success covers all 88,336 source turns, 41,843 query rows
and 41,761 supported rankings, with zero fused score error. It shares SciPy's
sparse-dot kernel and does not prove inference, independent-kernel or Rust parity.
Auditor SHA256:
`ee25105effc299a4c4acb773fa0ca61049805c7645daabcca2ec6456e615d4ce`.
Prerequisite source/count/vector audit SHA256:
`81a90726da98a803e19ed59fbc881ecade7cdae52854b0a301b6342b4c8503b6`.
Terminal audit report: `topic-qmsum-dual-audit-first-20261002.json`.

Observed research wall/CPU and external target RSS/PSS are diagnostic costs.
Timers exclude repeat comparison and final JSON output; the external sampler
includes them and imports, excludes observer/other-process memory. Sampled peaks
are lower bounds, status approximate, monitoring intrusive, and launcher high
water may persist. This is not complete ingestion or a production-memory gate.

Final fresh first/repeat match every nontiming field. Results SHA256:
`1980ba611cfb6f8867e0882e14741e6b3d26bb590685e418d5c75d61f5a25462` and
`04de1a777ee02c62a535c0f5a06b7fd731b5324e3724cc41a62e802b9f8e5e0a`.
Artifact directories: `topic-qmsum-dual-{first,second}-20261002`.

After-import setup is 2.380/1.757 s, evaluation 21.240/21.442 s, total
23.620/23.199 s, CPU 23.492/22.866 s. Timers stop before repeat comparison and
final JSON. Repeat overlaps the independent auditor and is not a matched
performance comparison. Rusage is 508232/509908 KiB; sampled external full-target
RSS 508916/678248 KiB, PSS 500836/664250 KiB, status high water 508420/683384 KiB.
The repeat loads another complete query ledger after the experiment timer, so
its memory scope exceeds the reported experiment rusage. There are 975/1021
samples, one/zero vanished reads, median/max gaps 28.22/42.66 and 28.55/64.11 ms.
Do not subtract counters or attribute offline audit/comparison memory to ingestion.

Beads experiment `mash-c0d.19.2.20.7`; next actual discovery/label-quality work
`mash-c0d.19.2.20.8`. The broader topic-method decision remains incomplete.
