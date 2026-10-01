# Candidate parent-topic names

Continuation of `mash-c0d.19.2`, after
[semantic ancestry](2026-10-01-topic-ancestry.md). Test-only Rust experiment;
production indexing, CLI, MCP and search remain unchanged.

## Supporting evidence and shared naming

Capture the words from each retained, matching user segment during direct-topic
assignment when hierarchy diagnostics are enabled. Parent naming unions supporting
words across a thread's descendant direct tags **before** counting one contribution
per word/thread/parent. Do not sum child-class frequencies or substitute the entire
conversation's word bag. Require exact direct-tag/evidence alignment; missing or
stale evidence fails rather than silently broadening the source text.

Leaves and groups share a typed `Name` payload, c-TF-IDF utility and the installed
local-model ranking/cache implementation. Topic/group identities remain distinct.
Keep at most 64 candidates and eight output labels per entity. The candidate cache
remains capped at 16,384 vectors and embedding batches at 64; generation matching
and explicit installed assets remain mandatory. No LLM, remote calls or downloads.

Parent ranking uses its normalized descendant-centroid mean, unchanged cosine/
relative-TF-IDF weighting, minimum cosine and duplicate suppression. Counts/terms
come from supporting evidence, not from BIRCH compression nodes. Release captured
per-thread evidence after parent classes are prepared; raw supporting word maps
and vectors are not serialized in reports.

## Full-corpus optimized comparison

Same frozen 789-thread Claude Code/Codex snapshot, cleaned input, compatible local
384-dimensional model, both insertion orders and radii 0.4/0.6. All eight runs pass
without exhausting the candidate cache. The leaf naming pass precedes parent
naming, so parents benefit from candidate embeddings already cached for leaves.

| Radius-0.4 mode / order | Named / total groups | Supporting-word aggregation and c-TF-IDF ms | New parent phrase embeddings | Semantic naming ms |
| --- | ---: | ---: | ---: | ---: |
| Representatives / forward | 102 / 102 | 197 | 93 | 64 |
| Representatives / reverse | 91 / 91 | 214 | 22 | 53 |
| Whole mean / forward | 44 / 44 | 264 | 9 | 52 |
| Whole mean / reverse | 43 / 43 | 255 | 0 | 45 |

All parents have usable labels; this does not mean every leaf is named or every
label is relevant. Parent naming adds roughly 261/267 ms per representative
configuration across the complete corpus in this single-run comparison. Hierarchy
construction/inheritance remain separate. Assignment now captures supporting word
maps for this stage; whole-pipeline overhead must include that work too.

The complete eight-run diagnostic takes 12.84 seconds after compilation. Timing
differences from previous separate runs are not an isolated throughput experiment.
No production search request performs these naming passes.

Largest-group qualitative inspection yields descriptors such as “deadlocks,”
“triage tools,” “test package” and “autonomous workflow.” Some nested groups have
similar descriptors. These are coding-agent observations, not a measured precision
score, independent human review or the required representative 50-thread evaluation.

## Held-out and resource check

The unchanged projected 174-thread held-out partition gives 83/85 representative
groups at radius 0.4, all named. Word aggregation/c-TF-IDF takes 266/250 ms;
semantic naming takes 70/64 ms with only 73/20 new phrase embeddings. Whole means
produce 40/40 named groups in both orders. Labels include assigned held-out text,
so this is not independently held-out label accuracy.

The eight-run held-out process takes 14.26 seconds and peaks at 589,964 KiB
(576.1 MiB) RSS, using 160% CPU. This includes corpus/source strings, installed
sessions, captured supporting words, temporary parent classes, candidate cache,
graph matrices and report writes. It does not establish a production memory bound
or streaming/index throughput guarantee.

A read-only artifact audit compares all 16 current reports with the preceding
hierarchy experiment: direct memberships, semantic forest, inherited memberships
and distinct-thread/provider counts are unchanged. Named-parent counts match the
actual nonempty output labels. Parent labels do not alter direct assignment or
the earlier stability result, which still misses the proposed held-out target.

## Validation and next work

505 regular tests pass. The new regression verifies one word contribution per
thread at every parent, overlapping-child deduplication, sibling-word isolation
and failure on missing/stale supporting evidence. Shared payload/naming refactoring
retains existing leaf-label tests. Optimized real-model full/held-out diagnostics,
all-target/features Clippy, formatting, diff checks and doctests pass. Regular
validation skips external services; no production backend changes.

Use the same projection/hierarchy diagnostic flags as the preceding report.
Detailed labels, identifiers, graph and audit artifacts remain outside Git in
`mash-artifacts/topic-parent-labels-20261001`. Python performs artifact comparison
only; evidence aggregation and naming remain Rust.

Next: review parent/child label usefulness and redundant nesting, improve weak
context-dependent leaves, complete representative relevance/overlap evaluation,
then implement stable revision lifecycle and scope-safe persistence/MCP tools.
Neither parent labels nor inherited tags are exposed through production MCP yet.
