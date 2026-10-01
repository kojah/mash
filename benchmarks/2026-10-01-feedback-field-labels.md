# Fixed-membership feedback field labels

Development experiment, 2026-10-01. No method promotion or production change.
This follows the [explicit feedback payload pilot](2026-10-01-feedback-payload-controls.md).
The prospective setup was recorded in `mash-c0d.19.2.25` before new labels.

## Setup

Pin that pilot's forward GRINCH payload memberships: eight source records from
one UI-feedback thread, four groups. Preserve the same model-window exclusion
of the ninth task. No embeddings, clustering, membership changes or LLM calls.
Source judgments and facet names are evaluation-only. These are exposed,
single-assistant development judgments, not independent human evaluation.

Compare full payload with exact target/feedback/unknown spans, and with
feedback/unknown spans alone. The parser recognizes two explicit feedback layouts;
page, viewport, location, element-path, nearby-text and framing are separate roles.
Keep unknown content, code under its enclosing field, complete Unicode offsets,
and original source text. Unsupported layouts fail closed. This recognizer is
specific to an exposed format, not a general semantic importance classifier.

Count source-adjacent unigrams/bigrams separately for each retained span: removing
fields must not create compounds. Merge adjacent spans of the same role to retain
within-field line continuations. Use the previous c-TF-IDF scorer, record-DF-weighted
c-TF-IDF, and that weighting with lexical containment diversity (not semantic MMR),
top eight, maximum 8,192 features. DF counts original records, not selected fragments.
Vocabulary/background are transductive on the eight records, not independent or
online-maintained corpus statistics. Full-payload c-TF-IDF must exactly reproduce
the prior terms and scores before comparisons are accepted.

## Results

Two fresh captures reproduce every nonmeasurement field, including scores,
source-role audits, lost terms and fixed memberships, across all nine controls.

| Selected source | Characters | Vocabulary | Counted terms | Missing target terms* | c-TF-IDF labels / metadata-only |
| --- | ---: | ---: | ---: | ---: | ---: |
| Full payload | 2,292 | 71 | 193 | 0 | 32 / 16 |
| Target + feedback + unknown | 625 | 39 | 59 | 0 | 25 / 0 |
| Feedback + unknown | 423 | 28 | 37 | 18 | 21 / 0 |

*Distinct source-adjacent target terms missing from retained text, summed per
record; the 18 occurrences affect seven of eight records. This is a retention
diagnostic, not 18 independently judged meaningful concepts. Counts include bigrams.
Characters are Unicode code points, not UTF-8 bytes or model tokens.

Record-IDF yields metadata-only counts 14/32, 0/25 and 0/21 respectively;
containment diversity yields 16/32, 0/22 and 0/19. Every returned phrase has exact
retained-source support; none lacks single-role support in these captures.
Zero metadata-only labels after filtering is largely enforced by construction,
not independent evidence of improved semantic label quality.

The full-payload mixed group includes `astro-ytufwdmq`, `location`, numeric viewport
values and `feedback`. Target-inclusive labels instead include `codebase`,
`architecture`, `codebase layout` and `ai policy`. Yet this group still mixes
sidebar-hierarchy and heading changes; clean words do not fix its memberships.
Feedback-only loses `fact model` from one target, even while retaining `inferred facts`.
Other remaining candidates such as `like`, `s` and `node` show that field removal
does not solve vocabulary or label specificity. Retain target text for this
format; do not generalize these observations to arbitrary provider records.

## Cost and memory limitations

Parsing 0.464–0.523 ms; all three scorers together per source selection:
full payload 4.98–5.91 ms, target-inclusive 2.24–2.44 ms, feedback-only 1.60–1.87 ms.
Whole experiment after imports 16.39–18.72 ms, CPU 16.29–17.77 ms, zero model calls.
These are small label-only batch costs, not full-ingestion or online-update costs.

Linux `/proc/self/status` VmHWM reports 132.24–132.63 MiB including imports.
`getrusage(RUSAGE_SELF).ru_maxrss` simultaneously reports 955.94 MiB in both
processes. A separately launched phase probe measured approximately 132 MiB
after imports and 132 MiB after actual input validation/labels. The two memory
counters disagree for the captured launches; preserve both rather than attribute
956 MiB to this workload. The cause is not established. Earlier captures only
recorded rusage, so their memory attribution needs remeasurement before a gate.

## Reproduction and next evidence

`field_labels.py --packets ... --review ... --memberships ... --output ...` uses
immutable source/review/prior capture hashes, exact original envelope bounds and
a complete disjoint membership partition. `compare_field_labels.py --first ...
--second ... --output ...` rejects missing selections/scorers/sources and any
nonmeasurement drift. Public synthetic tests cover Unicode/CRLF, unknown layouts,
code markers, separate phrase counting and corrupted comparison grids. All 125
lab tests pass. No Rust files changed.

Private root: `topic-python-review-v3-20261001` under the durable artifact directory.
Final capture SHA256:
`6c87f1b8d1819404a9332f8bf5779eeab20fb244f25ad25602ce6e8d6aa5013c`
and `0eba3f439c20e76681e0b37f514b68d5ff69faa4b58d826d9f165f330f62c313`.
The first exploratory capture predates target-loss/memory diagnostics and is
excluded. Private bodies and candidate tables remain outside Git.

Next needs independent label/source-role judgments, wider formats, native-context
review for unresolved referents, stable background statistics and complete
topics-enabled ingestion measurements. No semantic parents, inherited tags,
guided-search improvement or <=10% ingestion gate is established.
