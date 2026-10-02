# Source-only topic token boundary audit

Bead `mash-c0d.19.2.36.1`. After the negative graph and online LDA controls,
inspect representation loss before more clustering grids. This audit changes
neither original tokenization nor production search.

## Procedure

Use the same 200 observed programming questions and pinned source-channel
capture (`ce527aac29659a72872e9430a8bfc534f5c05113f9e31ecdc638cffd22b0b634`).
Preserve exact title and displayed body hashes, prose/code run identities and
every original TOKEN match. Original normalization is lower, not casefold.
Compare those matches against maximal runs of Unicode alphanumerics, underscore,
combining marks and `. + : / # -`. These runs are diagnostic spans, **not a
selected tokenizer or a ground truth of meaningful identifiers**. Ordinary
sentence periods and code operators can also extend them. Pure punctuation
runs are excluded; no source text is deleted. Every span and match retains
code-point and UTF-8 byte coordinates into its original title/displayed body.

Record the existing stopword decision, then membership in the original 8,192
feature vocabulary and the 629 graph-unigram vocabulary. The latter was
filtered using the old calibration source cohort; these are representation
inventories, not eligibility judgments on new sources or topic quality. Keep
all forms that map to a shared lowercased original match. No tags, outcome
scores, extra embeddings or generative inference enter this audit.

## Findings

| Inventory | Count |
| --- | ---: |
| Maximal diagnostic runs | 47,731 |
| Original TOKEN matches | 47,789 |
| Different boundaries | 4,181 |
| Runs split into multiple matches | 55 |
| Runs containing plus/hash | 212 |
| Plus/hash runs with different boundaries | 199 |
| Normalized terms with multiple diagnostic forms | 1,107 |

Matches comprise 2,112 title, 21,564 prose and 24,113 block-code occurrences.
The existing stoplist excludes 14,354; 3,227 are outside the original feature
vocabulary, 16,939 additionally outside the graph vocabulary, and 13,269 are
graph-retained. These counts span all 200 sources, unlike the prior 64-source
fit; do not compare them as the same training-token population.

Observed examples include `C++` (six occurrences), `#include` (23), `id#11L`
(ten), `+0200` (nine) and `HomeController#index` (five). `C++` and `C#` become
`c`; hash can split one diagnostic form into multiple matches. The normalized
`c` inventory combines `-c`, `c`, `c#`, `c++`, `c.` and `c:`. Leading path/flag
punctuation is also lost under the old regex. Synthetic Unicode fixtures
additionally demonstrate loss of decomposed combining marks.
This demonstrates non-injective normalization, not that every distinction is
useful for topics. Of the 4,181 boundary differences, many are ordinary punctuation;
199 of 200 questions have at least one difference. Keeping all diagnostic
punctuation as new tokens would introduce its own noise.

The stoplist excludes `name` 140 times, `where` 44, `call` 17 and `system` 14.
Some may be code identifiers, but incidence alone does not establish semantic
importance. Do not patch these terms into a stoplist based on individual cases.

## Validation and next decision

First/repeat outputs match all nontiming fields in 2.505/2.216 seconds after
imports. A separate scalar verifier reconstructs all 47,731 spans and 47,789
matches, complete source fields/runs, coordinates, states and collisions.
It shares the original HTML renderer and stopword library, so does not prove
independent browser semantics or stoplist suitability. Six new tests and all
318 research tests pass. There is no semantic quality or ingestion-performance
measurement here.

Run `token_audit.py --root ROOT --output FRESH_OUTSIDE_GIT_JSON`; repeat with
`--compare FIRST_JSON` in the pinned graph research environment. Artifacts remain
outside Git in `topic-public-stackoverflow-20261002` under mash-artifacts.

- First SHA256: `8f48c259f8e15b1dda2353af947fb448e246c4bfd19fde46a4a979409c0a41cc`
- Repeat SHA256: `7777fa67bf52aab251131aadab24b457b764fc5380b7c50c942298c2bc3cb3e9`

A next candidate must preserve source distinctions without treating arbitrary
operators and sentence punctuation as topics. Compare against the immutable
old representation, preregister normalization/phrase rules and require
independent subject judgments plus new source groups before selection.
Existing embedding tokenization and production SQLite/remote search were not
audited by this experiment; these findings concern the Python topic features.
