# Incremental NMF coding-chat baseline: reject this fixed configuration

Decision: **do not promote the frozen NMF configuration**. Retain source-aware
phrase topics for independent review, without declaring them production-ready.
Do not run further K/seed/pass/threshold sweeps in this round.

## Setup and prior art

The official [MiniBatchNMF API](https://scikit-learn.org/stable/modules/generated/sklearn.decomposition.MiniBatchNMF.html)
supports partial-fit factor updates; the official [text-topic example](https://scikit-learn.org/stable/auto_examples/applications/plot_topics_extraction_with_nmf_lda.html)
uses nonnegative factorization for topic extraction. We test this conventional
alternative to many separate phrase subjects, not a novel algorithm.

`research/topics/INCREMENTAL_NMF_PROTOCOL.md` froze the design before results:
K16/32, one/five passes, canonical forward/reverse order, batches128, random718,
Frobenius loss, no forgetting/regularization/restarts, transform100/tol1e-4.
Exact source terms, one lexical vote per distinct input per thread, 8192-feature
transductive vocabulary, 40%-thread DF exclusion after three-thread warmup,
sublinear TF/thread IDF/L2. Factor updates are incremental; vocabulary/IDF are
fitted on this complete corpus, so the whole pipeline is not fully online.

Final source assignment uses component-norm-corrected coefficients normalized
to mass, >=.25 absolute and >=.75 of best, top3 with component-index ties. These
are uncalibrated mixture shares, not probabilities. Thread selection uses the
fixed half-share/half uncovered distinct-input utility, top8. Labels rank component
weights with original-source occurrence and lexical containment pruning, top8;
display the first. A label must occur somewhere in its assigned topic sources,
not necessarily every source or every tagged thread. Source membership can be
semantic/cofactor evidence without literal occurrence of the displayed phrase.
Legacy review-helper fields named `any_phrase_evidence_recall` and
`subjects_with_no_literal_evidence` therefore mean model-assigned support in these
arms; do not interpret them as literal-label grounding. Report distinct-support
F1 for comparison and require independent semantic/source review for promotion.

Same frozen eight-thread/68-record exposed review (9 primary/9 secondary) and
separate 128-thread/1850-record cohort as prior controls. Original scope/hashes,
source identities and all exclusions preserved; no new judgments, inference,
LLM, model download or paid call. Fresh pinned CPython3.11.15/NumPy2.4.6/
SciPy1.17.1/sklearn1.9.1, one OMP/OpenBLAS/MKL worker, author module hash recorded.

## Quality diagnostics and decision

| Model | Primary distinct support F1 | Secondary distinct support F1 | Secondary unmatched | Referential-only topics |
| --- | ---: | ---: | ---: | ---: |
| Source-aware phrase baseline | .734512 | .515344 | 2/9 | 10 |
| NMF16, one pass | .525333 | .509259 | 1/9 | 2 |
| NMF16, five passes | .494709 | .440741 | 2/9 | 1 |
| NMF32, one pass | .445679 | .492593 | 2/9 | 5 |
| NMF32, five passes | .684063 | .322222 | 4/9 | 6 |

Small review forward/reverse metrics agree; 68 records fit one batch, so this
scope does not provide a meaningful multiple-batch history stress test. Every
tested NMF option is below the phrase baseline on both support-F1 groups.
Fewer referential-only components do not establish more accurate labels: a
generic component can mix those replies with content. Single-assistant,
corpus-familiar exposed annotations and oracle source admission remain
limitations. These metrics compare evidence shapes, not independent semantic
precision or universal superiority of phrase discovery over NMF.

| Broad model | Topics | Untagged sources forward/reverse | Multi-tag sources forward/reverse | Dominant-membership order ARI |
| --- | ---: | ---: | ---: | ---: |
| NMF16, one pass | 16 | 382 / 370 | 211 / 211 | .809075 |
| NMF16, five passes | 16 | 282 / 246 | 90 / 134 | .585414 |
| NMF32, one pass | 32 | 662 / 639 | 57 / 68 | .690154 |
| NMF32, five passes | 32 | 594 / 567 | 63 / 64 | .585509 |

All128 threads receive tags. 1766 distinct active training units yield14/70
partial-fit steps. Dominant-membership ARI is permutation invariant but reduces
multi-label output to one tag and groups all unassigned sources together; it is
not semantic or multi-label stability. More passes do not consistently improve
coverage/stability. Fixed K avoids hundreds of microtopics at the cost of
potential broad mixtures; no independent review shows these are useful topics.
Model-generation/index identities also rename all components after model changes;
unknown vocabulary, edits/deletion and identity reconciliation remain unresolved.

The support losses/order drift justify rejecting this **configuration**, without
retuning it on these cases or claiming NMF can never work. The retained phrase
candidate still lacks independent label quality and full-ingestion performance.
Acceptance/stop rules: `research/topics/SELECTION_CRITERIA.md`. No further new
families or parameter grids in this round; at most one specified two-arm correction,
then independent gates and an explicit acceptance/failed-gate report.

## Cost and verification

Broad full Python frame/fit/transform/snapshot pipelines, forward/reverse seconds:

| Model | First | Repeat |
| --- | --- | --- |
| NMF16, one pass | .828 / .754 | .962 / .805 |
| NMF16, five passes | 1.645 / 1.527 | 1.812 / 1.574 |
| NMF32, one pass | 1.466 / 1.307 | 1.525 / 1.409 |
| NMF32, five passes | 3.333 / 3.566 | 3.496 / 3.751 |

Whole16-arm experiment after initial hashes is17.426/18.518s wall,
17.294/18.239s CPU; includes loading/DB source verification and final source/code
hash rechecks, excludes imports/initial hashes/output serialization/compare.
Review arm costs17.8--47.5ms first. No convergence warnings were captured. These
are prototype observations, not complete production ingestion or a matched
throughput comparison with old phrase timings; the <=10% gate remains unproven.

Externally sampled target RSS/status HWM199820/196684 KiB, PSS189775/186492 KiB;
pre-serialization rusage199220/195580 KiB. 746/794 samples,0/1 vanished reads,
maximum gaps45.47/43.91ms. Samples are lower bounds, status counters approximate,
observer intrusive; target includes imports/source-loading/native libraries,
excludes children/observer. Rusage can preserve launcher history; do not subtract
peaks or claim isolated topic-worker memory.

Two sequential runs reproduce every non-runtime field exactly, including feature/
component/coefficient hashes, labels/memberships/review/ARI. Four fixtures check
scale invariance, zero abstention, duplicate training votes, source/provider
scope, labels with original support and threshold/index ties. A near-threshold
fixture preserves binary64 `.75*.4 > .3` rather than adding a hidden epsilon.
All670 research tests pass in4.384s with author-reference paths. No independent
factor-solver audit or NMF persisted lifecycle/Rust parity is claimed.

Private artifacts under `/home/james/scratch/mash-artifacts`:
`incremental-nmf-{first,repeat}-20261002.json`, their `.log` files,
`incremental-nmf-{first,repeat}-memory-20261002.json`, and
`incremental-nmf-tests-20261002.log`. Outputs are0600 and retain input/helper/
protocol/author/package provenance.

First SHA256: `477a394c957b5d48a701a8e30ae0c9da262ee64068c9b750950c7ecb6123414c`.
Repeat SHA256: `2639297b6e98746e7b6114999336698ff693178b86d746982a9883e490428b9e`.
Source control SHA256: `8f392fdbe90c3debcc4abe17ebf99611b4c1901438e13e89aa18158be0d87507`.
