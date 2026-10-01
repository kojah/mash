# Derived topic input projection

Continuation of `mash-c0d.19.2`, using
[bounded representatives](2026-10-01-topic-representatives.md). This is an opt-in
test-only Rust experiment. Production indexing, search, CLI and MCP remain unchanged.

## Projection rules and correctness

Recognize supplied `# AGENTS.md instructions for ...` / `<INSTRUCTIONS>` frames,
rendered file-list / `My request` envelopes and empty rendered image blocks.
Omit standalone metadata, retaining any actual request outside the frame. Preserve
quoted, incomplete, unknown or nonempty image formats rather than guessing.

Decode complete `send_user_message_question_reply` envelopes with a typed,
strict Rust `QuestionReply` model. Keep question/answer text, exclude transport
IDs, and preserve the original format if parsing or validation fails. Accept at
most eight replies in a 64-KiB body. Omit complete recognized aborted-turn notices
from topic evidence. Canonical history and ordinary search are unchanged.

Changed text receives a **fresh vector** from the explicit installed model pool,
whose generation must match the frozen index. Never pair projected words with the
old search vector. Reuse the existing pool, deduplicate projected inputs and embed
in batches of at most 64, with at most 1,024 unique projected inputs. Normalize and
validate every returned vector before replacing derived thread evidence. Model
absence/failure cannot silently fall back to stale vectors. Preparation, embedding
and application are separate stages; failed preparation or model work leaves
the supplied derived evidence unchanged.

Keep source text aligned with individual user-message evidence during this private
experiment. Deduplicate equivalent cleaned sources within a thread, including
rendered-with-images versus rendered-without-images copies. Preserve canonical
vectors and source databases. No model downloads or remote calls occur.

The frozen corpus omits 60 additional metadata/event contributions, projects 27
messages using 26 unique fresh embeddings, and deduplicates 15 equivalent inputs.
Projection takes 154 ms after the already-required model pool is loaded. Counts
follow existing within-thread source deduplication; they do not imply deletion of
native records or production search data.

## Optimized full-corpus results

Same 789-thread Claude Code/Codex snapshot and local 384-dimensional model.
All eight radius/order/mode runs pass without exhausting prototype leaf or cache
bounds. Counts use all corpus threads, including threads without eligible user-topic
evidence; coverage is not a relevance score.

| Radius-0.4 mode / order | Centroids | Active labeled topics | Assigned threads | P95 tags | Maximum tags | Assignment ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Representatives / forward | 405 | 398 | 778 | 5 | 44 | 783 |
| Representatives / reverse | 391 | 384 | 778 | 5 | 40 | 759 |
| Whole mean / forward | 149 | 117 | 750 | 3 | 17 | 448 |
| Whole mean / reverse | 150 | 118 | 751 | 3 | 17 | 413 |

Representatives retain broader coverage than whole means. First-pass topic naming
embeds 8,054 candidates in 1,892 ms; reverse order adds 1,046 in 296 ms. All active
topics have names. The eight-run process takes 11.50 seconds after compilation.
These timings concern the complete diagnostic corpus, not a search request; later
configurations reuse the phrase cache. No isolated indexing-throughput claim follows.

| Radius-0.4 mode | All-instance Jaccard | Unique-source-sequence Jaccard |
| --- | ---: | ---: |
| Representatives | 0.945 | 0.817 |
| Whole mean | 0.948 | 0.808 |

The stricter column uses the same fixed 451 representative threads as the preceding
comparison: exact ordered raw user-vector sequences, after within-thread exact-text
deduplication and **before** topic filtering/projection, retaining the lexicographically
first thread. It is a read-only artifact analysis, not complete semantic deduplication
or independent-conversation verification. Repeated histories inflate the headline
scores. Radius-0.6 all-instance stability remains poor at 0.571/0.575.

## Held-out and resources

Projection changes the normalized user-sequence hash and therefore the copy-aware
partition: 174 threads are now reserved, rather than 169 before projection. These
are different splits; do not infer improved or worse held-out recall from the raw
before/after covered counts. The denominator can include threads without eligible
user-topic evidence and needs a separate eligibility metric in subsequent reviews.

Radius-0.4 representatives assign 154/174 reserved threads in both insertion orders,
with 769/789 overall assigned, p95 five tags and maximum 36/34. All 334/338 active
topics receive labels. Whole means assign 145/174 in both orders, overall 736/739.
Labels still incorporate assigned held-out text; this is assignment coverage, not
independent held-out label accuracy.

All-instance co-membership Jaccard is 0.936 for representatives and 0.895 for whole
means. Under the same fixed 451-thread collapse, it is **0.793 / 0.725**. These
compare full-corpus memberships under held-out discovery, not solely the reserved
threads. Representative stability still misses the proposed 0.8 adoption target.

The eight-run held-out process takes 11.25 seconds and peaks at 568,664 KiB
(555.3 MiB) RSS, using 165% CPU. This includes source strings, corpus vectors,
temporary projection plans, installed model sessions, cache and private report
writes. It is not a production-worker memory bound or streaming-throughput result.

## Qualitative findings and next work

Targeted coding-agent reinspection confirms that a named-project thread loses its
incidental Git-policy topic. The mash thread loses attachment-path and clarification
transport topics while retaining MCP/core, filtered HNSW, PostgreSQL/HTTP, enums
and memory-usage subjects. Generic short follow-up topics remain. These are specific
observations, not a completed 50-thread review or measured topic precision.

Continue with short/context-dependent evidence, skill/hook/template framing,
eligible-thread denominators and representative relevance/overlap review. The
prototype still lacks stable model/projection-versioned IDs, revision persistence,
edits/deletes/restart handling, semantic ancestry and inherited tags, scoped
labels/counts, and MCP topic navigation/filters. Do not promote it from these
measurements alone.

## Validation and reproduction

499 regular tests pass, including projection boundaries, typed clarification
validation, canonical-vector retention, omission/deduplication and explicit rejection
of stale-vector fallback. Optimized real-model full/held-out diagnostics, all-target/
feature Clippy, formatting, diff checks and doctests pass. Regular validation skips
external-service tests; no production backend path changes.

Use the ignored `topics::evaluation::benchmark_topic_discovery` test with explicit
`MASH_TEST_SEARCH_DB`, `MASH_TEST_MODELS_DIR`, `MASH_TEST_TOPIC_REPORT_DIR`, and
`MASH_TEST_TOPIC_REPRESENTATIVES=1`, `MASH_TEST_TOPIC_PHRASES=1`,
`MASH_TEST_TOPIC_SEMANTIC_LABELS=1`, **`MASH_TEST_TOPIC_CLEAN_INPUT=1`**. Add
`MASH_TEST_TOPIC_HOLDOUT=1` for the held-out run. Private artifacts remain in
`mash-artifacts/topic-input-projection-20261001`; source histories, IDs and detailed
labels stay outside Git. Python is used only for aggregate artifact analysis,
not runtime topic discovery or projection.
