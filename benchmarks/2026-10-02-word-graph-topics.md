# Word communities and sparse mixed memberships: negative calibration

This implements an actual graph/mixture comparator after the literal-term
baseline, not a Rust production method or exact TopicMapping replication.
Bead `mash-c0d.19.2.35` remains in progress. No setting meets the frozen proxy
gate; no winner is evaluated on reserved outcomes.

## Method and provenance

TopicMapping motivates frequency-aware word-graph discovery followed by
nonexclusive mixture refinement. Its published count/Poisson null differs from
this diagnostic's binary fixed-DF hypergeometric null. Infomap discovers weighted
random-walk communities; PLSA provides document-specific word mixtures. We use
ordinary EM, not Hofmann's tempered-EM retrieval experiments. See
[TopicMapping](https://amaral.northwestern.edu/media/publication_pdfs/PhysRevX.5.011007.pdf),
[Infomap](https://www.mapequation.org/assets/publications/RosvallBergstromPNAS2008Full.pdf),
and [PLSI](https://eecs.csuohio.edu/~sschung/DSA460/PLSIHoffman.pdf).

The prospective Beads comment fixes all settings before outcomes. Reconstruct
the unchanged source hashes/counts and equal title/body mass from the original
topic and balance captures. Use only the original exposed 64 calibration
questions; tags enter pair evaluation only. The original 8,192-term vocabulary
was transductively fitted on 128 question sources and 72 background questions;
reserved source text therefore contributes vocabulary, but reserved references
and outcomes do not enter selection. Keep unigram terms with an alphabetic
codepoint and parent DF >=2 and <=.40. This admits **629 words**; cap 1,024 fails
without truncation. Numerical-only terms and phrases are excluded explicitly.

Test every unordered pair (197,506), using one binary vote per parent regardless
of token count. Weight admitted edges by observed parent co-occurrence. Compare
uncorrected upper-tail p<=.05 against Benjamini–Hochberg q=.05 over **all pairs**,
including zero-co-occurrence p=1. Dependent word tests, document-length variation
and mixed topics mean this is not a demonstrated corpus FDR guarantee.

Infomap 2.15.1 runs undirected/two-level, ten trials, one thread, seeds 17/29/101.
Canonicalize word groups, preserving actual partitions. Isolates are excluded
from topics rather than assigned singleton topics. Hard scores sum each topic's
balanced source mass divided by all eligible-word mass, including isolates.
PLSA initializes word distributions from graph communities/global source mass
with epsilon .001, then performs 50 sparse EM updates. Epsilon is used only at
initialization. Fractional balanced counts define a weighted likelihood, not
literal integer-count likelihood. Responsibilities are bounded by nnz*K<=2M;
zero-evidence documents and collapsed topics retain zeros. Mixture scores are
theta multiplied by graph-covered mass fraction. Save 51 likelihood values,
including the final parameters, and fail on a material decrease.

For each filter/seed/membership method, absolute thresholds .10/.20/.30 and
budgets 3/8 combine with a .5-of-best relative threshold: **72 configurations**.
Six literal unigram controls use the same vocabulary, frozen c-TF-IDF/background
IDF and budgets 1/3/8. All 2,016 natural pairs (97 tag-overlap positives) are
evaluated. Selection requires precision >=.70 and >=20 links in **every seed**,
then maximizes worst-seed recall, worst precision and a deterministic key.
This proxy threshold does not establish semantic acceptability.

## Results

| Filter | Edges | Covered / isolated words | Topics for seeds 17 / 29 / 101 |
| --- | ---: | ---: | --- |
| Uncorrected p<=.05 | 2,787 | 592 / 37 | 37 / 35 / 36 |
| BH q=.05 | 0 | 0 / 629 | 0 / 0 / 0 |

BH produces explicit unsupported/no-edge models; all questions abstain. Among
settings with at least 20 predicted links, the best observed hard precision is
7/48=.146, recall .072 (seed29, threshold.30, budget3). Best mixture precision
is 8/31=.258, recall .082 (seed101, threshold.30, budget3); 23 false positives
remain, one question abstains and 64 memberships are emitted. These are
post-outcome descriptive maxima, **not selected winners**. The robust selector
returns null. Higher training likelihood does not prove better topics.

Matched literal background-IDF budget3 has 8 TP/36 FP, precision .182 and recall
.082; budget8 has 26 TP/240 FP, precision .098 and recall .268. c-TF-IDF budget3
has 10 TP/55 FP. Differences from the previous literal report include restricting
the vocabulary to unigrams; do not compare them as a pure algorithm ablation.

Alphabetically inspected large communities still mix terms such as `class`,
`content`, `div` and `button`, or generic `problem`, `reason` and `used`.
This inspection is qualitative, not independent intrusion judgments. Whole-parent
co-occurrence lets incidental implementation details connect subjects. The
strict correction's empty graph shows limited support in this small, sparse
cohort; it does not prove graph discovery fails on larger corpora. Do not tune
stopwords or relax thresholds to make these exposed outcomes pass.

## Reproducibility and cost limits

The optional `graph` research extra pins Infomap without adding an application
dependency. The new runner verifies the archived original lock at `65ea577`,
all original package records (including multiple Python-version resolutions),
unchanged base dependencies and installed NumPy2.4.6/SciPy1.17.1/sklearn1.9.1.
Older runners retain strict lock guards; reproduce them from their pinned
commits, not by weakening drift checks.

Two fresh processes match all non-timing output. Total work after imports is
1.652/1.637s; graph filtering takes .211–.265s/filter, community discovery plus
50 EM iterations .139–.157s/seed. No additional inference/model download occurs;
prior bounded-unit inference remains charged. Times exclude imports, serialization,
memory monitoring and full initial/incremental ingestion. RSS, p95, updates,
deletions, restart recovery and the <=10% ingestion gate remain unverified.

Independent combinatorial enumeration verifies all **395,012** probability tests
across the two filters, including the full correction family. Dense EM replays
verify all three nonempty models, every likelihood, responsibility count
conservation, final distributions and coverage-scaled scores. Independent
membership/pair reconstruction verifies **157,248** decisions across 72 graph
and six literal controls. The full lab suite has 273 passing tests with the
graph extra; two integration tests skip explicitly without that optional extra.

Artifacts are outside Git under `topic-public-stackoverflow-20261002`:

- `graph-first.json`: `a37376e6d9c34e24f24ec5eb9d7aad3664887193fb063b9a6be0362ffb43a0a6`
- `graph-second.json`: `341be97e57c6e8fd2d7fefb82f294c308a711c2afc349cad092aa5a9439cffcc`
- `verify-graph.py`: independent audit; captures retain full graph tests, source
  counts, communities, score matrices, mixture distributions and code pins.

Next investigate source-supported local episode/window co-occurrence versus
whole-parent co-occurrence with settings frozen before outcomes. Genuinely new
groups, independent agent-chat judgments and complete maintenance measurements
are required before selection. These flat word communities provide no semantic
ancestors, online update contract, Rust parity or MCP topic feature.
