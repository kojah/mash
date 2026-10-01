# Source-grounded question bodies and fixed lexical controls

Full-body embeddings lose too much coverage to adopt: 51 of 128 question inputs
exceed the model window. Matched-sample ranking gains are mixed and based on only
26/9 shared-tag pairs. A post-hoc full-support title/lexical fusion gives small
AP gains without further inference, but needs independent confirmation. No topic
method, label pipeline or production behavior is selected.

Tracked by `mash-c0d.19.2.33`; follows the
[programming-title diagnostic](2026-10-02-programming-title-transfer.md).
Both question samples are already exposed **development outcomes**, including the
phase named `reserved-source`. No generative calls, model downloads or paid APIs.
Question tags remain incomplete external metadata, not agent-session topic gold.

## Frozen extraction and comparison

Preregistered before body/model outcomes. `question_html.py` uses Python's standard
HTML parser with explicit events. Preserve data, code, whitespace and decoded
entities; add newline separators at block boundaries. Attributes/markup/comments
are explicit exclusions, never model text. No filler filtering or semantic
rewriting. Active/style/declaration layouts are unsupported rather than silently
partially extracted. Caps: 512 KiB source, 65,536 rendered bytes, 4,096 pieces.

Every piece records kind, source and rendered code-point/UTF-8 intervals and
both hashes. Non-separator source intervals must cover the complete original HTML
exactly once; separator provenance can reference an already-accounted markup
interval. All 200 bodies render successfully. They yield 23,107 pieces:
10,286 data, 6,700 entities, 5,101 excluded markup and 1,020 separators. No actual
comments occur in this capture; comment exclusion is covered by tests. Maximum
rendered size is 24,674 bytes and maximum piece count 1,388.
This is a declared text projection rather than browser rendering. Image content,
attribute/link context and table-cell layout need separate semantic validation
before broader extraction reuse.

Two pre-experiment issues were caught by tests: a method name collided with the
base parser's `offset` attribute, and an unterminated entity suffix was correctly
emitted as literal data rather than triggering the test's expected rejection.
Rename the method and correct that test; preserve literal suffixes. No corpus
outcome-driven extraction rules or threshold changes were added.

`question_representations.py` requires the original title-capture SHA
`a77d5efeb2ce4a816419bf6ed3efc1c5bb0dfcde26988c91c6280293db619333` and verifies
its full code/dependency/source contract. All original 128 title vectors, pair
scores and metrics must reproduce exactly. Join original title, two newlines
and the full displayed body. Explicit installed/pinned MiniLM, singleton
one-thread CPU, mean pooling, symmetric signed-byte directions, no padding or
truncation, 2–512-token support and a 128-unique-input preflight cap. Preserve
source/input hashes and abstentions. Exact cross-phase joined inputs are excluded.

Fit the unchanged word uni/bigram TF-IDF policy (minimum DF 2, maximum 8,192,
smooth IDF, sublinear TF, L2, no stoplist) on the 72 disjoint background joined
questions, excluding exact selected inputs/duplicates. No exclusions are needed
here. Vocabulary 1,725, background 7,619 nonzeros; all 128 selected questions have
nonzero sparse features. No tags, scores or ownership metadata enter fitting.

Five fixed views on common support: saved title, title+body, lexical full question,
75% title/25% lexical, 75% joined/25% lexical. Score cosine AP/AUC only; no new
clustering, cuts or outcome-based method selection. Label quality, overlapping
memberships, ancestors and guided search are not evaluated.

## Coverage and matched ranking

| Development phase | Original questions | Supported joined | Window abstentions | Matched pairs | Matched shared-tag pairs | Original shared-tag pairs |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Calibration-source | 64 | 39 | 25 | 741 | 26 | 97 |
| Former reserved-source | 64 | 38 | 26 | 703 | 9 | 51 |

The token limit retains 60.2% of questions but only 26.8%/17.6% of the respective
original positive pairs. Common-support gains cannot establish whole-corpus
quality. All 200 extraction records pass; no rendering failure or zero lexical
vector contributes to the coverage loss.

| View | Calibration AP | Calibration AUC | Former reserved AP | Former reserved AUC |
| --- | ---: | ---: | ---: | ---: |
| Title | .1018 | .6375 | .1608 | .8807 |
| Title+body | .1118 | .7824 | .2031 | .8599 |
| Lexical full question | .0581 | .5893 | .0419 | .7858 |
| Title 75 / lexical 25 | .0993 | .6515 | .2330 | .8905 |
| Joined 75 / lexical 25 | .1090 | .7832 | .2125 | .8705 |

Joined AP improves on both small matched samples, but former-reserved AUC falls;
title fusion also has a calibration AP loss. Lexical alone is weaker here.
These are dependent, sparse development comparisons, not significance or semantic
quality evidence. Whole-support original title AP remains .1166/.1898, exactly
unchanged; do not compare those directly with differently supported body results.

After observing the window loss, `question_body_support.py` performs a separately
disclosed **post-hoc support audit** using saved features, without refitting,
parameter changes or inference. Preserve all 64 questions/2,016 pairs per phase
and the fixed 75/25 title-fusion rule:

| Full-support view | Calibration AP | Calibration AUC | Former reserved AP | Former reserved AUC |
| --- | ---: | ---: | ---: | ---: |
| Original title | .1166 | .6429 | .1898 | .7415 |
| Lexical full question | .1601 | .6604 | .1192 | .7182 |
| Title 75 / lexical 25 | .1262 | .6685 | .2038 | .7546 |

Small fusion gains preserve coverage, but the audit is post-hoc and both source
phases are exposed. No winning representation is selected. Need bounded,
source-grounded units with explicit complete coverage and newly reserved
evaluation; preferentially reuse compatible existing vectors. Any additional
encoder work needs justified quality gains and full ingestion measurement.

## Cost, reproducibility and limitations

Two runs: 3.057/2.425 seconds total, rendering 432.17/309.49 ms, lexical fit/transform
78.45/55.64 ms, additional body inference 2046.55/1673.99 ms. There are 77 unique
body calls, zero cache hits, maximum supported input 501 tokens. Totals include
model construction, source/contract loading, validation and scoring; output
serialization is excluded. Fragment hashes/offsets are lab diagnostics and
their cost is included. These are not initial/incremental ingestion or peak RSS
measurements; no ≤10% overhead gate, speedup or cross-hardware inference claim.

Both body repeats match every non-timing field. Independent reconstruction from
the original HTML verifies all 200 records' complete source coverage, decoded
content, UTF-8/rendered offsets and hashes, model input identities and background
isolation. Recompute all 10 matched AP/AUC values independently with score-tie
blocks; verify pair truth and full original title baselines. The full-support audit
also repeats byte-for-byte. Research suite: 224 passing tests, including Unicode,
CRLF, combining marks, numeric/unknown entities, source exclusions, resource caps,
source-pinned input construction and fixed score weights.

Captures stay outside Git under `topic-public-stackoverflow-20261002`:

- Body first (9,613,714 bytes): `057763eb18aea347ad3740495e1e8c368fab2172f016868cbe9c69fc19f87f7f`.
- Body repeat: `1846fe1015426f8021680be0c420a5bb44776123d3733094a0b99064b7115877`.
- Full-support audit/repeat: `8505f3d437bd63f74715e349637d6859c32707c2772974a531709166e4a1e31c`.

External question tags, sparse sampling, evolving posts, unknown generative
assistance/pretraining overlap and missing full agent dialogue context remain.
No human label/ancestor review, online lifecycle, persisted topic tree or Rust
parity gate is completed by this experiment.
