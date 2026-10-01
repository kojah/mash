# Candidate semantic ancestry

Continuation of `mash-c0d.19.2`, on top of
[derived topic projection](2026-10-01-topic-input-projection.md). All code remains
test-only. Production indexing, CLI, MCP and search are unchanged.

## Structure and guarantees

Build a separate candidate hierarchy from **active topic centroids**, not the
internal BIRCH CF branches. Use complete-link agglomeration: a merge's similarity
is the minimum cosine across its two descendant sets. A maximum heap selects the
closest active pair; stale pairs are discarded. This prevents similar-neighbor
chains from joining dissimilar endpoints.

Retain nested cuts at cosine 0.75, 0.60 and 0.45. Suppress intermediate binary
merge nodes that add no cut-level distinction. Preserve every direct topic and
its closest retained parent. Unrelated topics remain separate forest roots;
there is no artificial all-topics ancestor. At most three semantic ancestors
occur on a leaf path under these settings.

Typed `NodeId::Topic(TopicId)` and `NodeId::Group(GroupId)` keep leaf/group
identities distinct even when numeric values coincide. Expose parent and children
in the private graph report. Direct memberships stay separate from inherited
group memberships. Inherit every retained ancestor and deduplicate shared ancestors
across a thread's direct tags. Unknown identities fail rather than disappearing.
Group thread/provider counts use distinct inherited thread sets.

IDs are experiment-local, not stable model/index/revision namespaces. Cosine
cohesion is a geometric guarantee, not proof of meaningful topic names or human
semantic ancestry. Parent naming and relevance evaluation remain unfinished.

## Bounds

Reject more than 1,024 active topics, dimensions outside 1–2,048, duplicate IDs,
invalid vectors and invalid/non-descending cuts. Permit at most eight cuts.
Construction uses `O(n² * (dimensions + log n))` work and bounded `O(n²)` matrix/
heap storage. Input order is canonicalized by normalized vector and ID. Group
centroids are normalized unweighted means of descendant topic centroids; they
are not used to reinterpret BIRCH storage branches as semantic parents.

This is batch derived-index work. It does not implement online semantic-hierarchy
updates, retractions, stable-ID matching or persisted checkpoints.

## Real-corpus results

Same frozen 789-thread Claude Code/Codex corpus, explicit installed local model,
cleaned topic evidence and optimized Rust build. Three cuts above, both insertion
orders, representative and whole-mean modes, radii 0.4/0.6. No source writes,
downloads or remote inference.

| Radius-0.4 mode / order | Active leaves | Semantic groups | Forest roots | Maximum ancestry depth | Build ms | Inheritance/count ms | Deduplicated ancestor contributions |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Representatives / forward | 399 | 102 | 256 | 3 | 24.3 | 1.9 | 889 |
| Representatives / reverse | 385 | 91 | 256 | 3 | 19.8 | 1.6 | 642 |
| Whole mean / forward | 118 | 44 | 47 | 3 | 2.7 | 1.2 | 494 |
| Whole mean / reverse | 118 | 43 | 47 | 3 | 2.8 | 1.1 | 393 |

Roots include singleton direct topics as well as semantic groups. Maximum inherited
tags per thread are 25 in both representative runs, separate from direct tags.
Many roots remain separate, so this is not yet a compact, reviewed browsing taxonomy.
Deduplicated contributions count repeated parent paths removed within threads;
they do not count deleted messages or filtered direct tags.

Ancestry adds roughly 21–26 ms per preferred representative configuration for the
complete corpus in this single-run comparison. The eight-run diagnostic takes
10.63 seconds after compilation. These are derived-graph timings, not search latency
or proof of less than 10% production indexing overhead. No graph work runs on the
production search path.

## Held-out and structural audit

Under the same projected 174-thread held-out partition, representative runs use
334/338 active leaves, create 83/85 groups and 218 roots, and retain depth three.
Build time is 16.7/16.5 ms; inheritance/counts take 1.5 ms in both orders.
921/796 duplicate ancestor contributions are removed. Direct membership behavior
is unchanged by the optional hierarchy stage.

The eight-run held-out process takes 11.22 seconds and peaks at 569,496 KiB
(556.1 MiB) RSS with 163% CPU, including corpus/source strings, installed model
sessions, projection plans, phrase cache, graph matrices and report writes. This
does not isolate graph memory or establish a production memory bound.

A read-only artifact audit checks all 16 full/held-out reports: unique typed node
identities, exact roots and child/parent relationships, acyclic inheritance,
inherited memberships equal to the union of direct-tag ancestors, and exact
distinct-thread/provider counts against the frozen SQLite metadata. This is
structural verification, not independent semantic relevance review.

The audit also distinguishes active from named leaf topics: the full representative
runs have 399/385 active leaves but only 398/384 usable labels. The previous
projection report is corrected; unnamed leaves remain a quality issue rather
than silently being excluded from ancestry.

## Validation and remaining work

504 regular tests pass, including every-ancestor inheritance, overlap deduplication,
complete-link cohesion across all descendant pairs, order-independent input,
typed namespace separation, bounds/invalid inputs, and distinct cross-provider
thread counts. Optimized full/held-out real-model diagnostics, all-target/features
Clippy, formatting, diff checks and doctests pass. Regular validation skips external
services; no production backend changes.

Add `MASH_TEST_TOPIC_HIERARCHY=1` to the preceding projection diagnostic flags.
Detailed private graphs, IDs and artifacts remain outside Git under
`mash-artifacts/topic-hierarchy-20261001`. Python is used for artifact audit only;
hierarchy construction, inheritance and counts are Rust.

Next: name and review candidate parents, improve weak/unnamed/context-dependent
leaf topics, complete representative relevance/overlap evaluation, then implement
stable revision lifecycle and scope-safe persistence/navigation/filtering. Topic
MCP tools and production inherited tags are still unexposed.
