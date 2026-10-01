# Bounded complete question units (development diagnostic)

## Procedure

Continue the source-pinned 128-question experiment described in
[body representations](2026-10-02-question-body-representations.md). Both 64-question
phases are already exposed development samples; neither is a fresh held-out test.
The same 72 disjoint background questions and saved sparse/title features remain
unchanged. Community tag overlap is an incomplete topical proxy, not independent
agent-session annotation. No answers, reference tags or generated text enter inputs.

The procedure was recorded in `mash-c0d.19.2.25` before unit/model outcomes.
`bounded_question_units.py` keeps the exact title plus complete displayed body
as one input when its actual tokenizer length is 2–512. Otherwise it splits at
recorded block-separator endpoints, recursively divides oversized spans at the
nearest interior whitespace endpoint to the midpoint (smaller position wins
ties), and falls back to a Unicode-codepoint midpoint. Every resulting original
substring is checked with the frozen tokenizer; no monotonic length assumption,
token decoding, rewriting or prefix truncation. Each modeled input prepends the
original title and two newlines. Whitespace-only spans remain explicit without
model calls. Every displayed character and UTF-8 byte is covered; units reference
the existing complete HTML projection records, including its documented exclusions.

Reject the entire question if any nonempty unit cannot fit or there are more
than 32 modeled occurrences. Retain the complete rejected plan. Preflight at
most 1024 unique planned/cache strings; never resample or raise caps after outcomes.
Reuse the prior 77 complete-body vectors only after verifying exact input hashes,
the frozen body capture, code/dependency/model/tokenizer contracts and finite
normalized 384D directions. Singleton CPU inference uses only installed assets.
Identical inputs are deduplicated per question for scoring, while all source
occurrences are preserved. No clustering, cut selection or generative calls.

Compare title cosine, fixed 75% title/25% lexical cosine, normalized mean of
unit directions, maximum cross-unit cosine, and fixed 75% dense/25% lexical
fusions of mean/max. All views use identical question pairs. Repeated titles
inside units and maximum-score multiplicity may bias scores; these mechanical
units are not validated semantic episodes or proven primary/secondary topics.

## Coverage and ranking

All 128 questions are supported, recovering the 51 whole-body window failures.
There are 485 modeled occurrences, 36 whitespace spans, one per-question scoring
duplicate, 484 unique vectors and 597 planning tokenizations. The 77 short
questions retain exactly their original single input/vector. The other 51 use
3–23 modeled units; no cap rejection, lost body tail or resampling.

Each phase has 2016 pairs, with 97/51 tag-overlap positives respectively.
AP/AUC are ranking diagnostics; there is no operating cutoff or precision claim.

| View | Calibration-source AP / AUC | Former reserved-source AP / AUC |
| --- | --- | --- |
| Title | .11660 / .64290 | .18976 / .74151 |
| Title75lex25 | .12623 / .66853 | .20383 / .75461 |
| Unit mean | .15383 / .77057 | .19690 / .80555 |
| Unit maximum | .16757 / .77484 | .21284 / .82317 |
| Mean75lex25 | .15879 / .77843 | .20366 / .81721 |
| Max75lex25 | .17295 / .77832 | .21177 / .83267 |

Unit maximum improves AP over titles by .05097/.02308; lexical fusion adds
.00537 in the first phase but loses .00107 in the second. Mean fusion barely
trails the cheap title fusion in the second phase. These mixed outcomes on
exposed samples do not select a winning representation. Multiple vectors retain
more source content but do not establish meaningful overlapping tags, semantic
ancestor labels, guided-search gains or robust false-match control.

## Cost and verification

Two final fresh-process runs take 5.650/5.743 seconds after imports, including
source/contract loading, model construction and scoring, excluding output
serialization. Planning takes 355.48/356.83 ms; additional inference
4663.86/4762.52 ms; encoder tokenization 147.02/150.36 ms; scoring 142.26/137.22 ms.
Both require 407 additional singleton calls and 77 cache hits, maximum 508 tokens.
Reuse is exact, but this remains substantial additional model work. No ingestion
throughput, initial/incremental lifecycle, CPU/RSS or ≤10% overhead gate is proved.
Evaluate reuse of existing source-unit vectors and lower-cost representations
before considering production adoption; freeze any changed method before fresh
evaluation. Do not infer performance on either MacBook from this CPU experiment.

Final repeats match every non-timing field. Independent reconstruction checks all
128 original body ranges, UTF-8 coordinates, piece references and input hashes,
all 77 reused vectors exactly, normalized means, 4032 pair truths and 24,192
scores. All 12 AP/AUC values are independently recomputed with explicit tie
blocks. The lab passes 231 tests, including Unicode/codepoint fallback, complete
coverage on cap rejection, nonmonotonic token counts and JSON schema round-trips.

The initial capture is preserved. Before repeating, tuple-valued internal plans
were explicitly converted to schema arrays at the output boundary; this prevents
JSON decoding from creating a false comparison failure. Independent comparison
confirms all source/vector/score outcomes match the initial capture; only code
pins and timings differ. This is Python self-repeat evidence, **not Rust parity**.

Captures outside Git under `topic-public-stackoverflow-20261002`:

- Final first: `ede6119b31439830c7cbf1f01f53e185e8cffe3a4957722b23bb0b205b1850cd`.
- Final repeat: `2f6275cb4cc4cbf5b2016c34c452216f2e16348327cf63c8aadd9bb84e1e2fe2`.

Public questions are complementary evidence; independently reviewed coding
threads, topic/ancestor quality, lifecycle and complete-ingestion validation
remain required before selecting and porting a method.
