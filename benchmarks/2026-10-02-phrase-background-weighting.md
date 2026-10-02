# General-English background weighting: mixed evidence, more unknown atoms

## Procedure and provenance

Use the same exposed eight-thread, 68-record oracle-admitted review and
separate 128-thread, 1,850-source coding cohort. Nine primary/nine secondary
subjects come from the familiar single-assistant review, not independent gold.
Require exact snapshots, metrics and lifecycle summaries for all three prior
arms against the frozen phatic capture. No new source selection, embedding,
LLM, model download or paid call occurs.

Add optional research dependency `wordfreq==3.1.1`. Its English `large` frequency
dictionary is external background data, not a learned conversation model.
Credit Robyn Speer and the underlying SUBTLEX work by Marc Brysbaert et al.;
SUBTLEX is freely available data. See [wordfreq documentation](https://github.com/rspeer/wordfreq),
[notice](https://github.com/rspeer/wordfreq/blob/master/NOTICE.md) and
[release](https://pypi.org/project/wordfreq/3.1.1/). The distribution contains
mixed-source data with an approximately 2021 language snapshot; it is not a
current technical dictionary. No frequency data is copied into Git.

Before examining results, freeze the weighting rule: for each exact scanner
atom with probability p (zero if absent), compute
`max(1,min(9,-log10(max(p,1e-9))))/9`. Multiply the existing thread phrase weight
by the mean atom factor. This uses exact dictionary lookup, not library
tokenization or a joint phrase-probability estimate. Cache atom factors once
per refresh. Apply the same source top-two shortlist, half score/coverage
utility, containment/ties, DF, vocabulary and eight-tag cap. Background and
phatic filtering are separate arms; do not combine or retune them.

New output scores explicitly include background weighting. Original scores,
IDs and outputs stay unchanged for the other policies. All original source
records/evidence survive. Unknown atoms have bounded maximum weight: technical
identifiers may benefit, but typos, debris and opaque strings may too.

The English asset contains 321,180 entries, SHA256
`dffae8066b78dce0a6667cf5f58e567054f902674667090a7ac8a8a44628b05c`;
API source SHA256 `a50df96622a21a7bcab6a8fc8a8ce7abff8cb47e5dcf532341215640c337c1d0`.
The driver checks these identities again after fitting; actual package/data
source was inspected. Failed guesses at a GitHub release tag were not used.

## Results

| Diagnostic | Source coverage | Phatic coverage | Background coverage |
| --- | ---: | ---: | ---: |
| Primary evidence recall | .986111 | .986111 | 1.000000 |
| Secondary evidence recall | .796296 | .796296 | .740741 |
| Primary best single-phrase support F1 | .734512 | .734512 | .720370 |
| Secondary best single-phrase support F1 | .559788 | .559788 | .589418 |
| Primary / secondary name-atom hits / 9 | 3 / 5 | 3 / 5 | 3 / 4 |
| Secondary subjects without evidence | 1 | 1 | 1 |
| Reviewed referential-only topics | 10 | 9 | 11 |
| Reviewed topics / tags | 48 / 48 | 48 / 48 | 50 / 50 |
| Broad topics / tags | 442 / 720 | 442 / 718 | 430 / 725 |
| Broad recurring / provisional topics | 291 / 151 | 288 / 154 | 273 / 157 |
| Broad sources without evidence | 769 | 776 | 822 |
| Broad multi-tag sources | 413 | 411 | 413 |
| Reviewed topics with all atoms outside dictionary | 6 | 7 | 15 |
| Broad topics with all atoms outside dictionary | 83 | 86 | 180 |

All threads remain tagged. Ranked topic-ID lists change for all eight reviewed
threads and 124/128 broad threads relative to source coverage. Evidence-covered
broad records fall from 1,081 to 1,028. The rule strongly changes selection and
increases entirely out-of-dictionary topics, but these are not proven bad topics:
identifiers can legitimately be absent. Primary support F1 and secondary recall
decline while secondary support F1 increases. Name overlap and referential-only
counts do not show a consistent improvement. This mixed result is insufficient
for promotion; do not tune bounds on this exposed sample.

All metrics remain evidence proxies, not semantic precision/recall or guided
agent usefulness. Unrelated phrases on multi-topic support records can count
toward recall. Independent label/multitag review, full-ingestion overhead and
selected Rust differential parity remain open. Repeated lexical weighting
controls have not resolved specificity or reference handling; a stronger
source-grounded quality comparison is needed before further scoring changes.

## Verification and cost

Two runs match every non-runtime field, including frequency provenance. The
new arm passes duplicate vote/original-tag invariance (8/128 duplicate inputs),
cleanup, real-source deletion/absence, deleted restart, reinsertion, refreshed
restart and reverse final semantics. The source ledger does not serialize the
background dictionary; its caller reloads the explicitly identified frame.
This is a research contract, not production persistence or independent replay.

A no-producer-import audit independently decompresses the pinned cB/msgpack
asset and derives probabilities from bucket positions. It reads original
SQLite/review sources and checks all eight snapshots: weights, exclusions,
shortlists, votes, utility/ties, identities, evidence and reviewed metrics.
All 3,379 selected scores/ranks have zero error; F1 error is 1.11e-16. Shared
frequency data, declared formula, static stoplist and review inputs remain
dependencies. No semantic gold or independent native/lifecycle verification.
The 658-test suite passes in 4.017s; fake-frame tests validate bounds, phrase
means, invalid probabilities/missing frames, and corruption rejection across
all four policies without requiring wordfreq assets.

Initial frequency loading takes 444.18/1,492.05ms. Reviewed background refresh/
evaluation is 25.33/15.97ms. Broad background refresh is 3,364.41/3,928.29ms
versus unfiltered coverage 3,393.54/5,362.18ms. Whole four-arm/lifecycle
experiment is 95.331/129.276s wall and 94.469/106.758s CPU, including frequency
and broad loading, excluding imports/initial reviewed loading, serialization,
repeat comparison and the final asset recheck. Host scheduling varies strongly;
these are not full-ingestion throughput, p95 updates or isolated worker costs.

Sampled target RSS/status HWM is 228,716/230,904 KiB, PSS
218,611/219,754 KiB; pre-serialization rusage 228,448/227,468 KiB. There are
3,352/4,239 samples, one/two vanished reads and maximum gaps 58.85/111.79ms.
Samples are lower bounds, status approximate, observer intrusive, children/
observer excluded and launcher history may affect rusage. Do not subtract
peaks or infer <=10% ingestion overhead from this experiment.

Actual science environment remains Python3.11.15, NumPy2.4.6, SciPy1.17.1,
sklearn1.9.1. Installed wordfreq3.1.1, msgpack1.2.3, regex2026.9.29,
langcodes3.5.1, ftfy6.3.1 and locate1.1.1 match their new optional lock entries.
The pre-existing general research lock selects NumPy2.5.3/SciPy1.18.1, unlike
this long-lived experiment environment. Do not claim `uv sync` reproduces
these runs without installing the stated scientific versions. Follow-up bead
`mash-c0d.19.2.39` tracks executable environment reconciliation while preserving
original captures. The optional addition did not upgrade the actual science
environment or alter the original arms' scientific outputs.

Private result SHA256:

- First: `0eaad7d465c19b0067e89ef573b0ee2fcdb2b86b5cf7370d67eddf2530457613`.
- Repeat: `486bff514048c7d1d908abd045e91a15d55c00c8f88531ef3aa9d97a05f2d0e2`.

Artifacts use `phrase-background-{first,repeat}-20261002`, memory sidecars/logs,
audit/test logs, and embedded original input/code/baseline identities. No raw
sources, labels or credentials enter Git. No production method or Rust port
selected; the overall production-readiness goal remains unproven.
