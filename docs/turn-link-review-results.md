# Blind changed-link review

The amended protocol was frozen before packet generation and annotation. The
corpus-wide G treatment is excluded under the user requirement that embeddings
and clusters depend only on their own history. Historical G results are unchanged.

The sampler reconstructed four G-off C/F graphs from pinned vectors and verified
all existing judged predictions. No new inference or parameter choices occurred.
Every original reference pair was excluded in either orientation. The frozen
manifest SHA256 is `95903c86c5f10a2e1645a13f3b9c0cb444ecfd44e7c45c741988507e2fd6b69a`.

There were 31 changed pairs and 31 distinct controls. Thirty controls matched the
same history and distance band; one used the nearest distance in the same history.
Eligible pools were C additions: 1,053 long / 0 short; C removals: 2,065 long /
3 short; F changes: 8,922 long / 5 short. Fixed quotas produced six C additions,
nine C removals and seventeen F changes, with one overlapping selection.
Two fresh blind agents labeled alternating randomized packets. An independent
agent reviewed sixteen pairs. Exact ID coverage and literal evidence were
validated against hash-verified full turn text. No judgment was ambiguous.

| Selection reason | Related | Unrelated | Interpretation |
|---|---:|---:|---|
| C added link | 4 | 2 | Four useful additions, two false matches |
| C removed link | 6 | 3 | Six useful relationships lost, three false matches removed |
| F changed link (removal) | 10 | 7 | Ten useful relationships lost, seven false matches removed |
| Stable control | 22 | 9 | Diagnostic comparison group |

| Fixed cell | TP | FP | FN | TN |
|---|---:|---:|---:|---:|
| Baseline | 35 | 15 | 7 | 5 |
| F only | 21 | 7 | 21 | 13 |
| C only | 33 | 14 | 9 | 6 |
| C + F | 24 | 6 | 18 | 14 |

Independent review agreed on 15/16 pairs. Substituting the review subset changes
each method by one TP becoming FP; primary labels are preserved. Agent judgments
remain provisional and can share systematic errors. These purposively sampled
counts are not population precision/recall or statistical significance. Original
benchmark scores and sealed confirmation remain unchanged. Retain the baseline;
test targeted context selection and thread-local token-level boilerplate weighting
next. This completes the evaluation-coverage lever, not the entire three-lever goal.

Private packets, texts, labels and typed results live under
`/home/james/scratch/mash-artifacts/turn-link-review-20261006`.
