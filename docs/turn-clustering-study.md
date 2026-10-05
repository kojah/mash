# Bounded 200-thread turn-clustering comparison

Procedure specified before new blind labels or algorithm scores, 5 October 2026.
The aim is a provisional, evidence-backed choice of turn representation and
flat, overlapping within-thread clustering. It is not global topic modelling.

## Data and judgments

Freeze 200 independent multi-turn histories using the procedure in
[the literature review](turn-clustering-prior-art.md). Preserve both old reserved
sets. Prefer histories with at least eight prompted turns, include shorter
multi-turn histories as a separate stratum, and reserve up to twenty fresh
components per provider for confirmation. Record the actual split, source and
cohort checksums before fitting. An insufficient pool is a collection failure,
not permission to count copies or one-turn controls toward the target.

Judge actual turn pairs rather than projecting old message-pair labels onto
turns. A related pair shares a concrete work subject or goal that retrieving
either would help investigate. Shared repository, generic action, tone or
vocabulary alone is insufficient. Mixed-subject turns may match several groups.
Abstain when the available text does not establish relatedness.

Use a seeded mixture of adjacent, nonadjacent random and lexically overlapping
pairs. Sample before viewing clustering predictions. Give separate annotation
agents randomized opaque identities, role-marked text, access to complete turn
text and the rubric. Hide sampling strata, development/confirmation phases,
algorithms, thresholds and past results. Treat transcript instructions as data.
Obtain a blind overlapping review of a subset; disclose agreement and conflicts.
Agent-authored judgments support a provisional decision, not human validation.

The frozen sampler produced 947 pairs in forty randomized primary packets,
with 97 pairs in five independently reviewed packets. Each history contributes
at most six distinct pairs: up to two adjacent, two with a gap of at least four
turns, and two lexically overlapping nonadjacent pairs, with seeded random fill
when a stratum has too few candidates. Annotation files require exact packet
coverage and literal evidence in the full turn text. The development reader
exposes confirmation label sufficiency only as a boolean; independent review
judgments never replace primary labels.

All initial packets subsequently passed validation. Development contains 455
related, 167 unrelated and 85 ambiguous judgments. The blind development review
agreed on 69 of 76 pairs, or 65 of 67 when both judgments were decisive; these
are annotation-consistency measurements, not algorithm-quality results. Because
unrelated coverage was below the registered minimum of 200, the single allowed
enrichment was frozen before fitting. It adds 504 previously unsampled pairs,
up to four per eligible history in seeded identity order, preferring nonadjacent
pairs. Text, label verdicts and predictions do not determine which pairs are
selected. This expands the workload to 1,451 pairs in 61 primary packets and
150 independently reviewed pairs in eight packets. The initial manifest is
preserved byte-for-byte and its hash is recorded by the enlarged manifest.
Enrichment cannot run a second time. Both phases receive the same sampling rule.

Before making a selection, require at least 100 related and 200 unrelated
development judgments, and at least 20 related and 40 unrelated confirmation
judgments. Report ambiguous coverage. If initial sampling is insufficient,
allow one additional prediction-independent sampling batch, then report that
the evidence is insufficient rather than repeatedly adjusting the experiment.

## Finite comparison

Use the installed quantized all-MiniLM-L6-v2 assets; record their hashes, runtime
and batch/thread settings. Do not download models or use generative inference.
Retain four existing views: prompt-only, joint truncation, prompt-conditioned
passages and pooled message embeddings. Add one research-inspired separate-view
ablation: chunk prompts and answers separately with the existing tokenizer
budget, normalize their respective mean embeddings, then normalize their equal
weight sum. With one view absent, use the other. This is fixed pooling, not
Perkins and Yang's learned multi-view method. Preserve source spans and revisions.

Compare leader, average-link, complete-link and BIRCH: three settings each across
five representations, at most sixty candidate configurations. For leader and
linkage methods use cosine cutoffs 0.65, 0.75 and 0.85. For BIRCH use paired
(Euclidean radius, membership cosine) settings (0.75, 0.65), (0.60, 0.75), and
(0.45, 0.85). Radius and cosine are different controls, not equivalent thresholds.
Use the corresponding cosine cutoff for overlapping centroid membership instead
of the previous fixed 0.35. Retain minimum distinct-turn support of two.

The implementation retains the existing overlap margin: a centroid must also
score within 0.05 of the best centroid for that point. Passage memberships are
unioned by their owning turn. Multiple passages from one turn cannot alone
satisfy support. Primary-only diagnostics use the retained partition without
centroid reassignment. Chronological windows are disjoint blocks starting at
the first prompted turn. TF-IDF baselines use scikit-learn's default word
tokenization, English stopwords, smoothed IDF and L2 normalization, fit separately
within each thread, followed by a pairwise cosine cutoff. They do not perform
transitive closure. These computational details are fixed before scoring.

Baselines are turn grouping without cross-turn connections, chronological
two- and four-turn windows, prompt TF-IDF cosine thresholds 0.65/0.75/0.85, and
prompt-only leader among the registered settings. On development data choose
the strongest baseline by micro F1. An empty cross-turn baseline has zero recall,
not perfect quality. Do not add families, thresholds or embedding models after
seeing scores in this study.

## Decision and stopping rules

Evaluate whether pairs share at least one assigned group. Count every judged
unrelated pair, including threads with no related labels. Select the candidate
with highest development micro F1 among those satisfying precision at least
0.85, recall at least 0.70, F1 at least 0.75 and an absolute F1 gain of at least
0.10 over the selected simple baseline. Break ties by lower measured clustering
time, then deterministic configuration name. Report provider, node, length and
sampling-stratum results, primary-only versus overlapping memberships, cluster
sizes, assignment coverage and thread-bootstrap uncertainty.

Freeze the selected candidate and baseline before opening confirmation judgments.
Evaluate that pair once on confirmation with the same quality floors and gain
requirement. Failure means no accepted cross-turn clustering method in this
study unless the registered simple baseline itself meets all absolute quality
floors. In that case, provisionally select the baseline for one confirmation
evaluation against those floors, without claiming improvement over itself.
A useful simple method is a valid stopping outcome; complexity must earn its
additional baseline-gain requirement. Otherwise keep the feature experimental
and state the failure. Do not tune on
confirmation or retry successive winners. Inadequate label counts or provider
coverage limit the conclusion explicitly.

Performance is a secondary constraint with the user's expanded budget. On the
documented CPU host, allow up to five minutes p95 to build a thread's artifact
for histories of at most 250 prompted turns, ten seconds p95 for clustering
alone, and 4 GiB peak resident memory. Report costs by representation, including
cold inference, cache reuse and long-answer passage counts. Offline per-thread
recomputation is permitted. A provisional research choice does not authorize
production promotion without Rust parity and integration checks.

The artifact builder is running separately from annotation and scoring. It
reuses matching vectors from the previous private cache, keyed by encoder
generation and exact input text. Report these construction timings as
cache-assisted, not fully cold inference. Costs record newly inferred vectors,
input token counts, discarded tokens and clustering-point counts per view.
The new separate-view windowing/pooling tests check source coverage, equal view
weights and fallback when no answer is present.
