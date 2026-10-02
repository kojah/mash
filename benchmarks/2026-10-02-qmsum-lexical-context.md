# QMSum causal lexical evidence diagnostic

This is an exploratory source-representation comparison, not a selected topic
algorithm, coding-session quality evaluation, or production search benchmark.
No LLM, encoder, inference call, download, validation split or test split is used.

## Frozen procedure

Use the complete audited QMSum training capture at author commit
`83d7768c1f2b4dfeb091385d3dc7e239b8e5bb7e`: 162 meetings, 88,336 original turns.
Original source SHA256 is
`55964a968e4b4c1c9da2434678f213f3b70c0c3d3cf609f5cd4ecd44cc8d2427`.
The required preflight SHA256 is
`3a79fe865d7714ffb913e20123520a9fad6aad5244ae1b502146dabbc0b3371b`.

Features are lowercase Unicode unigrams with regex `(?u)\b\w\w+\b`, no
stoplist, and a fixed maximum of 20,000 vocabulary entries. Fit source-only
CountVectorizer and smooth-IDF/sublinear-TF/L2 TfidfTransformer on all original
training turns. The observed vocabulary is 18,316 entries: the cap is inactive.
This fitting makes evaluation in-sample. Annotation names, query/answer text,
speakers, topic IDs and turn positions never enter feature construction.

Compare original-turn candidate vectors with raw-count aggregation of the
current turn and up to four preceding whole turns within the same meeting,
before applying the same TF-IDF transformation. Context has a 1,024-regex-token
budget including repetitions; stop at the first whole preceding turn that would
exceed it. Never skip/crop a turn. Keep an over-budget current turn intact and
borrow no context. Both policies use the same original-turn query vector.

Eligible queries have one annotated topic, at least five regex tokens and
nonempty vectors under both policies. Candidates are nonempty singleton-annotated
turns in the same meeting. Exclude all candidates within four original turns
of the query, ensuring candidate context does not contain the query source.
Unknown and multi-membership turns are not negative qrels. Candidate relevance
uses the current candidate's local source topic, not its complete context.
Retain zero scores; rank descending cosine then ascending original ordinal.
Keep unsupported queries explicitly and distinguish Hit@k from Recall@k.

## Results

41,843 queries are eligible; 82 have no nonoverlapping known same-topic peer.
All 41,761 supported queries have both relevant and nonrelevant peers, so the
supported and informative reporting scopes coincide. Mean relevant-candidate
prevalence (random Hit@1 expectation) is 0.306874.

| Metric | Original turn | Causal context |
|---|---:|---:|
| MRR | 0.644210 | 0.657425 |
| Hit@1 | 0.500587 | 0.574124 |
| Hit@10 | 0.920883 | 0.854314 |
| Recall@1 | 0.004745 | 0.005941 |
| Recall@10 | 0.033101 | 0.046005 |

The 1,000 paired meeting resamples (seed 20261002) give descriptive MRR-gain
2.5/50/97.5 percentiles 0.009566/0.013297/0.016951. They do not account for source
family correlation, pretraining or experiment-selection uncertainty. They are
not independent held-out confidence in coding-session quality.

Of 81,456 singleton anchors, 78,731 constructed windows have only the same
singleton annotation; 2,725 include different or unknown membership. These are
post-construction diagnostics, never annotation-guided window selection. Context
limits: 800 meeting roots, 87,480 five-turn limits, 53 whole-turn token limits,
and three over-budget-current fallbacks. There are 287 empty source vectors.

Context improves first-result accuracy but reduces the fraction of queries with
any relevant top-ten result. Higher Recall@10 alongside lower Hit@10 can occur
when more relevant items concentrate in successful queries. Aggregate metrics do
not establish that mechanism causally. Do not replace original-turn evidence
with context alone. The next fixed comparison should retain both views with
source-identity deduplication, and measure whether it preserves broad coverage.
It must precede conclusions about clustering, labels or semantic ancestors.

## Reproduction and audit

Drivers: `research/topics/qmsum_lexical_context.py` and
`research/topics/causal_lexical_context.py`. The older `lexical_context.py` and
its tests remain byte-identical to HEAD before this task; the new helper has a
distinct name. An earlier run before correcting the module naming collision is
retained as superseded provenance, not silently overwritten. Policy is unchanged.

Final runs live outside Git under
`/home/james/scratch/mash-artifacts/topic-qmsum-lexical-context-final-{first,second}-20261002`.
The second run uses `--compare FIRST/results.json`; every nontiming result and
all saved CSR arrays/vocabulary/IDF hashes match exactly. Dependencies are pinned
in `research/topics/uv.lock`; Python 3.11.15, scikit-learn 1.9.1. Full enabled
research suite: 438 tests pass, including the five new causal-context edge cases.

The independent auditor is
`/home/james/scratch/mash-artifacts/topic-qmsum-lexical-context-audit-20261002.py`.
It reads original JSONL without importing experiment helpers and reconstructs
complete counts, source windows, TF-IDF vectors, annotation membership, query
support, complete candidate rankings, metrics and paired resamples. Terminal
success verifies all 162 meetings/88,336 turns and 41,843 query rows; maximum
vector error is 2.78e-16 and saved score error is zero. It shares SciPy's
sparse dot kernel; it does not establish independent-kernel, inference or Rust
parity. Frequency-cap membership is checked independently; exact tied cap
selection is only pinned (the cap is inactive on this source).

Final first results SHA256:
`e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488`.
Auditor SHA256:
`f069b0b4390b837d3cd9ece31c841d676d91089dac01f7a5f554467d38273925`.
Terminal audit artifact: `topic-qmsum-lexical-context-audit-first-20261002.json`.
Beads: completed bounded experiment `mash-c0d.19.2.20.6`; next dual-view comparison
`mash-c0d.19.2.20.7`. Broader topic selection remains incomplete.

Run with `memory_audit.py --output MEMORY.json -- PYTHON
research/topics/qmsum_lexical_context.py --source SOURCE --preflight PREFLIGHT
--output-directory FRESH_DIRECTORY [--compare FIRST/results.json]`.
Use the existing `topic-graph-python-env/bin/python`, no model assets needed.

## Cost scope

After-import wall first/repeat is 15.389/15.912 s; CPU 15.265/15.319 s.
Source loading/preflight is 1.627/1.832 s, fit/count aggregation 0.820/1.118 s,
matrix writes 0.111/0.149 s, evaluation 12.831/12.813 s. Evaluation dominates.
The timer excludes repeat comparison and final results-JSON writing. The repeat
overlapped the independent auditor: these are observed scoped costs, not a clean
speed comparison or complete ingestion budget.

Rusage peak is 598864/590788 KiB. External target-only RSS maxima are
599568/594104 KiB; PSS 591726/586056 KiB; status high water 599568/593404 KiB.
Sampling has 664/701 observations, one/zero vanished reads, median/max gaps
28.33/51.99 and 28.47/52.76 ms. External scope includes imports, repeat comparison
and final JSON output, excludes observer and auditor memory. Sampled peaks are
lower bounds, status counters approximate, monitoring intrusive, and rusage may
retain launcher high water. Do not subtract counters or attribute this offline
evaluation workspace to production topic memory. No full-ingestion gate passes.
