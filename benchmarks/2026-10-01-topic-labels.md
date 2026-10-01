# Phrase and semantic topic labels

Continuation of `mash-c0d.19.2`, using the
[thread-discovery/segment-assignment candidate](2026-10-01-topic-grouping.md).
This is a private, test-only Rust experiment. The production index, CLI and MCP
surface remain unchanged, and the topic-adoption gates remain open.

## Method and bounds

Extract unigram, bigram and trigram candidates from each contributing user-message
prefix: at most 4,096 Unicode characters and 256 raw tokens. Punctuation, stop words
and discarded metadata tokens break adjacency, so phrases are not synthesized across
sentences or omissions. Technical short names such as `go`, `js`, `ai` and `ui`
are permitted. Prefix bounds affect label vocabulary, not canonical text or embeddings.

Keep per-topic c-TF-IDF candidates from matching segments, with one word/phrase
contribution per thread. Shortlist at most 64 candidates per topic. Explicitly load
the installed local embedding model through the existing application `Models`/
`EmbeddingPool` core and require its generation to match the frozen index. This
experiment neither downloads a model nor calls an LLM or remote endpoint.

Rank shortlisted phrase embeddings against the normalized topic centroid with
`0.85 * cosine + 0.15 * relative_c_tfidf`, retaining cosine at least 0.25. Return
at most eight labels and suppress phrase containment or embedding-near duplicates
(candidate cosine at least 0.92). Scores are ranking values, not probabilities.
Names are extracted from source text rather than generated.

Reuse a generation-scoped candidate-vector cache across diagnostic runs, capped
at 16,384 entries and embedding batches of 64. Exceeding the cap is an explicit
error. The first labeling pass includes candidate embedding; subsequent passes
embed only missing candidates. Report timing excludes initial model loading, which
occurs once per diagnostic process.

Exact short acknowledgments/greetings and delivery markers are omitted from topic
discovery/assignment. Larger requests containing greetings or thanks are retained;
even the literal `hello world` remains eligible. The frozen corpus excludes 106
distinct user-message contributions through this rule. Canonical history, ordinary
search and source databases are unchanged; this is not provider-record deletion.
Filtering changes thread means and the copy-aware held-out partition.

## Results

Xenia, optimized Rust build, frozen 789-thread Claude Code/Codex snapshot, one
compatible 384-dimensional local-model generation. Radius 0.4 remains the preferred
candidate for further evaluation.

| Run | Active topics | Newly embedded candidates | Total cached candidates | Semantic-label ms |
| --- | ---: | ---: | ---: | ---: |
| Radius 0.4, forward | 125 | 4,534 | 4,534 | 1,557 |
| Radius 0.4, reverse | 125 | 1,079 | 5,613 | 415 |
| Radius 0.6, forward | 53 | 80 | 5,693 | 28 |
| Radius 0.6, reverse | 47 | 58 | 5,751 | 31 |

Every active topic (at least one assigned thread) received a semantic label.
Radius-0.4 discovery has 157/156 centroids, including inactive discovery seeds;
those empty seeds are not evidence of unlabeled active topics. Coverage is 764
threads in both orders, p95 three direct tags, maximum 15/22, and pairwise
co-membership Jaccard 0.874. These are training-corpus grouping measures, not
independently judged relevance or label-accuracy scores.

Largest-cluster inspection shows clearer descriptors. For example, c-TF-IDF alone
favored generic analysis/candidate vocabulary for technical debugging groups;
semantic ranking favors phrases such as “real deadlocks,” “triage tools” and
“test package.” A Git-policy group favors “git completion” and “current branch.”
Workflow groups favor “autonomous workflow” over generic artifact vocabulary.
This is qualitative inspection, not the required representative 50-thread review.
Some distinct clusters still share descriptors, and rare named entities can be
disadvantaged by the embedding model or shortlist. These remain review targets.

## Held-out and resource check

The updated copy-aware split reserves 170 threads. At radius 0.4, both orders
assign 150/170 held-out threads, with overall coverage 752/789 and pairwise
Jaccard 0.914. P95 is four tags and maximum 11/15. All 110/109 active topics
have semantic labels. Labels include assigned held-out text, so this measures
centroid assignment coverage, not independently held-out label accuracy.

The four-run held-out diagnostic takes 6.14 seconds, peaks at 579,248 KiB
(565.7 MiB) RSS, and uses 172% CPU under `/usr/bin/time -v`. This includes
corpus retention, local embedding sessions, vocabulary/candidate maps, cached
vectors and report writes. It does not isolate label-vector memory or predict
production streaming throughput or search-worker RSS. Topic names should be
materialized during derived-index work, not inferred on each search/navigation call.

## Remaining work and reproduction

The naming approach is promising enough for the next review stage. Do not infer
production readiness from these measurements. Review representative threads,
including overlaps, outliers, proper names and provider/template bias. Then verify
accepted revisions, edits/deletes/restarts, model-versioned stable topic IDs,
meaningful semantic ancestry and inherited memberships, scope-aware labels/counts,
and MCP navigation/filters. No topic tool or production tag is exposed yet.

```sh
MASH_TEST_SEARCH_DB=/path/to/frozen.sqlite3 \
MASH_TEST_TOPIC_REPORT_DIR=/path/to/private/reports \
MASH_TEST_MODELS_DIR=/path/to/explicitly/installed/models \
MASH_TEST_TOPIC_ANCHORED=1 \
MASH_TEST_TOPIC_PHRASES=1 \
MASH_TEST_TOPIC_SEMANTIC_LABELS=1 \
cargo test --release --lib topics::evaluation::benchmark_topic_discovery \
  -- --ignored --exact --nocapture
```

Add `MASH_TEST_TOPIC_HOLDOUT=1` and use a separate private output directory for
held-out evaluation. Private titles, memberships, candidate labels and logs stay
outside Git in `mash-artifacts/topic-labels-20261001`.

Validation: 490 regular tests passed, plus explicit optimized corpus/held-out
diagnostics using real installed models. Tests cover phrase boundaries/work bounds,
canonical-input preservation, exact acknowledgment exclusion, semantic diversity,
ranking and invalid/missing embeddings. All-target/all-feature Clippy, the existing
doctest, formatting and whitespace checks passed. The regular suite skips external
services/manual diagnostics; no shared-backend integration behavior changed.
