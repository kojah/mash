# Matched source-unit topic grouping

This exposed development experiment tests the unit of representation, not a
production topic winner. It uses no LLM, paid API or reviewer-written model text.
Two fresh captures produce identical nonmeasurement fields for all 72 controls.

## Setup

The frozen development scope contains 91 original records (68 user anchors and
23 intact assistant evidence records), split into 257 exact structural spans.
Admission and reference evidence are oracle controls from earlier reviews;
metadata and rejected proposals remain present. Unicode offsets, identities,
source hashes and the complete input scope are reconstructed before inference.
Historical commands are data and are never executed. Private bodies, keywords
and memberships remain outside Git.

Before predictions, a single exposed coding assistant froze 50 source-pinned
pair judgments: 14 overlapping task aspects, 33 distinct and three uncertain.
The review includes all 36 cross-response comparisons between six detailed fix
families, six pairs connecting those families to a multi-aspect summary,
legacy-import counterparts and five cross-case contrasts. Shared broad parents
do not imply the same fine aspect; topic mention does not imply endorsement.
This purposive, correlated sample is neither representative nor independently
annotated. Uncertain pairs are excluded, leaving 47 evaluated pairs.

Installed quantized MiniLM encodes exact original spans offline, CPU one thread,
one span per call, with a 512-token preflight and no truncation. All 257 spans
fit (maximum 439 tokens); 256 unique calls and one cache hit. Eight spans have
zero lexical features. Every geometry uses the same remaining 249 spans, pooled
into 86 records or eight threads where requested. These are normalized means
of the same span directions, not whole-text embeddings or existing Rust vectors.

