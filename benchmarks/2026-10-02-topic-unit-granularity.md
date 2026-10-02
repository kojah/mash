# Matched dense evidence granularity under average linkage

P0 bead `mash-c0d.19.2.20.8.3.3.3.9`, in progress. This is an exploratory
representation diagnostic, not a selected online topic pipeline.
Frozen [protocol](../research/topics/UNIT_GRANULARITY_PROTOCOL.md).

## Why this comparison

The repository already contains an audited raw-message versus causal-context
fixed-seed experiment: context improved far recall/F1 but precision stayed
around32%. Repeating that experiment would not add evidence. The recent
coherence cut failed badly, but that alone does not show that coarse evidence
is the cause. Change granularity under one fixed offline hierarchy here.

Reuse the existing raw MiniLM vectors on all162 QMSum training meetings.
Compare raw messages, the unchanged causal up-to-five-turn/512-token/gap-stop
normalized vector sum, and unchanged physical16-turn/512-token/gap-stop blocks.
No new inference, LLM, download, learned projection, cutoff/window grid or
held-out source. All arms use cosine SciPy average linkage in original order.
Context is vector aggregation, not concatenated-text encoding or reference
resolution. The original block topology and complete merge sequence replay
exactly; original dense-block metric totals match within frozen1e-12.

Account for all88,336 sources; all84,434 inherited common-ready sources enter
each geometry, while3,902 unsupported sources remain in the source ledger.
Unknown/multi-topic sources remain in geometry, outside conditional gold.
Every singleton-known source contributes once to ancestor class fractions.
Every arm excludes positive pairs sharing an ORIGINAL physical block, so fine
arms do not win by adding easy within-block pairs. All/far/recurring positive
support stays7,978,214/7,923,483/84,323. These are broad meeting annotations,
not independent coding labels or semantic parent/child judgments.

## Results

| Evidence | Broad purity | Far purity | Recurring purity |
|---|---:|---:|---:|
| Raw message | .402046 | .402285 | .546808 |
| Causal context | .433803 | .432418 | .561856 |
| Physical block | .475705 | .475791 | .561768 |

Fine raw messages are worse under this fixed dense hierarchy. Causal context
improves over raw, but broad purity stays below physical pooling; recurring
purity differs from blocks by only.000087861. No significance/equivalence or
coding-quality claim. Do not promote finer evidence solely from these numbers.
This weakens the hypothesis that shrinking physical blocks alone will solve
the failed topics. Generic dense similarity, topic selection, source roles and
distinctive labels still require separate evidence. These scores do not rank
thread-search quality or measure label accuracy.

## Captures and validation

Private artifacts under `/home/james/scratch/mash-artifacts`:

- First `topic-qmsum-unit-granularity-first-20261002.json`, SHA256
  `61e11897f620da4215e5f2305e6605919d1a0e86fe774826d1e8a4598af49294`.
- Repeat `topic-qmsum-unit-granularity-second-20261002.json`, SHA256
  `97bf3e21e638dbb656e2e28778c6a3b725c01ba817139d7955dd00283a00e0ff`.
  Every non-timing field matches, including source/context lineage, topology,
  merge tables, metric/topic/summary totals and begin/end code pins.
- `audit_unit_granularity.py` independently reconstructs original annotation
  spans/memberships, context starts/stop rules, source-weighted LCA fractions,
  pair eligibility, merge populations/mean distances and summaries. It verifies
  486 trees,176,578 merge heights,47,958,060 positive pair-scope contributions
  (repeated across arms/scopes), max numeric discrepancy2.91e-11 in aggregate
  sums within frozen relative1e-12/absolute1e-12 tolerance.
  Source auditor SHA256
  `fe5de1b795c24823d28fef4962f9ec412be407d333a9b4dd4ca7fcad984005ed`.
  Shares frozen vectors and NumPy dot/norm/mean arithmetic; this scoped audit
  does not check global-minimum/tie choice of every linkage merge.
- Separate global-minimum audit uses independently maintained weighted cluster
  distances and the minimum over all active pairs. It allows near ties within
  the frozen1e-12 numerical tolerance, without requiring identical tie choices.
  Terminal success verifies all486 trees/176,578 merge decisions, maximum
  chosen-minus-global-minimum gap0.0. Private report
  `topic-qmsum-unit-granularity-global-audit-20261002.json`, SHA256
  `cd0ffdcc5ff2813a2001770a69a71e484c44c0f79f48d49505276dd6af280dab`.
  Global auditor SHA256
  `49557bad44e0fd89f69c3fc649d63724a39ecd0b6d762604d6c2959a2101c02a`,
  minimum helper SHA256
  `7287e8a970d980c57261a7a0bfa4bfc2169a4f2181fed2189e0c2a592ea6610d`.
  This reruns the source/LCA/height audit and freezes begin/end audit code pins.

Three new fixtures verify common fine/coarse positive support, unknown-source
geometry, empty-support nulls and forbidden coarsening across exclusion units.
Two audit fixtures reject a non-minimum merge/inactive children/bad populations
while allowing ties. Application/Rust behavior is unchanged.
The full enabled lab suite passes543 tests; paper section41 is built and
rendered for inspection. No Rust validation is inferred from Python checks.

## Measured scope

First/repeat wall after imports31.987/29.788s, CPU25.639/24.723s,
setup16.453/15.091s. Across162 meetings, raw/context/block fits total
1.927/1.628/.103s first,1.558/1.573/.102s repeat. These include Gram building
and batch linkage, not online assignment, complete ingestion, labels or
persistence. The target also loads/validates large frozen ledgers/raw evidence,
computes context/metrics and retains complete output. Main timer excludes
imports, final JSON/repeat comparison; external monitoring includes them.

Rusage peak2,084,972/2,084,692KiB. Externally sampled RSS/PSS maxima
2,085,876/2,074,179 and2,085,700/2,074,779KiB; approximate status HWM
2,085,876/2,085,700KiB. Samples915/891, vanished0/1;
median/max gaps35.676/78.744 and35.393/80.868ms.
Scope is one target excluding children/observer; intrusive monitoring and
uncontrolled host load preclude speed claims. Sample peaks are lower bounds,
kernel status is approximate, launcher rusage can retain pre-exec high water;
no subtraction or production-worker attribution. Scientific runs are sequential.

Independent labels/cuts, coding multitag/semantic ancestry/guided-search
evaluation, shared global namespace, bounded online state, edit/delete/restart,
complete-ingestion overhead and selected Python/Rust differential CI remain
promotion gates. Do not reinterpret this batch control as a production design.
