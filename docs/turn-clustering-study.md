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

The enrichment subsequently completed and all 69 annotation files validated.
Development now contains 617 related, 313 unrelated and 121 ambiguous judgments.
Blind development review agrees on 106 of 116 pairs, including 99 of 104 with
decisive judgments from both annotators. Confirmation label sufficiency is true
without exposing those judgments or scores. The enlarged annotation manifest
hash is `a725b589699dfb7b03b6a601666251349bbb168f1e2371cb16214067b195482f`.
The completed representation build and negative development decision are
documented below; no algorithm has been accepted.

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

The artifact builder ran separately from annotation and scoring. It
reuses matching vectors from the previous private cache, keyed by encoder
generation and exact input text. Report these construction timings as
cache-assisted, not fully cold inference. Costs record newly inferred vectors,
input token counts, discarded tokens and clustering-point counts per view.
The new separate-view windowing/pooling tests check source coverage, equal view
weights and fallback when no answer is present.

## Completed development comparison

All 1,000 thread/view matrices and cost records completed. The registered
development runner scored all sixty configurations on all 160 development
histories. No configuration met all quality gates; the frozen selection is
`null`. The strongest simple baseline was chronological four-turn blocks:
precision 0.869, recall 0.548 and F1 0.672. Its recall and F1 fail the absolute
floors too. Confirmation judgments and algorithm scores remain closed.

Best registered configuration per representation (all BIRCH radius 0.75,
membership cutoff 0.65):

| Representation | Precision | Recall | F1 | Bootstrap 95% F1 interval |
| --- | ---: | ---: | ---: | --- |
| Prompt-only | 0.859 | 0.386 | 0.532 | 0.483–0.585 |
| Joint truncation | 0.781 | 0.804 | 0.792 | 0.752–0.828 |
| Prompt-conditioned passages | 0.798 | 0.793 | 0.795 | 0.759–0.830 |
| Pooled existing messages | 0.825 | 0.733 | 0.776 | 0.733–0.820 |
| Separate prompt/answer pooling | 0.839 | 0.737 | 0.785 | 0.737–0.828 |

These are exploratory within-development rankings, not held-out winners.
Intervals use 2,000 paired thread bootstrap resamples, including negative-only
histories. The highest-F1 passage method gains 0.123 over the baseline, but
precision is below 0.85. Separate pooling gains 0.113 but also misses precision.
The corresponding bootstrap gain intervals are 0.072–0.171 and 0.058–0.163;
the preregistered gate uses point estimates, not an interval-based exception.

The separate-pooling configuration scores F1 0.909 on two-to-seven-turn
histories but 0.643 on histories with at least eight turns. Its distant-pair
precision/recall are 0.629/0.541. Passage BIRCH scores F1 0.889/0.707 on those
length strata and distant-pair precision/recall 0.561/0.680. Thus the global
precision near-miss is not sufficient evidence of robust long-thread grouping.
The four-turn baseline has zero recall for distant pairs by construction.

The predeclared primary-only diagnostic for joint BIRCH (radius 0.60) scores
precision 0.864, recall 0.729 and F1 0.791, with an F1 gain of 0.119. That mode
was diagnostic, not an eligible selection in the sixty-configuration grid.
Its long-thread precision/recall/F1 are 0.817/0.554/0.660, versus short-thread
F1 0.903. It must not be retroactively promoted as the registered winner.

Per-thread cache-assisted construction p95 is 2.826 s for prompts, 7.369 s for
joint inputs, 14.258 s for conditioned passages, 0.248 s for stored-message
pooling and 14.197 s for separate pooling. Joint inputs record 1,478,672 discarded
tokens; passage and separate inputs record none. Construction inferred 24,359
new vectors across the four fresh views. The leading passage and separate
BIRCH configurations have clustering p95 31.0 ms and 11.6 ms, respectively;
runtime is not the limiting development gate. The observed kernel high-water
RSS during the near-complete build was 1.43 GiB; this is an observation through
that point, not a complete externally captured process memory benchmark.

Private authoritative artifacts are `turn-study-development-20261005.json`,
`turn-study-analysis-20261005.json` and the original matrices/annotation packets
under `/home/james/scratch/mash-artifacts`. The cleanup/extractive-selection
follow-up is tracked in `mash-jfn`. It must freeze its changes before new scores
and preserve the unopened confirmation set; this study's negative decision is
not permission to retry successive candidates on confirmation.

## Development-only assignment-drift diagnostic

After inspecting development results, we compared the existing full-thread
assignments with a fit using only turns through the later member of each judged
pair. Embeddings and insertion order remain identical; only subsequent turns
are removed. This is an exploratory counterfactual, not a new eligible method
or an online-quality validation. Ambiguous labels are excluded from its counts.

For separate pooling/BIRCH radius 0.75, 701 of 930 decisive pairs have later
turns; 63 decisions change, with 37 corrected and 26 worsened by removal. On
long threads, 44 of 510 eligible pair decisions change, with 27 corrected and
17 worsened. Only 14 long-pair primary assignments change, so the centroid
membership stage contributes to instability beyond primary partition changes.
Joint/BIRCH radius 0.60 changes 21 of 510 eligible long-pair decisions, correcting
14 and worsening seven; thirteen primary decisions change.

Pairwise cosine ROC AUC, independent of clustering, is 0.856 for short and
0.767 for long histories with separate pooling, and 0.840/0.787 with joint
inputs. These are discrimination diagnostics, not newly tuned thresholds.
The strata contain different types of pairs, so the AUC difference does not
isolate a causal effect of length. Assignment drift is real, but removing later
turns fixes only part of the observed errors. Input normalization, context and
clustering changes require separate, bounded ablations.

The deterministic twelve-case development audit (four false positives, four
false negatives and four true-positive controls, at most two cases per history)
also exposed provider question/reply transport JSON inside an embedding input
and a very high-similarity false positive between different proposal-writing
tasks. These are hypotheses for normalization and context work, not reasons
to rewrite labels or add phrase-specific exceptions. Private case excerpts and
the complete drift audit remain outside Git.
