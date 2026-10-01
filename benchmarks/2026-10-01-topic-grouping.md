# Separating topic discovery from segment assignment

Tracked in `mash-c0d.19.2`, following the
[user-segment evaluation](2026-10-01-topic-segments.md). The adoption gates from the
initial report remain in force. This is test-only Rust experimentation, with no
change to production search, ingestion, CLI or MCP behavior.

## Compared approaches

**Leaf grouping:** normalize CF leaf centroids, sort them in canonical vector
order, and greedily group them with a complete-link admission rule: every pair
of member centroids must meet the configured cosine threshold. Choose the nearest
eligible group's count-weighted center. This prevents chaining dissimilar endpoints
through similar neighbors; it does not prove semantic coherence of represented
messages. A group's center weights normalized leaf centroids by their sample counts.

The experiment bounds inputs at 8,192 leaves, validates dimensions, identities and
sample counts, and uses quadratic pairwise work plus candidate sorting. It retains
linear vector storage. Canonical ordering reproduces results for the same leaf set;
different BIRCH insertion orders can produce different leaf sets and hence different
groups. This is a batch grouping pass, not persisted online hierarchy maintenance.
Group IDs are experiment-local and are not exposed as stable topic identities.

**Thread discovery with segment assignment:** feed one normalized user-message
mean per thread into BIRCH, then assign the thread's individual semantic segments
to the resulting centroids. This keeps the discovery population coarse while
permitting segments of a mixed-topic thread to match separate topics. It avoids
treating every small message cluster as a user-visible topic. Labels and support
still come from matching segments. Segment admission, cosine thresholds and
evidence weighting are unchanged from the preceding experiment.

Both approaches retain original 384-dimensional coordinates and require the same
embedding generation. CF storage branches are not semantic parents. Neither
approach implements ancestor inheritance, stable identities or source revisions.

## Results

Xenia, optimized build, same frozen 789-thread corpus. Counts and times below are
forward/reverse discovery order. They are exploratory individual runs, not latency
guarantees or judged accuracy scores.

| Approach | CF radius | Topics | Grouping ms | Assignment ms | Pairwise Jaccard | p95 tags | Maximum tags |
| --- | ---: | --- | --- | --- | ---: | --- | ---: |
| Leaf grouping, cosine 0.45 | 0.4 | 1,830 / 1,836 | 891 / 888 | 3,000 / 3,003 | 0.675 | 12 / 11 | 239 |
| Leaf grouping, cosine 0.45 | 0.6 | 1,433 / 1,444 | 456 / 472 | 2,209 / 2,227 | 0.393 | 11 / 11 | 192 |
| Leaf grouping, cosine 0.60 | 0.4 | 3,017 / 3,029 | 1,462 / 1,419 | 5,068 / 6,076 | 0.874 | 13 / 13 | 398 |
| Leaf grouping, cosine 0.60 | 0.6 | 2,230 / 2,272 | 690 / 812 | 4,149 / 4,065 | 0.536 | 12 / 11 | 231 |
| Thread discovery, segment assignment | 0.4 | 162 / 158 | — | 618 / 560 | 0.879 | 3 / 3 | 17 |
| Thread discovery, segment assignment | 0.6 | 62 / 65 | — | 577 / 618 | 0.705 | 3 / 3 | 8 |

Leaf grouping starts from 4,040/4,061 leaves at radius 0.4 and 2,746/2,804 at
radius 0.6. Coarser grouping reduces assignment cost and tag count but amplifies
ingestion-order differences. Stricter grouping passes the proposed stability gate
at radius 0.4 but retains too many small topics. It is not the preferred path.

Thread discovery retains 451 distinct means after exact-vector deduplication.
At radius 0.4 it assigns 772 threads in both orders, versus 785 with ungrouped
segment discovery: lower coverage is a trade-off, not automatically an improvement.
Discovery takes 225/276 ms and labeling 66/54 ms; assignment is about 0.6 seconds
for the entire 789-thread corpus. The preceding raw segment approach took roughly
5.7 seconds for assignment and reached 486 tags on its longest thread. These are
separate runs under shared-machine load; no statistically calibrated speedup claim
is warranted.

## Held-out check and recommendation

The copy-aware split reserves 169 threads without letting their exact user-vector
sequences train discovery. At radius 0.4, thread discovery with segment assignment
assigns **154/169 held-out threads in both orders**, with overall pairwise Jaccard
**0.913**, p95 three tags, and maximum 17/13. Overall coverage is 762/763 threads.
Radius 0.6 assigns 151/150 held-out threads and falls to Jaccard 0.633.

The four-configuration held-out diagnostic completes in 5.21 seconds and peaks at
202,564 KiB (197.8 MiB) RSS under `/usr/bin/time -v`. This includes retained input
vectors/words and private report writes; it is not an isolated production topic
worker measurement. Labels include held-out assignments, so this tests centroid
assignment coverage rather than independently held-out label accuracy.

**Use radius-0.4 thread discovery with segment assignment as the next candidate,
not as an adopted production configuration.** It substantially improves practical
tag counts while clearing the proposed grouping-stability target. However,
qualitative largest-cluster label inspection still finds incidental task vocabulary.
Coverage and grouping stability do not prove that individual topic tags are relevant.
Phrase candidates and semantic label ranking, a representative reviewed thread set,
and an explicit outlier/missing-topic assessment remain necessary.

After quality passes, the remaining work includes accepted-revision publication,
edits/deletes/restarts, model-versioned stable per-index IDs, meaningful semantic
parent relations with inherited tags, and scoped MCP navigation/filters. Do not
silently interpret the CF tree or this batch grouping as that hierarchy. Discovery
from evolving thread means also needs explicit replacement/update semantics.

## Reproduce and validate

```sh
MASH_TEST_SEARCH_DB=/path/to/frozen.sqlite3 \
MASH_TEST_TOPIC_REPORT_DIR=/path/to/private/reports \
MASH_TEST_TOPIC_GROUPING=1 \
cargo test --release --lib topics::evaluation::benchmark_topic_discovery \
  -- --ignored --exact --nocapture
```

Replace `MASH_TEST_TOPIC_GROUPING=1` with `MASH_TEST_TOPIC_ANCHORED=1` for thread
discovery with segment assignment. Add `MASH_TEST_TOPIC_HOLDOUT=1` and use a
different private output directory for held-out runs.

Tests cover chain prevention, canonical-order reproducibility, weighted sample
conservation, invalid/colliding identities and counts, and end-to-end secondary
topic matching with label isolation. Raw titles, memberships and diagnostic logs
stay outside Git in `mash-artifacts/topic-grouping-20261001`.

Validation: 486 regular tests passed, together with the explicit optimized grouping,
thread-discovery and held-out diagnostics. All-target/all-feature Clippy, the
existing doctest, formatting and whitespace checks passed. External-service/manual
tests are skipped by the regular suite; this change affects only test-only topic
modules and does not change shared-backend integration behavior.
