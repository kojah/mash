# Representative discovery for mixed-topic threads

Continuation of `mash-c0d.19.2`, after
[notification filtering](2026-10-01-topic-input-quality.md). Rust test-only
prototype; production indexing, search and MCP are unchanged.

## Method

Whole-thread means can fall between distinct subjects and match none of the
original segments. A regression demonstrates this with four orthogonal subjects:
whole-mean discovery yields no assignments, while representative discovery
retains all four subjects without requiring other single-subject threads.

Select at most four normalized user segments per thread. Prefer the largest
support first, then maximize `message_count * (1 - nearest_selected_cosine)`.
Require cosine below 0.65 against every selected representative to add another.
Break equal-score ties by canonical vector order. Validate every vector, settings
and segment support; no all-pairs distance matrix is allocated. Selection costs
`O(segments * 4 * dimensions)` with linear normalized-vector storage. Discovery
deduplicates exact representative vectors across threads as before.

Assignment still evaluates all substantive user segments, with the unchanged
absolute/relative cosine and relative-support thresholds. Four discovery seeds
do not cap a thread at four resulting tags. This is batch representative selection,
not a persisted online update/retraction algorithm. No semantic parent hierarchy
or stable cross-revision topic identity is inferred from the CF tree.

## Frozen-corpus comparison

Same 789-thread Claude Code/Codex snapshot and explicitly installed compatible
384-dimensional local model. Optimized build, both insertion orders and radii
0.4/0.6. All eight runs pass without exhausting the 8,192-leaf or 16,384-candidate
cache bounds. No model downloads, remote inference or source-database writes.

| Radius-0.4 mode / order | Unique discovery samples | Centroids | Active labeled topics | Assigned threads | P95 tags | Max tags | Fit ms | Assignment ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Representatives / forward | 668 | 398 | 394 | 781 | 5 | 46 | 196 | 899 |
| Representatives / reverse | 668 | 383 | 379 | 781 | 5 | 39 | 178 | 639 |
| Whole mean / forward | 448 | 159 | 126 | 765 | 3 | 21 | 105 | 380 |
| Whole mean / reverse | 448 | 153 | 118 | 762 | 3 | 17 | 108 | 418 |

Representatives improve coverage and retain distinct subjects, at a cost of more
tags and roughly 1.5–2.4 times assignment work in this single-run comparison.
Timings cover assignment across the complete corpus, not a search request.
All active topics receive semantic labels. First-pass naming embeds 8,487
candidates in 2,978 ms; reverse order adds 1,030 candidates in 395 ms. The complete
eight-run diagnostic takes 11.31 seconds after compilation. Whole-mean naming
runs later and benefits from the same candidate cache, so its naming time is not
an independent cold-model comparison.

Across all thread instances, radius-0.4 co-membership Jaccard rises from 0.884
to 0.937. Radius-0.6 representatives reach 0.780, below the proposed 0.8 gate.

## Held-out and copied-history checks

The same copy-aware 169-thread partition is used for both modes. Radius-0.4
representatives assign 156/169 held-out threads in both insertion orders, versus
150/149 for whole means. Overall coverage is 776/789 in both orders, p95 six
tags, maximum 36/37, and all 343/345 active topics are labeled. Co-membership
Jaccard across all instances is 0.918, versus 0.876 for whole means. Labels include
assigned held-out text: coverage is not held-out label accuracy.

The eight-run held-out process takes 10.66 seconds, peaks at 537,412 KiB
(524.8 MiB) RSS and uses 164% CPU. This includes the corpus, model sessions,
normalized vectors, vocabulary, cache and detailed private reports. It does not
isolate streaming/index throughput or predict search-worker memory.

Repeated histories can disproportionately weight pairwise stability. A separate
read-only artifact analysis keeps the lexicographically first thread per exact
ordered raw user-vector sequence, after within-thread exact-text deduplication but
**before topic-only exclusions**. It retains 451 representatives. This differs
from the normalized, filtered sequence hash used for training/held-out partitioning
and can retain near copies or scalar-equivalent vectors; it is a stricter diagnostic,
not a claim of independent conversations or semantic deduplication.

| Radius-0.4 mode | Full-corpus unique-sequence Jaccard | Held-out-run unique-sequence Jaccard |
| --- | ---: | ---: |
| Representatives | 0.810 | 0.788 |
| Whole mean | 0.751 | 0.678 |

The held-out-run column compares memberships for the complete corpus under
held-out discovery, not just the reserved threads. Representatives still improve
stability, but miss the proposed 0.8 target in this stricter held-out-run check.
Do not use the copy-weighted headline score alone to justify adoption. This
artifact analysis uses Python; runtime topic discovery and labeling remain Rust.

## Relevance findings and next work

Targeted reinspection of previously missed cases finds useful tracing and
pull-request-outreach labels. The long mash discussion now receives subjects for
MCP/core sharing, filtered HNSW search and Rust enums. However, it also receives an
attachment-format topic. Short acknowledgments and context-dependent follow-ups
produce low-information topics, and project instructions still produce incidental
policy tags. These are coding-agent qualitative observations, not a completed
50-thread review or a measured relevance percentage.

Keep representatives as the more promising experimental candidate, while addressing
attachment/instruction framing, short follow-up evidence, copied-history evaluation
and excessive/incidental tags. Public adoption still requires representative
relevance/overlap review, stable namespaces and revision lifecycle, meaningful
semantic ancestors with inherited memberships, scope-safe labels/counts, bounded
MCP navigation and composed topic filters. No topic MCP tool is exposed yet.

## Verification and reproduction

495 regular tests pass, including selector bounds, canonical ties, invalid inputs,
duplicate suppression and the mixed-thread discovery regression. Optimized real-model
full and held-out diagnostics pass. All-target/all-feature Clippy, formatting,
diff checks and doctests pass. External-service tests are skipped by the regular
suite; no production backend path changes.

Run the existing ignored `topics::evaluation::benchmark_topic_discovery` test with
`MASH_TEST_TOPIC_REPRESENTATIVES=1`, `MASH_TEST_TOPIC_PHRASES=1`,
`MASH_TEST_TOPIC_SEMANTIC_LABELS=1`, explicit `MASH_TEST_SEARCH_DB`,
`MASH_TEST_MODELS_DIR` and `MASH_TEST_TOPIC_REPORT_DIR`. Add
`MASH_TEST_TOPIC_HOLDOUT=1` for the held-out comparison. Source excerpts,
identifiers, labels and detailed artifacts remain outside Git in
`mash-artifacts/topic-representatives-20261001`.
