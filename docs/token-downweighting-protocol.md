# Token downweighting experiment

Freeze before new inference or scoring. This is a new exploratory development
study, not a rescue of the completed six-cell greeting experiment. Production
and the frozen historical baseline remain unchanged. No LLM or model download.

Use the existing 200-history cohort SHA256
`d779a9a1850cb29558987e510ddd2452878aca20ebb57dd12ee5c657d5192805`,
only its 160 development histories, and installed quantized MiniLM generation
`2894b8dc20a8bea9f9f78c49d9e349728c69450a273a509298ec8aaf94905491`.
Keep confirmation sealed. Rebuild both context-whole pooled and heading-aware
chunk views with singleton inference, equal-role pooling, the existing i8 codec,
uniform smoothing alpha .10/window four on the pooled view, BIRCH radius .60,
cutoff .75, and namespaced union. No context-selection or clustering tuning.

## Four fixed Optuna trials

Apply each policy to both views and both roles. Attention/input text is unchanged.
Reuse each input's hidden states across policies, never across input batches.

1. **uniform**: matched singleton mean pooling.
2. **function**: weight a fixed conservative list of articles, pronouns,
   conjunctions, auxiliaries, and conversational interjections .25; others 1.
3. **idf**: each original source message is one document, using only this thread.
   Weight `max(.25, (1 + log((N+1)/(df+1))) / (1+log(N+1)))`.
   Absent terms receive 1. This is an IDF-style contextual pooling adaptation.
4. **sif**: own-thread unigram counts over original source messages, weight
   `max(.10, .001/(.001+p(word)))`. This is SIF-style contextual pooling,
   without component removal; it is not the original static-vector SIF pipeline.

Only ordinary alphabetic words are eligible. Preserve code fences/inline code,
negations, mixed-case technical names, paths, numeric identifiers, and special
tokens at weight 1. Each subtoken inherits its overlapping word's weight.
Statistics are computed once from original messages, not duplicated windows.
They never depend on another history. Broad frequency weighting may suppress a
genuine main subject; that risk is measured, not assumed away.

Score the same original development turn pairs. Report overall and long-history
precision, recall, F1, changed predictions, 2,000 paired thread-bootstrap intervals
versus the matched control, and historical-control differences separately.
Positive evidence requires positive lower delta-F1 bounds plus the existing
absolute quality gates overall and on long histories. Reused development labels
and unadjusted intervals remain exploratory; no confirmatory significance claim.
No post-hoc extra trials, parameter changes, or automatic confirmation opening.

## Message-level pilot

Use the three previously frozen embedding-cleanup TOML corpora (light synthetic,
heavy synthetic, and 17 reviewed real requests/117 documents). Compare uniform
versus fixed function weighting only; no cross-document frequency estimation.
Documents and queries both use their own singleton inputs, first 510 tokens if
needed. Report truncation, MRR, recall@5, first-rank counts, changed rankings and
regressions, with per-corpus hashes. Relevant IDs are source-finding labels, not
exhaustive relevance. The real pilot previously had perfect ranks: it is a
regression check with little headroom, not sufficient evidence for production
search improvement. No full hybrid/reranked-search claim.

## Invariants and runtime

Verify uniform pooling parity, nonzero weighting coverage, protection of code and
negation, finite normalized vectors, and exact singleton isolation under reordered
or mutated surrounding histories. Save input/model/protocol/code/artifact hashes,
weights, vector-change counts, timing and peak RSS privately outside Git. Resume
only artifacts whose contract and hashes match. Report procedures/results in the
LaTeX paper. Component removal and attention masking are excluded from this
bounded study; neither is inherently a filler detector.

Prior art: [SIF author implementation](https://github.com/PrincetonML/SIF),
[Sentence Transformers WordWeights](https://www.sbert.net/docs/package_reference/sentence_transformer/modules.html),
[TF-IDF](https://scikit-learn.org/stable/modules/feature_extraction.html#tfidf-term-weighting).

## Pre-score protection correction

Before any turn quality scoring, an edge-case review found that the initial
word matcher split curly-apostrophe negations into pieces. Stop that build,
retain it unscored, and rebuild with both straight and curly apostrophes treated
as one word and normalized for negation lookup. A regression fixture covers
both forms. Record the corrected code/protocol hashes in a fresh artifact
directory; do not mix generations or reuse the interrupted matrices.
