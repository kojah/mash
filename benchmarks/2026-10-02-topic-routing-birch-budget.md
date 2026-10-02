# Full-corpus BIRCH topic-routing feasibility: leaf budget failure

Bead `mash-c0d.19.2.30.4`, still in progress. The frozen first topic control
cannot finish discovery under its resource budget. No real-corpus topic labels,
overlapping tags, routed rankings or agent-navigation gains were reached. A
successful capture of this failure is not a successful topic method.

## Prospective procedure

Reuse the [verified semantic captures](2026-10-02-duplicate-semantic-retrieval.md)
and complete [window ledger](2026-10-02-semantic-window-fidelity.md), with no
new inference, model download or generative/paid call. Fit all 32,176 candidate
balanced title/body directions, in original-ID forward and reverse order. Query
and duplicate references never enter discovery or label fitting; reserved
query outcomes remain unscored. This public duplicate benchmark is not agent
primary/secondary topic or ancestor gold, and pretraining overlap is unknown.

The [pinned sklearn BIRCH API](https://scikit-learn.org/stable/modules/generated/sklearn.cluster.Birch.html)
exposes incremental CF-tree updates and leaf centers separately from a final
global clustering step. Freeze radius .55, branching factor 50, no global
clustering, `compute_labels=False`, partial-fit batches of 256 and leaf budget
8,192. Retain normalized float64 384-dimensional input without a changing
projection/centering transform. Check the cap after each complete batch; fail
without source truncation. Batch-boundary overshoot is explicit, not a strict
allocation ceiling. These are research settings, not selected product defaults.

The prepared downstream controls, **not reached on this corpus**, are:

- Normalize final leaf means for cosine assignment, with centroid-byte-hash
  snapshot identities for ties. These are not persistent semantic topic IDs.
- Hard nearest-balanced-vector membership versus maximum cosine across every
  original title/body window, with absolute .55/relative .85 cutoffs and up to
  three overlapping tags. Preserve ownership and unassigned documents.
- Route queries through up to three centroid topics, combine posting confidence
  by maximum query-topic cosine times membership confidence, retain at most
  1,000 candidates, and rerank only those candidates by exact balanced cosine.
  No dense prescoring or fallback may hide routing misses. This is automatic
  topic routing, not an agent using labels through MCP.
- Sparse class TF-IDF, multiplied by document IDF, and lexical-containment
  diversity controls on identical memberships, with eight terms and original
  source-span supports. Covered documents split count mass equally across tags;
  IDF sees all candidates, while class average length includes empty topic rows.
  These formulas follow the [class-TF-IDF representation](https://maartengr.github.io/BERTopic/getting_started/ctfidf/ctfidf.html)
  with an explicitly separate corpus-IDF multiplier, not semantic MMR.

Discovery failure prevents evaluating these controls. Direct balanced/BM25
reference metrics remain retained for a later valid comparison; they are not
reported as topic-routing outcomes. No method selection or threshold repair
is performed after the failure.

## Complete lexical prerequisite

Candidate-only source-adjacent vocabulary preserves C++, C#, dotted identifiers
and Unicode. Casefold tokens; trim terminal `.:/-`, retain `+#`; exclude tokens
longer than 128 original characters only from label eligibility. Bigram tokens
must be adjacent through original whitespace. A non-whitespace NUL separates
title/body for counting, preventing invented cross-field phrases. No stopword
list, reference terms or query text is used. Fit document DF min 2/max .40,
frequency cap 32,768, on all 32,176 candidates.

Capture all CSR counts: 32,176 × 32,768, 4,879,216 nonzeros. An independent
Counter/regex pass over the original fields verifies every selected count and
source ID, DF eligibility and frequency-cap ordering. There are 293,891 eligible
unigrams/phrases; the cap boundary frequency is 21. This does not independently
choose the same frequency-tied vocabulary or validate semantic label quality.
No real-corpus class counts, c-TF-IDF scores or labels are generated after discovery
fails. Seven synthetic tests cover technical tokens/Unicode offsets, sparse
mass conservation, formula checks, threshold/tie edges, maximum-window reduction,
no-fallback routing and structured budget reporting. All 355 research tests pass.

## Observed discovery failure

| Arrival order | Inserted at cap check | CF leaf summaries | Unprocessed | Singleton CF summaries | Largest CF summary |
| --- | ---: | ---: | ---: | ---: | ---: |
| Forward original ID | 11,776 | 8,345 | 20,400 | 6,266 | 13 |
| Reverse original ID | 11,520 | 8,297 | 20,656 | 6,316 | 25 |

Only about 36.6%/35.8% of candidates are inserted before the frozen cap fails.
Mean CF sizes are 1.411/1.388; 75.1%/76.1% of summaries are singletons. These
are partial CF compression statistics, not nearest-centroid memberships or
semantic topic counts. No source is quietly dropped and labeled as covered.

The first process explicitly returned an uncaught cap error before routing or
labels, leaving source counts and its exit-1 monitor outside Git. Structured
`DiscoveryBudget` capture now retains every batch trace, unprocessed counts and
empty downstream modes for both orders. This changes failure diagnostics only;
radius, geometry, arrival order and resource cap remain unchanged. Two fresh
diagnostic captures agree on every nontiming field and all source-count files.
Exit 0 means failure recording completed; both candidate records still explicitly
say `discovery-budget-exceeded`. Order-stability metrics are absent, not invented
for incomplete corpora.

Independent prefix replay verifies each growth trace, the first crossing batch,
and leaf-summary count conservation. It uses the same pinned sklearn algorithm,
not an independent BIRCH implementation. CF vector sums match the selected
prefix within 4.72e-12. Maximum squared radii are .302475/.302445, below .55²;
unit-radius identity errors are at most 4.44e-16. Private `_get_leaves()` access
is confined to this pinned outside-Git audit, not a production SDK dependency.

For unit inputs, squared CF radius is
`R² = mean(||x||²) - ||mean(x)||² = 1 - ||mean(x)||²`.
Thus radius .55 requires mean-vector norm at least approximately .8352.
For a group of size n, its mean off-diagonal pairwise cosine must be at least
`(n × (1 - .55²) - 1)/(n - 1)`, approaching .6975 for large groups. This explains
why admitting a close pair does not imply a much broader topic can grow in the
same CF summary. It does not prove the underlying causes of this corpus's
spread or imply Euclidean distance universally fails in high dimensions.

## Costs and limitations

Linux x86-64/AMD EPYC 9645, Python 3.11.15, NumPy 2.4.6, SciPy 1.17.1,
sklearn 1.9.1; OpenMP/OpenBLAS/MKL threads one. Inputs, source/cohort/model
provenance, enumerated code pins and complete sparse arrays are retained.

First diagnostic run: setup 8.620 s, vocabulary/counting 16.606 s, partial
forward/reverse fits .799/.867 s, total 27.037 s after imports. Repeat:
8.666/16.446/.733/.807 s, total 26.804 s. These are monitored research costs,
not full initial/incremental ingestion or a controlled speed comparison. The
sub-second partial fits cannot establish complete-corpus throughput.

External first/second monitors record 846/854 samples, one vanished-address-space
read each, requested sleep 20 ms; actual median/max gaps are 34.37/65.18 and
34.34/61.65 ms. Observed smaps RSS maxima are 1,221,872/1,227,328 KiB, PSS
1,213,759/1,219,480 KiB. Status VmHWM maxima are 1,226,828/1,227,328 KiB;
separate rusage values 1,226,000/1,226,164 KiB. Do not subtract or equate these
counters. Status is approximate and sampled RSS/PSS maxima are lower bounds;
target-only scope excludes children and observer, and sampling can perturb
runtime. Loading source ledgers and retaining arrays contribute to this process.
These are not production worker memory or an ingestion-overhead gate.

## Reproduction and decision

`cqadup_topics.py --root SOURCE_ROOT --semantic FIRST_SEMANTIC_DIRECTORY
--repeat VERIFIED_SEMANTIC_REPEAT_DIRECTORY --preflight PREFLIGHT_JSON
--output-directory FRESH_DIRECTORY`; fresh repeat adds
`--compare FIRST_DIRECTORY/results.json`. Wrap with `memory_audit.py` on Linux.
Captures and the independent audit remain outside Git under mash-artifacts.

- First structured result: `topic-cqadup-topics-budget-first-20261002/results.json`,
  SHA256 `c4903697116d607706934e3f35c8b68ac30e8c85e52842c0a7ac6e4f4d52775c`.
- Repeat: `topic-cqadup-topics-budget-second-20261002/results.json`, SHA256
  `f187300dc3d74589415fda94038052d46dccbcc893d21405f7560641a9934a90`.
- Audit: `topic-cqadup-topics-budget-audit-first-20261002.json`.
- Monitors: `topic-cqadup-topics-budget-memory-first-20261002.json` and
  `topic-cqadup-topics-budget-memory-second-20261002.json`.
- Initial failed output/monitor: `topic-cqadup-topics-first-20261002` and
  `topic-cqadup-topics-memory-first-20261002.json`; retained rather than overwritten.

Reject this **unprojected leaf-as-topic setting** as incomplete under its frozen
budget. This does not reject all BIRCH pipelines or demonstrate GRINCH superiority.
Before another candidate, distinguish CF compression from semantic topic grouping
and labels. A separate fixed/versioned geometry or grouping experiment must be
preregistered and compared with the same direct retrieval controls; do not just
increase the cap/radius to erase this failure. No topic quality, semantic ancestor,
provider generalization, guided-search, lifecycle, ingestion-performance or Rust
parity gate has passed.