Geometries are dense 384 dimensions, fixed nonnegative lexical hashing into 512
dimensions, and equal-weight normalized concatenation into 896 dimensions.
Lexical features are exact source-adjacent unigrams/bigrams with English
stopwords. [HashingVectorizer](https://scikit-learn.org/stable/modules/generated/sklearn.feature_extraction.text.HashingVectorizer.html)
has no learned vocabulary or IDF; its fixed coordinates permit streaming, but
collisions remain a limitation, especially with this small feature budget.
Labeling separately uses source-supported c-TF-IDF candidates, not hash inversion.

The fixed grid is three geometries × three unit representations × four methods
× forward/reverse order. BIRCH uses batch `fit_predict`, threshold 0.4,
branching 64, no global clustering. GRINCH-style uses exact leaf neighbors and
the previously tested tree, accepting a cut only when every internal merger
cosine is at least 0.7. Offline average linkage uses cosine distance 0.3.
HDBSCAN uses Euclidean distance, minimum cluster size/sample count five and EOM.
These native criteria are different and uncalibrated; none was tuned afterward.

## Results

Raw pair ranking is independent of clustering method/order. AP is average
precision on these reviewed pairs, not assignment accuracy or topic quality.

| Representation | Dense AP / ROC AUC | Lexical AP / ROC AUC | Hybrid AP / ROC AUC |
| --- | --- | --- | --- |
| Exact spans | 0.855 / 0.918 | 0.753 / 0.797 | 0.831 / 0.885 |
| Record span means | 0.305 / 0.515 | 0.305 / 0.515 | 0.305 / 0.515 |
| Thread span means | 0.333 / 0.576 | 0.333 / 0.576 | 0.429 / 0.636 |

Coarse pooling necessarily maps different aspects from one source to the same
vector. The review intentionally stresses that limitation; the AP improvement
must not be generalized to arbitrary chats. Equal lexical fusion does not beat
dense span ranking on this sample, so it is not promoted.

Fine-span native grouping still misses important overlaps:

| Geometry / method | Clusters | Noise spans | True / false positives, out of 14 / 33 |
| --- | ---: | ---: | --- |
| Dense BIRCH | 228–229 | 0 | 3 / 0 |
| Dense GRINCH-style | 227 | 0 | 3 / 0 |
| Dense average | 228 | 0 | 3 / 0 |
| Dense HDBSCAN | 4 | 169 | 6 / 4 |
| Lexical BIRCH / GRINCH-style / average | 238 | 0 | 0 / 0 |
| Lexical HDBSCAN | 2 | 188 | 0 / 0 |
| Hybrid BIRCH | 239–240 | 0 | 0 / 0 |
| Hybrid GRINCH-style / average | 242 | 0 | 0 / 0 |
| Hybrid HDBSCAN | 2 | 138 | 7 / 9 |

Dense GRINCH-style recovers three of six repeated detailed families. The other
three have cosine 0.652–0.675, below its cut. All six summary-to-family cosines
are only 0.361–0.487; neither native assignment nor the all-centroids cosine
≥0.7 affiliation diagnostic recovers any. The two legacy counterparts score
0.520 and 0.288. Lowering a threshold on these same exposed cases would not
demonstrate a robust fix.

The centroid diagnostic allows multiple affiliations, but does not solve this
representation problem. Dense HDBSCAN yields true/false positives 4/2 forward
and 1/1 reverse under that diagnostic, despite identical native 6/4 counts.
It is not HDBSCAN prediction or calibrated confidence. Noise label −1 never
counts as a shared topic. Pooled record/thread controls often conflate 28 of 33
distinct pairs, even when native cuts form numerous singleton coarse points.

Source-contiguous labels are not automatically specific names: one compression
cluster ranks `output`, `deliberately` and `obligations` before `compression`.
A singleton SQL-related row yields `complete iteration` and `owning transaction`.
These inspected examples establish candidate limitations, not a quantified
label-quality score. No semantic ancestor interpretation is justified.

## Runtime and provenance

Xenia, Python 3.11.15, NumPy 2.4.6, scikit-learn 1.9.1, ORT 1.30.0 and Tokenizers
0.23.2; numerical and encoder CPU threads one. Encoding takes 846–863 ms,
including preflight/cache handling; measured inference takes 723–734 ms.
Lexical/hybrid feature construction takes 7.7–7.9 ms. Dense fine-span grouping
takes BIRCH 11–27 ms, GRINCH-style 186–259 ms, average 8–37 ms and HDBSCAN
31–35 ms, including validation/cut extraction where applicable. These ranges
cover two orders and two fresh processes, not confidence intervals.

Each complete 72-control process takes 10.2–10.7 seconds after imports and peaks
at 956 MiB RSS, including source loading and the scientific runtime. Shared
encoding/cache and sequential controls prevent interpreting these as standalone
method memory/cost or production ingestion overhead. The ≤10% ingestion target,
edits/deletes, persistence and guided-search gain remain unverified.

Implementation: `research/topics/span_grouping.py`, `span_units.py`,
`unit_grouping.py`, `pair_review.py`, `compare_spans.py`. The 105-test lab suite
covers source mutation/offset rejection, pair pinning, matched feature support,
pool cancellation, multi-affiliation, noise correctness and complete repeat checks.

Private root: `/home/james/scratch/mash-artifacts/span-grouping-20261001`.
`first.json` is an exploratory capture before explicit HDBSCAN `copy=False` and
indentation cleanup; only the two validated captures support the final results.

- `validated-first.json`: SHA256 `658d15eea10022a74a4b1470bb1875430cc7a4266444901d00f0c586e56400d9`
- `validated-second.json`: SHA256 `812d14d459c1dbebe3b0edfc9b23cb3fc7710465791fb2857a10208f35fe16e3`
- Frozen pair review: SHA256 `c075848ed114e1d621e4013ee3ea5eb47885abccb12e15d30fdfa0c2f301889d`
- Exact structural capture: SHA256 `52d857979929c42202e8bcfa784b73a63faf887fc46eccd02ab37e9073bce8d8`

The next step is bounded within-paragraph multi-aspect source units and
development-group-separated operating-point evaluation, preserving original
offsets and measuring extra encoder work. A clustering swap alone is not the
supported remedy. No Rust/default change follows from this experiment.
