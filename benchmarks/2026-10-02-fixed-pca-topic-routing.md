# Fixed-bootstrap PCA topic routing: complete but poor retrieval

Bead `mash-c0d.19.2.30.4.1`. This prospectively frozen control completes
discovery, unlike the [unprojected control](2026-10-02-topic-routing-birch-budget.md),
but loses substantial relevance and changes with arrival order. Do not promote
it to production or interpret fewer CF summaries as useful semantic topics.

## Procedure

Reuse all 32,176 candidates, 77,915 complete candidate source windows and the
same 64 development queries from the [semantic baseline](2026-10-02-duplicate-semantic-retrieval.md).
No inference, model downloads, generative or paid calls occur. Query text and
duplicate references never enter fitting; reserved query outcomes stay unscored.
Public duplicate links are partial retrieval judgments, not agent-topic,
multi-membership, label or ancestor gold. Pretraining overlap is unknown.

Before outcomes, freeze seed `mash-topic-bootstrap-pca25-v1-20261002`, hash
`seed + NUL + original candidate ID` with SHA256 and take 1,024 candidates in
hash order. Fit 25-component float64 PCA using full SVD, no whitening, on their
384-dimensional balanced unit vectors. [The PCA API](https://scikit-learn.org/stable/modules/generated/sklearn.decomposition.PCA.html)
centers features before SVD. Persist the mean, components, variance ratios and
bootstrap identities; never update this coordinate system beneath existing CF
statistics. This offline hash bootstrap assumes candidate IDs are available;
it is not a demonstrated online cold-start policy.

Feed **unnormalized projected coordinates** to BIRCH: radius .55, branch factor
50, batches 256, leaf cap 8,192, no final global clustering, original-ID forward
and reverse arrivals. Separately normalize projected document/window/query
directions and CF means for cosine assignment. Zero projections abstain without
deleting sources; none occurred. Radius .55 has a different scale after PCA.
Cosine reassignment to CF means is not BIRCH's native Euclidean partition.

Keep the previous hard-balanced and maximum-window overlap controls, up to three
tags, absolute .55/relative .85 cutoffs, and query topic selection unchanged.
Pack posting unions by maximum query-topic cosine times membership confidence,
cap at 1,000, then rerank only selected candidates using **original 384D**
balanced cosine. No full-vector prescoring or fallback hides routing misses.
This prototype packing policy favors topic-central members; it is not an
evaluation of query-relevant vector retrieval inside a topic filter.

Keep all candidate-source vocabulary/counting, equal mass split across tags,
class TF-IDF, separate corpus-IDF multiplication, lexical-containment diversity
and source-span support controls unchanged. These are not semantic MMR.

## Geometry, coverage and retrieval

The retained components explain .441658 of bootstrap variance. Projected
document norms range .275140–.810707, median .602866, p95 .710653; mean squared
norm .364037. A fixed 512-source sample has 130,816 pairs: absolute cosine
change median .136967, p95 .354308, maximum .694226; raw distance ratio median
.633205, p95 .744119. No undefined angles or zero source distances occur in this
sample. Distortion is material, but it does not isolate the cause of routing loss.

Both orders insert all 32,176 sources. Forward has 20 CF summaries, reverse 11;
largest native CF summary sizes are 3,011/5,448. Maximum squared CF radius is
.251653/.292653, below .55². Native CF counts conserve inserted sources.
Nearest-cosine memberships below can differ from native CF sizes.

| Order / membership | Assigned documents | Multiple tags | nDCG@10 | Recall@100 | Candidate recall@1,000 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Forward / hard | 32,176 | 0 | .108054 | .238021 | .305729 |
| Forward / overlap | 24,704 | 5,121 | .124828 | .230729 | .282813 |
| Reverse / hard | 32,176 | 0 | .050521 | .140625 | .187500 |
| Reverse / overlap | 18,649 | 1,761 | .073755 | .153646 | .184896 |
| Direct balanced cosine | — | — | .351858 | .739676 | .942661 |
| Direct BM25 | — | — | .252525 | .502471 | .684598 |

All queries and unassigned documents remain in denominators. Forward/reverse
hard full-corpus adjusted Rand index is .444678. On the fixed pair sample,
positive coassignment Jaccard is .317158 hard/.420380 overlap; different pair
decisions are 10,610/4,488. This is arrival instability, not semantic accuracy.
No semantic ancestor labels are generated from capacity-tree parents.

## Where retrieval is lost

Posthoc diagnostics use existing explicit development references and archived
full semantic scores. They are not a new executed method, equal-work benchmark,
latency result or agent-navigation evaluation.

| Order / membership | Queries with no topic | Reference coverage in posting union | After cap | Stored-score union oracle Recall@100 |
| --- | ---: | ---: | ---: | ---: |
| Forward / hard | 27 / 64 | .397396 | .305729 | .314063 |
| Forward / overlap | 27 / 64 | .376563 | .282813 | .308854 |
| Reverse / hard | 39 / 64 | .341146 | .187500 | .276042 |
| Reverse / overlap | 39 / 64 | .286458 | .184896 | .223958 |

Packing loses 11/9/15/9 judged references across 8/8/11/8 queries respectively.
Union median sizes are 1,329/1,381/0/0; maxima 4,607/4,407/5,891/4,437.
Better query-relevant packing could recover some loss, but even ranking all
admitted members with stored original scores cannot recover references outside
the union. Separate topic admission, representation, discovery and packing
controls before attributing all loss to PCA or BIRCH. No thresholds are repaired
after these outcomes.

## Label observations

Inspect the largest three topics by assigned count, with topic index as the tie
rule, for every order/mode. This is an exploratory inspection, not independent
human gold or label intrusion testing. Corpus-IDF makes some labels more specific:
forward hard's 2,399-document group changes from `team, we, project, you, work,
time, development, they` to `team, agile, scrum, project, we, sprint, work,
development`. Its largest group has `class, public, object, classes, method,
interface, void, methods`. Another retains `function, why, int, variable, x, c,
0, name`. Reverse hard has broad `data, database, server, web, user, application,
service, api` and noisy `0, n, x, 1, int, function, algorithm, array` groups.

Source support verifies where keywords occur, not semantic coherence. Lexical
containment diversification leaves these inspected top-eight corpus-IDF lists
unchanged. Do not introduce outcome-specific stopword/numeric rules to beautify
these examples. Preserve technical identifiers and assess label-only denoising
prospectively with unchanged memberships.

## Audits and cost

Two fresh processes match every decoded nontiming field and every recorded
array hash. The outside-Git auditor validates 2,753,875 projected coordinates
via alternate reduction (maximum error 1.06e-15), bootstrap selection/mean,
orthonormality and covariance eigenvalues, complete normalized directions,
CF batch traces/radii/counts, 115,273 memberships, 256 query-control records,
1,488 labels and 721 source supports. Maximum class-count/label/dense-score
errors are 1.14e-13/5.56e-17/6.67e-16. Source count files match the previously
independently counted baseline exactly. BIRCH replay uses the same pinned
library, not an independent implementation; rank comparisons preserve verified
archived scores and do not prove cross-platform close-tie parity.

Linux x86-64, AMD EPYC 9645, Python 3.11.15, NumPy 2.4.6, SciPy 1.17.1,
sklearn 1.9.1; OpenMP/OpenBLAS/MKL threads one. First/repeat after-import wall
times are 40.527/38.412 s. Setup 9.586/8.121 s, vocabulary 17.290/16.881 s,
projection including arrays/diagnostics .482/.521 s. Forward/reverse fit is
.208/.201 s first and .196/.189 s repeat; assignment 1.649/1.913 s first and
1.750/1.900 s repeat. Each class-label/support pass is .775–1.266 s across runs.
First routing+metric p50/p95 milliseconds: forward hard 13.50/21.72, overlap
13.35/17.35, reverse hard 13.94/22.15, overlap 13.86/22.93. Query inference,
production storage, CLI/MCP/HTTP, network and end-to-end ingestion are excluded.

External first/repeat monitors record 1,176/1,170 samples, 2/0 vanished reads,
20 ms requested sleep; actual median/max gaps are 36.31/78.48 and
35.39/60.78 ms. Smaps RSS maxima are 1,506,988/1,507,080 KiB, PSS
1,497,192/1,499,272 KiB; status VmHWM has the same maxima here. Separate rusage
is 1,505,768/1,506,484 KiB. Status is approximate; sampled maxima are lower
bounds; target-only scope excludes children/observer. Retained ledgers/arrays,
monitor overhead and concurrent work prevent production memory or ≤10%
complete-ingestion claims.

## Reproduction and decision

Use `cqadup_topics.py` with the prior source/semantic/repeat/preflight arguments,
plus `--geometry fixed-bootstrap-pca25 --output-directory FRESH_DIRECTORY`.
Wrap with `memory_audit.py`; fresh repeat adds `--compare FIRST/results.json`.
358 research tests pass, including frozen-coordinate/chunk/centering checks,
zero-projection abstention and raw-radius versus normalized geometry separation.

Outside-Git artifacts under `/home/james/scratch/mash-artifacts`:

- First `topic-cqadup-pca25-first-20261002/results.json`, SHA256
  `ee0f9aa1fe26857ed3fdab10d6ca92d27fa1e8fbc1ccc820f26efe43cfeccaa4`.
- Repeat `topic-cqadup-pca25-second-20261002/results.json`, SHA256
  `5f7bb9d39068d9250fe0b448d0c28308c984c630de199d137cb0c904983958c9`.
- Final audit `topic-cqadup-pca25-audit-final-20261002.json`; initial audit retained.
- Posthoc `topic-cqadup-pca25-analysis-first-20261002.json`.
- Monitors `topic-cqadup-pca25-memory-first-20261002.json` and
  `topic-cqadup-pca25-memory-second-20261002.json`.

Reject this **complete PCA25 + frozen routing configuration** for promotion.
This does not reject every PCA/BIRCH pipeline or select an alternative. Next
isolate query-topic admission and centrality packing from discovery quality;
topic-filtered vector retrieval is a separate prospective control. Robust
overlapping topics, semantic hierarchy, independent labels, actual guided
agent search, online startup/lifecycle, complete ingestion and Rust parity remain
open under the parent bead.
