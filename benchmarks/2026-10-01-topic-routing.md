# Exact nearest-leaf topic insertion

`mash-c0d.19.2.5`, following the
[UI subject projection](2026-10-01-topic-ui-subjects.md). Rust test-only experiment;
production indexing, search, CLI and MCP are unchanged.

## Reproduced routing miss

Current CF insertion follows the closest immediate branch centroid, then searches
only that branch. This does not guarantee finding the closest leaf. A deterministic
two-dimensional fixture built through normal insertion reproduces the consequence:
after 27 samples, an existing point creates another leaf instead of updating its
original one. A nearby unique point does the same, so exact-vector deduplication
does not prevent the problem. With the 25-topic capacity already filled, branch
routing rejects both insertions even though an existing leaf can accept them.

Add typed `BranchCentroid` and `ExactLeaf` routing settings. The latter scans leaf
CF means directly, compares each distance once, and breaks exact ties by topic ID.
It does not materialize all centroid vectors or sort candidates. Try merging into
the nearest leaf under the existing RMS-radius rule; update its count/sum/squares
and every ancestor's cached feature on success. If that leaf rejects the point,
route a new leaf through the existing tree structure. Do not silently merge into
a farther leaf reached by the old branch route.

Existing live IDs, radius semantics, topic/sample limits and failure atomicity
remain intact. Scan work is O(leaves × dimensions), bounded by the existing leaf
and dimension settings. Traversal uses the tree's recursion stack and a single
best candidate; a selected merge copies one feature vector. This is not a proof
of a production process-memory/depth bound or constant insertion cost at scale.
CF storage branches remain distinct from semantic topic ancestors.

The ignored diagnostic opts in through `MASH_TEST_TOPIC_EXACT_ROUTING=1`. Report
models serialize the routing enum, including rejected configurations. Unset the
variable for the branch-routing control. Both alternatives remain experimental;
there is no new production configuration or automatic index rebuild.

## Fixed-corpus results

Same 789-thread Claude Code/Codex snapshot, compatible local model, input cleanup,
canonical reserved split, discovery/assignment thresholds, both insertion orders
and radii, and representative/anchored modes. All 16 control reports reproduce
the preceding baseline exactly apart from timing and the new routing metadata.
Exact routing keeps canonical source IDs, partition and eligibility unchanged:
778 eligible threads and 145 eligible reserved threads.

Representative discovery at radius 0.4:

| Metric | Branch: full | Exact: full | Branch: reserved run | Exact: reserved run |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 769/778 | 769/778 |
| Reserved assigned / eligible, both orders | — | — | 136/145 | 136/145 |
| P95 direct tags, both orders | 5 | 5 | 5 | 5 |
| Maximum tags, forward / reverse | 44 / 39 | 39 / 36 | 45 / 40 | 43 / 44 |
| Active leaves, forward / reverse | 431 / 430 | 409 / 408 | 376 / 378 | 373 / 370 |
| Named leaves, forward / reverse | 430 / 429 | 408 / 407 | 375 / 376 | 372 / 368 |
| Parent groups, all named, forward / reverse | 107 / 107 | 96 / 88 | 88 / 91 | 90 / 83 |
| All-instance pair stability | 0.935 | 0.950 | 0.941 | 0.952 |
| Canonical-copy collapsed stability | 0.888 | 0.915 | 0.899 | 0.918 |
| Reserved-only canonical stability | — | — | 0.833 | 0.877 |
| Older raw-vector collapsed stability | 0.779 | 0.826 | 0.800 | 0.828 |

The representative configuration now exceeds the proposed 0.8 stability threshold
in both raw-vector scopes, with unchanged coverage. This does not establish
order independence, relevance accuracy, stable IDs across rebuilds, or success for
all configurations. Exact anchored discovery still falls short in that measure:
0.799 full and 0.782 reserved. All configurations remain in the private metrics.
Reserved maximum tags increase in the reverse order, and weak unnamed leaves
remain. Do not substitute names/coverage/stability for full-source relevance review.

Source-backed spot checks retain the improved UI subjects. Contextual acknowledgment,
truncated-name and generic-question tags are still present in other inspected
threads. The missing AEO consultancy subjects remain unassigned in the reserved
model. Prefer exact representative routing for the next quality evaluation, while
keeping the branch control available and withholding production adoption.

## Cost and validation

Paired optimized runs on Xenia, representative radius 0.4:

| Time | Branch: full | Exact: full | Branch: reserved | Exact: reserved |
| --- | ---: | ---: | ---: | ---: |
| Fit, forward / reverse | 326 / 345 ms | 503 / 412 ms | 295 / 247 ms | 487 / 353 ms |
| Assignment, forward / reverse | 761 / 781 ms | 735 / 754 ms | 701 / 669 ms | 688 / 715 ms |
| Eight-run diagnostic | 20.33 s | 20.68 s | 18.81 s | 19.47 s |

Fit cost increases; assignment cost is similar. Whole diagnostics include installed
models, retained corpus, projection, labels, hierarchy, evidence and report writes.
Exact reserved peak RSS is 1,109,788 KiB, versus 1,110,256 KiB for the branch control
(both about 1.06 GiB). These are single paired measurements, not production
indexing-throughput, streaming-overhead, search-latency or Mac guarantees. Larger
topic populations need separate scaling evaluation before adoption.

528 regular tests pass with 107 skipped; 57 focused topic tests pass. Regression
tests cover the branch miss for repeated/unique points, capacity reuse, deterministic
ties, no farther-leaf fallback, invalid/capacity/sample-limit rejection, and exact
ancestor statistics. Optimized control/exact full/reserved diagnostics, all-target/
all-feature Clippy, doctests, formatting and diff checks pass.

Independent audits verify unchanged control math outputs, canonical identity and
eligibility, distinct provider/thread counts, acyclic forest relationships and
deduplicated ancestors. All 1,911 captured source references per exact report resolve
against read-only SQLite with matching ownership, ordinal and canonical BLAKE3
fingerprints. The same 138 incomplete excerpts remain incomplete. Private artifacts
live in `mash-artifacts/topic-routing-20261001`; no real IDs or histories enter Git.
Python only audits artifacts; topic insertion and inference remain Rust.

The routing evaluation bead is complete. Full-source precision/overlap review,
remaining topic noise, scale/resource bounds, namespace/lifecycle persistence and
scope-safe MCP tree exploration, thread tags and filters remain parent work.
