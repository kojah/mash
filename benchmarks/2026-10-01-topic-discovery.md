# Online thread topics: exploratory Rust prototype

## Decision criteria

These criteria precede selecting a production configuration. This first experiment
compares whole-thread means with message-level discovery and unions of message
assignments. It does not ship topic metadata, filtering, or MCP navigation.

Before adoption, review at least 50 representative threads, including mixed-topic,
short, long, held-out, copied, and outlier conversations across providers. Target
at least 80% relevant direct assignments and useful labels on that reviewed set;
report recall separately rather than increasing coverage with irrelevant tags.
For threads judged to contain multiple topics, require at least 80% recovery of
reviewed major topics. These are proposed acceptance targets, not measured scores.

Require at least 0.8 pairwise co-membership Jaccard across insertion orders on the
same corpus, comparing thread relationships rather than unrelated topic IDs.
Inspect unstable topics even if the aggregate passes. Reject a configuration
that assigns tens of incidental topics to ordinary conversations or mostly labels
provider boilerplate. Semantic parents must pass a separate reviewed coherence
check; CF-tree branches do not qualify as semantic ancestors.

Measure incremental and full-build wall time, peak memory, and indexing throughput
impact before integration. Target less than 10% throughput loss and no topic work
in search's critical path. MCP navigation and topic filters must meet the existing
search latency budget under provider/node/time restrictions and reveal no
inaccessible labels or counts. Persistence, accepted revisions, restart, edits,
deletes, stable namespaces, and ancestor recomputation remain mandatory gates.

## Method

The private, test-only Rust core implements normalized-coordinate BIRCH-style
count/linear-sum/squared-sum compression, bounded leaf growth, cached normalized
centroids, absolute and relative cosine thresholds, and class-normalized TF-IDF
terms. There is no new model download, LLM call, Python product dependency, changing
PCA basis, or search/index runtime integration.

The diagnostic opens a frozen SQLite snapshot read-only and requires a single
embedding generation with 384 dimensions. It decodes existing signed-byte vectors,
deduplicates identical search projections within each thread, and deduplicates exact
vectors across threads during discovery. It compares radii 0.4 and 0.6, forward and
reverse thread orders, and two modes: normalized whole-thread vector sums, or all
message vectors with assignments unioned back to the thread. Assignment thresholds
are absolute cosine 0.55 and relative 0.9. Labels use binary word presence per
thread to reduce repeated-log/provider volume bias. This is a c-TF-IDF variant,
not phrase extraction or a language model's topic naming.

The frozen input contains accepted Claude Code/Codex histories; it is not evidence
of all-provider quality, complete current indexing, or global index readiness.
Private memberships, IDs, titles and labels stay outside Git and stdout. Aggregate
results below will distinguish training-set coverage from judged accuracy.

Reproduce with explicit private paths:

```sh
MASH_TEST_SEARCH_DB=/path/to/frozen.sqlite3 \
MASH_TEST_TOPIC_REPORT_DIR=/path/to/private/reports \
cargo test --release --lib topics::evaluation::benchmark_topic_discovery \
  -- --ignored --exact --nocapture
```

Algorithm references: [BIRCH documentation](https://scikit-learn.org/stable/modules/clustering.html#birch)
and [class-based TF-IDF](https://maartengr.github.io/BERTopic/getting_started/ctfidf/ctfidf.html).
The implementation retains original coordinates to establish a baseline before
evaluating a fixed, versioned projection.

## First results and recommendation

Xenia, optimized Rust build, frozen snapshot with 789 threads (515 Claude Code,
274 Codex), 29,453 stored message vectors. Whole-thread discovery retained 756
distinct normalized means after exact-vector deduplication. These are exploratory
single-run timings, not latency guarantees or a judged relevance benchmark.

| Mode | Radius | Order | Topics | Fit ms | Assign ms | Label ms | Covered threads | Overlap threads | p95 / max tags |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Thread mean | 0.4 | Forward | 296 | 70.4 | 62.6 | 448.1 | 789 | 271 | 4 / 17 |
| Thread mean | 0.4 | Reverse | 282 | 65.2 | 60.1 | 396.1 | 789 | 239 | 4 / 20 |
| Thread mean | 0.6 | Forward | 60 | 47.7 | 22.2 | 343.4 | 782 | 168 | 3 / 7 |
| Thread mean | 0.6 | Reverse | 78 | 54.1 | 27.7 | 347.3 | 786 | 384 | 3 / 7 |
| Raw message discovery | 0.4 / 0.6 | Both | Rejected at 8,192-leaf cap | — | — | — | — | — | — |

Pairwise co-membership Jaccard was **0.758** at radius 0.4 and **0.653** at
radius 0.6, below the proposed 0.8 gate. A pair matches when two threads share at
least one topic; the metric compares intersections/unions of these unordered
thread pairs across runs, so it does not assume run-local IDs are comparable.
Median direct tags was one in all whole-thread runs. Cross-provider clusters
numbered 14/22 at radius 0.4 and 16/12 at radius 0.6. That indicates some mixing,
not proof of provider fairness or semantic coherence.

A qualitative check of the eight largest clusters in each forward run found
general instructions, operational vocabulary and incidental words among the top
terms. Some distinct clusters had identical top-five labels. Whole-thread word
contributions mix unrelated segments and are inadequate for precise multi-topic
naming. This is an exploratory label inspection, not the required 50-thread review;
no precision, recall or held-out accuracy claim is warranted.

**Reject these configurations for production topic tags and MCP navigation.**
The fast whole-thread fit does not compensate for label quality and order
sensitivity. Raw-message discovery exceeds the configured growth bound even on
this corpus; raising the cap alone would increase assignment cost and fragmentation.
The diagnostic records each rejected run as a typed error and continues the
remaining comparisons, without silently truncating vectors or adding partial topics
to a user-facing index.

Next, compare bounded topical segments, terms drawn from matching segments,
evidence-weighted thread assignments, and a fixed projection against this baseline.
Check copied conversations and provider boilerplate explicitly. Track this in
`mash-c0d.19.2`. Persistence and revision handling, stable per-index namespaces,
semantic hierarchy and inherited tags, scoped MCP navigation and topic filters
remain in `mash-c0d.19`; none is implemented or implied by this prototype.

The tree bounds dimensions, branch fan-out, sample counter precision and leaf
count, and rejects capacity/invalid vectors before changing existing tree state.
Those bounds are not a total process memory limit: the diagnostic retains its
input vectors, words and memberships, and labeling allocates corpus-wide maps.
An explicit second run of the same optimized test binary, measured with
`/usr/bin/time -v`, peaked at **183,652 KiB (179.3 MiB) RSS** for the full eight-run
diagnostic, including corpus loading and private report writes. This does not
isolate CF-tree memory or predict production streaming memory.
Streaming contention, incremental latency, edit/delete/restart behavior, all-provider
quality, scoped topic-filter performance and human-reviewed accuracy remain unmeasured.

Validation: 575 tests passed with explicit installed models and disposable
PostgreSQL, Redis and Qdrant services, including persisted-backend restart checks;
11 manual benchmarks were excluded. All-target/all-feature Clippy, doctests,
formatting and whitespace checks passed. The initial broad validation had two
PostgreSQL startup failures because the local test URL omitted `sslmode=disable`
for its non-TLS service; correcting that test environment produced the complete
passing run. No runtime SQL, CLI, MCP or shared API behavior changed.
