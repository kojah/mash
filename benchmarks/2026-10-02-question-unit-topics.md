# Complete question units: fixed topic memberships and lexical labels

The complete-unit representation has ranking signal, but the transferred topic
cuts still fragment these programming questions. Giving titles and bodies equal
label mass improves exact tag naming with memberships fixed. Neither result
establishes useful overlapping topics, semantic ancestors or a production method.

## Frozen procedure

Reuse the [complete-unit capture](2026-10-02-bounded-question-units.md), SHA256
`ede6119b31439830c7cbf1f01f53e185e8cffe3a4957722b23bb0b205b1850cd`.
The same source-only split has 64 questions per exposed phase and 72 disjoint
background questions. Community tags remain evaluation-only. The phase names
`calibration-source` and `reserved-source` describe the original split; both
outcomes have already been exposed in previous diagnostics. This is not fresh
held-out evaluation or a new calibration run.

Before outcomes, freeze the existing GRINCH-style average-cosine cut 0.4 and
normalized-centroid cut 0.6, without target-corpus tuning. Compare one normalized
unit mean per question against the deduplicated exact unit vectors. A question
inherits the union of its units' direct native topics, so overlapping membership
is possible. Execute forward, reverse and seed-17 shuffled arrivals: 24 trees.
Retain complete point partitions, question memberships, pair predictions, tree
statistics and arrival stability. Do not interpret structural branches as
semantic ancestors. This is our exact GRINCH-style prototype, not author-reference
GRINCH equivalence or a Rust implementation.

For labels, count original titles once per parent per topic and distinct original
body unit substrings separately. Never count the title injected into each model
input repeatedly. Fit a source-only capped 8,192-term technical unigram/adjacent
bigram vocabulary on selected questions plus background; this vocabulary is
transductive, not an online maintenance policy. The 756 source rows comprise
128 titles, 484 distinct body units, and 144 background title/body rows. Reset
bigrams at source boundaries. Sum background title/body counts per question
before computing its single document-frequency vote.

Compare existing c-TF-IDF against c-TF-IDF multiplied by smooth background IDF,
`log((1 + 72)/(1 + DF)) + 1`. Sort positive scores with explicit lexical ties.
Emit eight terms per topic and also enforce a matched total-eight-term budget
per question, ranking each term's maximum score across its assigned topics.
Validate that labels occur in original source. Community tags, names and aliases
do not influence vocabulary, score, membership or ranking.

## Topic grouping fails the useful-overlap test

Each phase has 2,016 natural question pairs; positive shared-tag pairs are 97
and 51 respectively. Forward outcomes are:

| Phase | Representation / linkage | Topics | Single-parent topics | TP / FP | Recall |
| --- | --- | ---: | ---: | ---: | ---: |
| Calibration source | Mean / average | 56 | 49 | 1 / 8 | .0103 |
| Calibration source | Mean / centroid | 64 | 64 | 0 / 0 | .0000 |
| Calibration source | Units / average | 58 | 52 | 0 / 6 | .0000 |
| Calibration source | Units / centroid | 65 | 65 | 0 / 0 | .0000 |
| Reserved source | Mean / average | 58 | 53 | 4 / 3 | .0784 |
| Reserved source | Mean / centroid | 64 | 64 | 0 / 0 | .0000 |
| Reserved source | Units / average | 59 | 53 | 3 / 3 | .0588 |
| Reserved source | Units / centroid | 66 | 65 | 1 / 0 | .0196 |

The 95th percentile question has one direct topic in every forward run; the
maximum is only one or two. Extra unit vectors do not produce a useful
multi-topic taxonomy at these cuts. Mean partitions repeat across all three
orders. Unit-average point ARI is .9919–.9990 for calibration and .9729–.9899 for
reserved alternative orders, while positive question-pair Jaccard falls to
.5714 and .8333 respectively. High ARI here reflects fragmentation as well as
stability. Empty/empty positive-pair comparisons are undefined, not perfect.
The one-match reserved unit-centroid precision of 1.0 is not robust quality.

## Naming ceiling and label-only follow-up

The bounded vocabulary contains 78 of 208 calibration tags and 95 of 204
reserved tags in their own question: 173/412. A subsequent complete-source audit
finds 204 exact source-supported tags; the existing whole-record analyzer
retains 183, and the vocabulary cap removes another 10. No additional loss is
attributable to unit boundaries in this tag inventory. Exact names are a limited
lexical proxy: synonymous labels, unmentioned technologies and community tag
conventions need not match. They do not measure semantic label precision.

