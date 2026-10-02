# Bounded semantic MMR labels: inexpensive, mixed quality proxies

Bead `mash-c0d.19.2.30.4.5`. With the existing topics fixed, semantic MMR
reduces label-pair cosine at a small measured phrase-encoding cost. It also
reduces lexical relevance and document coherence, and does not consistently
improve cross-topic term diversity. No semantic-quality or production promotion
gate passes from these objective/proxy measurements.

## Prospective procedure

Reuse all four verified [PCA25 memberships](2026-10-02-fixed-pca-topic-routing.md),
original vocabulary, candidate counts and sparse class counts. Derive each
topic's top 32 positive corpus-IDF candidates, ties lexical; require its first
eight to equal the archived baseline. No topic geometry, document tags,
source text or lexical scoring formula changes. Candidate selection occurs
before inference; 62 pools yield 495 unique phrases, below the frozen 1,024 cap.
Read only the original candidate corpus, with exact manifest/source hashes;
queries, duplicate judgments and reserved outcomes are never read as features.

The [original MMR work](https://doi.org/10.1145/290941.291025) combines relevance
and redundancy. This label-only adaptation uses corpus-IDF relevance divided
by the maximum in each pool, lambda .6, eight greedy selections:
`objective = .6 * relative relevance - .4 * maximum signed cosine to selected`.
First-step redundancy is zero; ties lexical. No extra containment rule,
stopword exceptions, similarity clipping or threshold repair is applied.
Compare plain and lexical-containment controls on the same pool. All archived
diverse terms fall within their pools, so the cap does not exclude those baseline
terms on this cohort. This is not a reproduction of the original retrieval or
summarization experiment.

Encode each unique casefolded source-supported phrase once using the installed
quantized MiniLM, CPU singleton inference, one intra/inter-op thread, exact
tokenizer IDs, float32 mean pooling including attended special tokens, and the
existing int8-direction rounding/float64 normalization. Complete phrase inputs
have at most eight tokens, below the 512 cap; no truncation or new model download.
This is a non-generative local encoder, with no paid/generative call. Retain all
495 × 384 vectors, token IDs, asset hashes and every selection step.

Record exact original title/body source spans in code-point and UTF-8 byte
coordinates, source hashes and distinct supporting-document counts for every
selected phrase. Measure [unchanged document-presence NPMI](2026-10-02-topic-label-coherence.md),
mean pair cosine, mean normalized lexical relevance and cross-topic unique-term
ratio. These are diagnostics/objectives, not independent human labels.

## Outcomes

| Order / membership | Plain / contained / MMR pair cosine | Plain / contained / MMR NPMI | Plain / contained / MMR relative relevance |
| --- | --- | --- | --- |
| Forward / hard | .4284 / .3942 / .3430 | .2717 / .2426 / .2163 | .6550 / .6471 / .6275 |
| Forward / overlap | .4267 / .3932 / .3480 | .2809 / .2494 / .2288 | .6537 / .6442 / .6284 |
| Reverse / hard | .4082 / .3801 / .3359 | .2649 / .2405 / .2196 | .6470 / .6403 / .6258 |
| Reverse / overlap | .4225 / .3830 / .3302 | .2911 / .2449 / .2440 | .6387 / .6307 / .6103 |

MMR unique-term ratios are .81875/.82500/.88636/.92045, versus plain
.86250/.86875/.92045/.94318 and containment .85000/.86250/.90909/.92045.
Within-list semantic diversity and cross-topic vocabulary diversity differ;
optimizing the former can worsen the latter. Lower selected cosine is expected
from the objective and does not independently demonstrate better topic names.

Exploratory inspection uses previously identified testing, agile and numeric/code
groups, not new human gold. Forward testing changes from `test, tests, unit,
testing, unit tests, unit testing, unit test, tdd` to `test, unit, tests, tdd,
testing, integration, public, write`. Containment instead gives `test, tests,
unit, testing, tdd, mock, integration, coverage`. MMR still keeps testing variants
and admits generic `public`/`write`; it is not a clear naming improvement.
Forward agile substitutes `stories` for `sprint` while retaining `we`.
Reverse numeric/code labels retain several one-character/numeric terms. No
outcome-specific stopword, numeric or synonym patch is introduced.

## Verification and measured costs

Fresh full-inference repeats match all decoded nontiming fields and the entire
phrase-vector file hash. 366 tests pass, including relevance/redundancy tradeoff,
deterministic ties, signed negative cosine handling and invalid candidate pools.
The independent audit checks lexical candidate ranking/formulas, greedy MMR
steps with alternate component-product cosine reduction, source membership/counts,
original Unicode/byte spans/hashes, pair diagnostics and macro summaries. The
Audit covers 62 pools, 496 greedy steps, 1,488 source supports and 5,208
selected phrase pairs; maximum lexical/objective differences are
5.56e-17/3.34e-16. The first checker invocation stopped on the BEIR `text` versus logical `body` field
mapping; correcting the checker leaves experiment captures unchanged.

Installed model SHA256 `afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`,
tokenizer `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`;
ORT 1.30.0, tokenizers .23.2, same Linux/AMD EPYC/science environment as prior
studies. First/repeat after-import wall time 4.041/4.046 s; process CPU
3.973/3.979 s. Setup 3.020/3.157 s, encoder startup .100/.097 s, complete
495-phrase encoding .369/.253 s (inference .313/.223 s; tokenizer
.012/.007 s), selection/support/diagnostics .544/.532 s. These batch refresh
costs are not per-message overhead or evidence for the ≤10% full-ingestion gate.
An incremental phrase cache could amortize inference, but update frequency,
persistence, invalidation and complete ingestion are unmeasured.

External first/repeat monitors have 226/227 samples, zero/one vanished reads,
20 ms requested sleep; actual median/max gaps 26.59/34.94 and 26.48/30.68 ms.
Smaps RSS maxima 491,348/491,392 KiB, PSS 483,814/483,881 KiB; approximate
status VmHWM 491,340/491,384 KiB; separate rusage 491,056/490,968 KiB.
Sampled maxima are lower bounds, target-only scope excludes children/observer;
retained corpus/counts/model, imports and monitoring contribute. No production
worker-memory, controlled speedup or full-ingestion claim follows.

## Reproduction and decision

Run `cqadup_semantic_labels.py --topics PCA_FIRST --topic-repeat PCA_SECOND
--source VERIFIED_CANDIDATE_ROOT --snapshot EXPLICIT_INSTALLED_SNAPSHOT
--output-directory FRESH_DIRECTORY`, wrapped by `memory_audit.py`.
Fresh repeat adds `--compare FIRST_DIRECTORY/results.json`. Outside-Git captures:

- First `topic-cqadup-semantic-labels-first-20261002/results.json`, SHA256
  `fcea88c40944a6594e60d3c97bb1eb41ca30c146703eb7df0e9d7510ed7c180d`.
- Repeat `topic-cqadup-semantic-labels-second-20261002/results.json`, SHA256
  `56942c435e5833fa0e3261d0195736765ca97db58968923c65fc256273984608`.
- Both vector files SHA256
  `efc0a99453f526dd556bcd0e785e7bab78002104293b13b16325b3da4885a7d7`.
- Audit `topic-cqadup-semantic-labels-audit-first-20261002.json`.
- Monitors `topic-cqadup-semantic-labels-memory-first-20261002.json` and
  `topic-cqadup-semantic-labels-memory-second-20261002.json`.

Do not promote this MMR setting on objective improvement alone. Candidate
evidence quality and intrinsic topic structure need further controls; independent
specificity/intrusion, actual agent navigation, lifecycle, ingestion and Rust
parity remain open. No topic method is selected.
