# Character-based source similarity control

Bead `mash-c0d.19.2.36.2`. The token audit found identifier collisions, but
preserving punctuation is not sufficient evidence of improved topics. Compare
the existing word representation with a general character-based candidate
before further clustering. This experiment measures source-pair similarity,
not topic assignments, semantic trees or guided-search success.

## Prospective protocol

Freeze the original exposed 64 sources/2,016 pairs (97 shared-tag positives).
Verify immutable source-channel and graph captures. Use original 629 raw word
counts versus [CountVectorizer char_wb](https://scikit-learn.org/stable/modules/generated/sklearn.feature_extraction.text.CountVectorizer.html)
3–5 character fragments, lowercase, source-question DF≥2 and ≤.40, cap 8,192
features. Character fragments retain punctuation and use artificial boundary
space padding; they are not source-contiguous phrase labels. No name-specific
rules, stemming or stopword repairs. Fit characters on only those 64 whole
sources without tags/reserved sources; the old word basis remains transductive.
Thus this comparison changes granularity, vocabulary construction and dimension,
not only punctuation retention. It cannot isolate a causal punctuation effect.

Reconstruct exact title/prose/code fields and hashes, verify full-source counts
equal summed channel counts. For both bases, use existing nonempty-channel L1
means for title/body, title/prose/code and title/prose, then smooth source-cohort
IDF `log((1+N)/(1+DF))+1` and L2 normalization: six matrices. Compute cosine
scores for the complete natural pair grid, AP/AUC, medians and fixed .1–.9 cuts.
Keep the .70 precision/20-link gate; no score-based fallback or topic selection.
No reference outcomes enter features and no encoder/LLM calls occur.

## Results

| View | Word AP / AUC | Character AP / AUC |
| --- | --- | --- |
| Title/body | .1543 / .6899 | .1120 / .6180 |
| Title/prose/code | .1633 / .7002 | .1262 / .6495 |
| Title/prose | .1401 / .6311 | .1244 / .6220 |

Characters reach the declared 8,192-feature cap. All sources have evidence in
all six matrices. All 54 cuts fail the unchanged gate. Best descriptive
precision among cuts with ≥20 links is .2105/.3077/.2500 for word title/body,
balanced and prose (all .2); character alternatives reach .1304/.1560/.1414
(all .1). These are exposed descriptions, not selected operating points or
independently measured semantic precision. Random-score AP prevalence is
97/2016≈.0481; above-prevalence ranking does not establish useful topic quality.

Synthetic fixtures show distinct character vectors for C++ versus C# without
special-casing names. That fixes a representation distinction but does not
improve this proxy ranking. Character fragments may share incidental code,
spelling or punctuation, while whole-source mixing remains. These are plausible
explanations, not isolated causal findings. Do not promote the candidate or
infer that all technical representations are ineffective.

## Verification and costs

Fresh processes repeat all nontiming fields exactly in 1.990/1.844 seconds
after imports. Matched feature counting takes .306/.261 seconds. These are
research reconstruction/scoring costs, not ingestion throughput or query latency.
No memory, edit/delete, persistence, incremental-vocabulary or production gate
is claimed. Five tests cover identifier distinctions, Unicode/channel count
conservation, IDF/L2/zero vectors, sparse round-trips and malformed inputs;
all 323 research tests pass.

The outside-Git scalar audit independently reconstructs padded character
counts, source hashes/channel conservation, DF admission, cap frequency
eligibility, original word counts, weights/vectors, 12,096 pair scores, AP/AUC
and 54 cut confusion matrices. The first verifier process was stopped after
identifying quadratic repeated row-total computation; the corrected verifier
precomputes totals. This does not alter experiment inputs/results. The audit
shares the source HTML/channel parser, so does not independently establish its
semantics. Equal-frequency cap ties use pinned library ordering; the audit
checks frequency eligibility rather than claiming a universal tie policy.

Artifacts remain outside Git in `topic-public-stackoverflow-20261002` under
mash-artifacts. Run `character_questions.py --root ROOT --output FRESH_JSON`,
then repeat with `--compare FIRST_JSON` in the pinned graph environment.

- First SHA256: `4e1a0e51e3d71835b3aee507dda528f8e4d5a37be987b560081a5992e6d14666`
- Repeat SHA256: `cd7b3e8c70e90c159f431ca8f1b63af8d27c5af7512e9b73919e775de2095e2d`

Independent source-grounded judgments and new groups remain necessary. The
candidate is not selected; production tokenization/search are unchanged.