After these outcomes, preregister a label-only follow-up on the eight frozen
forward partitions. Keep all vectors, memberships, source rows, vocabulary,
background and ranking budgets fixed. Compare title-only normalized bags with
equal title/body mass: L1-normalize each parent's title and sum of distinct
assigned body bags separately, average nonempty components, then sum parent
contributions per topic. Run both scorers on all 16 controls; no new inference.
Title-only has no general chat coverage guarantee. Equal mass uses every assigned
body bag without letting long bodies dominate title counts.

The existing scorer is retained exactly: row-length and corpus-term-frequency
denominators have a floor of one. For fractional normalized bags, the frequency
floor matters; this is the frozen implementation's score, not an unclipped
standard fractional c-TF-IDF formula.

Matched top-eight exact tag hits for average-linkage partitions:

| Phase / representation | Raw c-TF-IDF / background IDF | Title-only | Equal title/body mass |
| --- | ---: | ---: | ---: |
| Calibration / mean | 27 / 28 | 35 / 39 | 38 / 40 |
| Calibration / units | 28 / 29 | 35 / 39 | 38 / 40 |
| Reserved / mean | 34 / 33 | 57 / 59 | 60 / 60 |
| Reserved / units | 34 / 33 | 57 / 59 | 60 / 60 |

Raw and equal-mass rows emit 512 terms per phase under this budget. Title-only
emits 434–447, depending on the partition. Union-of-topic budgets can emit more
terms and are retained separately in captures. Across the two mean-average
cohorts, equal mass increases raw exact hits from 61 to 98; background-weighted
hits increase from 61 to 100. Background IDF alone is mixed, including losses on
reserved raw counts. Improvements describe label salience under largely
single-question topics; they do not repair poor grouping. Centroid controls and
all order outcomes remain in the captures, without selecting a winner.

## Cost, verification and reproducibility

Fresh processes repeat every non-timing field exactly. Topic runs take
15.531/18.768 seconds after imports; label-only runs take 9.079/9.333 seconds.
In the first topic run, forward tree fitting takes roughly 18–26 ms per mean
control and 81–151 ms per unit control. Label work takes about 495–602 ms per
control, exceeding these small tree-fitting costs. This includes source checks
and ranking; it is not a profile attributing all cost to one function. No RSS,
ingestion persistence or production p95 update gate is measured here. Reusing
vectors adds no inference in this stage, but the prior 407 model calls and about
4.7 seconds of inference remain charged to the unit representation.

Independent reconstruction validates all 756 sparse source count records from
original text, 24 complete point partitions, 48,384 pair decisions, native
question unions, raw and fractional count hashes, score formulas, deterministic
label and matched-budget rankings, exact naming totals and whole-parent source
support for every emitted label. Both fresh-process repeats match after removing
only the top-level timing field. All 252 lab tests pass. Tests cover title
deduplication, overlapping memberships, reference isolation, label support,
budgeting, body-length normalization and empty-source semantics. This is Python
research validation; the Rust/Python differential CI gate remains unimplemented.

Captures outside Git, under `topic-public-stackoverflow-20261002`:

- `topics-first.json`: `de9b34a307d358c0458c1d55899c18cffedd9ea23f9e97d3dcea00751026898a`.
- `topics-second.json`: `9289d186e1f5b30475353764f60c1ebc89ebf1e7557a4e57edf80bab6a7169d7`.
- `label-balance-first.json`: `6f9a46d7d0f9f1d58fed4bc37725c16e4af01b59e14676b6d9706e945918db4f`.
- `label-balance-second.json`: `ca7c12cea3a5a458c49d5a88289026e4339b262e801cc124f5b42138121de578`.
- `topics-independent-verification.log` and `topics-balance-tests.log` retain checks.

Invoke `question_topics.py` with `--root`, `--capture` pointing at the complete-unit
capture, and a fresh outside-Git `--output`; repeat with `--compare`. Invoke
`question_label_balance.py` with the original topic `--capture`, original
`--unit-capture`, the same root, and fresh output, then repeat with `--compare`.
Exact code hashes, source hashes, frozen cuts and vocabulary guard against drift.

Next measure and optimize label ranking with exact output parity, then evaluate
target-representation calibration on genuinely new groups and independent
primary/secondary/ancestor judgments. Do not loosen these cuts on exposed tags
and report the resulting fit as held-out quality. The complete-ingestion ≤10%
overhead target and lifecycle, semantic quality and guided-search gates remain
open in `mash-c0d.19.2.22`, `.25`, `.26` and `.33`.
