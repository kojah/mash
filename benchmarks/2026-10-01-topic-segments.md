# User-segment topic discovery

Follow-up to [the initial comparison](2026-10-01-topic-discovery.md), tracked in
`mash-c0d.19.2`. The original adoption criteria still apply. This remains a private,
test-only Rust prototype; no production CLI/MCP response or indexing behavior changes.

## Changes

The frozen snapshot contains 6,714 embedded user messages and 22,739 assistant
messages. This experiment compares normalized user-only thread means with
contiguous user-message segments. A streaming segment holds at most eight message
vectors and splits when the next vector's cosine against its current normalized
sum is below 0.75. Every accepted distinct user message contributes; there is no
arbitrary per-thread truncation. Deduplication keys include role so an assistant
copy cannot suppress a user's contribution. Threads without embedded user input
remain unassigned rather than silently falling back to assistant instructions.

Segment assignments require absolute cosine 0.65 and at least 0.92 of the best
centroid's cosine. Per-thread support sums cosine times contributing message count;
topics below 10% of the thread's strongest support are omitted. This is an
experimental relevance rule, not a probability or guaranteed minor-topic recall.
User means retain the earlier 0.55/0.9 thresholds as a simpler baseline.

Labels now receive words from the segments supporting that topic, with at most
one contribution per word per thread. A frontend segment cannot introduce frontend
terms into a separately assigned database topic. Label candidates exclude
digit-leading timestamp fragments and long hex digests while preserving technical
names such as `pgvector`, `gpt4`, `bm25` and `x86`. Original text and embeddings
are retained. This filtering does not remove arbitrary task prompts or infer which
instructions are important to the user.

An optional deterministic held-out split hashes the complete ordered user-vector
sequence. Exact copied sequences remain in the same partition; held-out sequences
never train the CF tree. Approximately one fifth of unique sequences is reserved,
which need not be one fifth of threads on this duplicate-heavy corpus. Near copies
and shared templates can still cross the partition. Labels include assigned
held-out input, so this tests centroid assignment coverage, not independently
held-out label quality or semantic accuracy.

## Reproduction

```sh
MASH_TEST_SEARCH_DB=/path/to/frozen.sqlite3 \
MASH_TEST_TOPIC_REPORT_DIR=/path/to/private/reports \
cargo test --release --lib topics::evaluation::benchmark_topic_discovery \
  -- --ignored --exact --nocapture
```

Use a separate private output directory with `MASH_TEST_TOPIC_HOLDOUT=1` for the
held-out comparison. `MASH_TEST_TOPIC_BASELINES=1` selects the original whole-thread
and raw-message modes instead. The initial report's exact historical results were
produced at commit `c3559c0`; current role-aware deduplication is explicit.

## Interpretation and remaining gates

Xenia, optimized Rust build, the same frozen 789-thread, 384-dimensional,
single-generation corpus. Final unheld-out runs:

| Mode | Radius | Topic count forward / reverse | Fit ms forward / reverse | Assign ms forward / reverse | Pairwise Jaccard | p95 tags forward / reverse | Maximum tags |
| --- | ---: | --- | --- | --- | ---: | --- | ---: |
| User means | 0.4 | 162 / 158 | 182 / 141 | 239 / 218 | 0.820 | 2 / 2 | 8 |
| User means | 0.6 | 62 / 65 | 164 / 109 | 236 / 266 | 0.720 | 2 / 3 | 6 |
| User segments | 0.4 | 4,040 / 4,061 | 797 / 859 | 5,758 / 5,671 | 0.968 | 13 / 13 | 486 |
| User segments | 0.6 | 2,746 / 2,804 | 838 / 815 | 4,123 / 4,155 | 0.543 | 11 / 12 | 275 |

Timing includes building/cloning segment representations and collecting
assignment word contributions, so assignment times are not just vector dot
products. They are single exploratory runs, not statistically calibrated latency
guarantees. All radius-0.4 runs assigned 785 threads. Discovery retained 451 distinct
user means or 5,006 distinct segment vectors after exact-vector deduplication.

The held-out split reserved 169 threads. At radius 0.4, user means assigned
159/161 of those threads (forward/reverse), and segments assigned 162/162.
Held-out runs' pairwise Jaccard was 0.922 for means and 0.950 for segments; it
compares grouping relationships across the entire corpus, not a held-out accuracy
score. Their maximum tag counts were 15 and 475 respectively. At radius 0.6,
held-out coverage was 156/161 for means and 161/158 for segments, with Jaccard
0.706 and 0.490. Coverage does not establish that those tags are correct.

The eight-configuration held-out diagnostic completed in 24.26 seconds, with
`/usr/bin/time -v` peak RSS **239,184 KiB (233.6 MiB)** and 98% CPU utilization.
This includes corpus loading, input/word retention, clustering, assignment,
labeling and report writes; it is not an isolated tree-memory measurement or
evidence of production streaming/index throughput impact.

The first user-only comparison stayed below the 8,192-topic cap. At radius 0.4,
pairwise grouping Jaccard improved to 0.820 for means and 0.968 for segments. However,
segments still assigned as many as 486 direct tags to a thread, and qualitative
inspection found weak labels. Stable repeated templates and many singleton
microclusters can improve this metric without useful semantic organization.

Inspection of large-cluster inputs found recurring agent-work instructions and
technical task prompts. Their recurrence explains some large groups; it does not
justify deleting them. Some are legitimate topics. The label ranking still favors
incidental vocabulary over concise task names.

**Do not promote these settings to MCP topic tags/navigation yet.** BIRCH leaf
microclusters need a separately evaluated semantic aggregation step rather than
being treated directly as user-visible topics. Compare a fixed/versioned projection,
semantic macro-topic grouping, phrase candidates and semantic label ranking. Keep
CF compression topology separate from semantic ancestors. A representative reviewed
thread set, including mixed topics and held-out inputs, remains necessary.

Persistence, accepted revisions, edits/deletes/restarts, stable per-index namespaces,
ancestor inheritance, scope-aware labels/counts, topic filters and MCP navigation
remain outstanding. The shared core and thin CLI/MCP boundary must be retained when
these become ready for integration.

Validation: 483 regular tests passed, including segment boundaries, invalid-input
state preservation, role-aware corpus deduplication, matching-segment label
isolation, evidence filtering, and held-out copied-thread exclusion from discovery.
Both explicit optimized real-corpus runs passed. All-target/all-feature Clippy,
formatting, whitespace and the existing doctest checks passed. Default tests skip
external-service/manual diagnostics; no shared-backend integration behavior changed.
Reports, memberships, titles and diagnostic logs remain outside Git under the
private `mash-artifacts/topic-segments-20261001` directory.
