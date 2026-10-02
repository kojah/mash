# Full-corpus lexical duplicate retrieval controls

Bead `mash-c0d.19.2.30.2`. A new retrieval signal tests whether source
representations retrieve community-judged duplicate problems, before further
topic discovery tuning. This is an offline research comparison, not a change
to mash search or evidence of semantic topics/ancestors.

## Prospective protocol

Use the pinned [benchmark preparation](2026-10-02-duplicate-retrieval-preparation.md),
all 32,176 original candidate questions and only 64 development queries.
Preserve title + newline + text, lowercase without accent stripping, cleaning,
stemming, stopwords or metadata features. Fit vocabulary on candidates only:
minimum DF 2, maximum DF 1.0 and cap 65,536. Corpus statistics are transductive;
reserved query text/outcomes never enter fitting or scoring. References are
used only for evaluation, without graph closure or sampled negatives.

Three fixed controls, no sweeps or result-based mixture:

- Word TF-IDF: default Unicode word tokens of length ≥2, unigrams.
- Character TF-IDF: `char_wb` fragments of length 3–5, with boundary padding.
- Word BM25: same word vocabulary, `k1=1.2`, `b=.75`, binary query terms.

[TF-IDF](https://scikit-learn.org/stable/modules/generated/sklearn.feature_extraction.text.TfidfVectorizer.html)
uses sublinear `1+log(tf)`, smooth `log((N+1)/(DF+1))+1` IDF and L2 cosine.
[BM25](https://www.staff.city.ac.uk/~sbrp622/papers/foundations_bm25_review.pdf)
uses term saturation and length normalization; our declared variant sums
`log(1+(N-DF+.5)/(DF+.5)) * tf*(k1+1)/(tf+k1*(1-b+b*dl/avgdl))` over distinct
query terms. The positive IDF is also used in
[Lucene](https://github.com/apache/lucene/blob/main/lucene/core/src/java/org/apache/lucene/search/similarities/BM25Similarity.java).
This is not an exact Lucene engine comparison: we use float64, exact lengths,
binary query terms, a filtered vocabulary and no field-specific statistics.
Lengths count complete analyzer output before vocabulary filtering.

Rank the entire corpus by descending score then lexicographic original ID,
including zero-score ties. Record zero-query evidence explicitly. Save complete
64×32,176 score matrices, top 100 identities/scores and exact ranks of every
explicit judged duplicate. Report macro MRR@10, nDCG@10 and recall@10/@100.
Unjudged candidates are not proven irrelevant. All queries have positive
references, so this does not evaluate no-match abstention.

## Results

| Method | MRR@10 | nDCG@10 | Recall@10 | Recall@100 | Queries with a hit @10 / @100 |
| --- | --- | --- | --- | --- | --- |
| Word TF-IDF | .2232 | .2309 | .3263 | .4877 | 24 / 36 |
| Character TF-IDF | .2132 | .2224 | .3000 | .5283 | 22 / 39 |
| Word BM25 | .2621 | .2525 | .3268 | .5025 | 25 / 37 |

Word vocabulary has 28,213 terms and 2,993,065 nonzeros; characters reach the
65,536 cap with 34,693,072 nonzeros. All 64 queries have feature evidence.
Character top-100 hits include nine word-TF-IDF misses but lose six word hits:
the candidate provides complementary evidence, not a uniform improvement.
BM25 improves first judged rank for 23 queries, ties 14 and worsens 27 versus
word TF-IDF; character scores improve 24, tie 12 and worsen 28.

Post-hoc descriptive paired-query bootstrap (5,000 resamples, NumPy seed 17,
percentile 95% intervals) gives nDCG@10 differences BM25−word `.0217`
`[-.0137,.0589]`, character−word `-.0084` `[-.0523,.0391]`. Both include zero;
these conditional development-sample intervals are not a selection rule,
confirmatory hypothesis test, annotation-error estimate or generalization proof.
No production winner is selected from the highest observed mean.

## Source-grounded failure inspection

All three controls fail to place any judged duplicate in the top 100 for 16
queries. Inspect the first three such query IDs in lexical order, keeping full
declared query text and judged source bodies (not a best-looking selection):

- `118140`: a question about getting a coworker's benefit when working alone
  links to `194922`, about rubber-duck debugging. Ranks are 18,099 word TF-IDF,
  15,097 character and 16,279 BM25. Lexical coworkers/workers dominate top hits.
- `144717`: single-page application tradeoffs link to `231062`, describing
  AJAX-loaded layout/body/modals without the same terminology. Ranks are
  598 / 2,307 / 691. BM25 top hits include generic advantages/disadvantages
  questions on unrelated implementations.
- `153581`: a RESTful-definition query links to `23891`, explaining REST.
  Ranks are 1,888 / 2,289 / 1,586. Other RESTful questions rank above the judged
  one; these unjudged hits are not necessarily irrelevant.

These examples suggest vocabulary mismatch, generic query terms and whole-source
mixing as candidates for controlled follow-ups, not proven causal explanations.
Do not add query-specific synonym rules or use judgments to rewrite features.
Character spelling overlap alone does not solve these problems. A bounded
local-encoder comparison and matched title/body representation controls are
worth testing next. Topic-derived navigation must beat direct retrieval under
equal budgets before being claimed useful.

## Costs and reproducibility

First-process fit times are 3.596 seconds word TF-IDF, 46.612 seconds characters
and 3.383 seconds BM25. Scoring plus full ranking and metric construction p50/p95
is 31.98/50.25 ms, 114.26/140.99 ms and 26.48/32.60 ms respectively.
The full first run takes 69.934 seconds after imports, including source integrity
checks and output writing. These Python brute-force research timings include
evaluation work and are **not production search latency or ingestion throughput**.
They do not measure incremental vocabulary maintenance, edits/deletes, persistence
or the ≤10% ingestion-loss gate. No inference/model download/paid API is used.

Use the pinned graph Python environment with one OpenMP/OpenBLAS/MKL thread.
`lexical_retrieval.py` contains reference-independent scoring/evaluation primitives;
`cqadup_retrieval.py` enforces immutable source/cohort pins and development scope.
Run `--root SOURCE_ROOT --output-directory FRESH_OUTSIDE_GIT_DIRECTORY`; repeat
with `--compare FIRST_DIRECTORY/results.json`. Raw sources and score matrices
remain outside Git. Five synthetic tests cover formulas, pre-filter lengths,
identifier distinctions, OOV evidence, ties, metrics and malformed inputs;
all 336 research tests pass.

Fresh processes match every nontiming field exactly, including vocabulary,
IDF/length pins, all scores, judged ranks and metrics. Repeat wall time is
128.683 seconds while running concurrently with the scalar audit; it is not
a controlled timing comparison. Environment: Linux x86-64, AMD EPYC 9645,
Python 3.11.15, NumPy 2.4.6, SciPy 1.17.1 and scikit-learn 1.9.1.

The outside-Git audit reconstructs word tokens with an independent regex and
padded character fragments with explicit loops, then counts, lengths, DF, IDF,
query weights and complete scalar posting-list scores. All 6,177,792 scores
agree within `1e-12` (maximum errors word `2.22e-16`, character `7.22e-16`,
BM25 `7.11e-15`). It independently sorts archived exact scores by the declared
tie rule and checks top100, every judged rank and macro/per-query metrics.
It does not claim that alternative floating-point score accumulation preserves
every close tie across platforms. It shares the already-audited source loader,
and validates the admitted vocabulary rather than independently reproducing
the library's frequency-cap tie choices. Separate checks prove complete
development query coverage and zero reserved scoring in both captures.
This is mathematical Python verification, not Rust parity or semantic gold.

First result SHA256:
`22ce86f9ec04daa7a8c41bc174b399faad93c1b2cb91066c5c5735a156658c0d`.
Artifacts: `topic-cqadup-retrieval-first-20261002` under mash-artifacts.
Repeat result SHA256:
`286353ab1f1dd4df1bca9f8b3515dc82fea095e573e326f46ec48c9a650228c3`.
Repeat artifacts, scalar audit script and post-hoc bootstrap manifest also
remain outside Git. No private conversation data is used.
Reserved outcomes remain unopened; this single forum is not an agent-dialogue,
multi-topic label, semantic-tree or guided-agent evaluation.
