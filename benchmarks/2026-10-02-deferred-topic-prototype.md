# Bounded online routing and deferred topic publication

This is an inspectable research prototype, not a selected production algorithm.
It uses existing stored MiniLM embeddings without inference, model downloads,
LLM calls or paid services. Production ingestion and MCP remain unchanged.

## Procedure

The frozen `search-performance-d62a597/index.sqlite3` snapshot has SHA256
`251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`.
The driver records embedding generation and material dependency hashes.
Source-only eligibility requires 3–80 nonempty user messages, 4–32 stored
vectors, at most 512 KiB each of original/represented thread text and 256 KiB
per represented message. Exact ordered user-text aliases collapse before
deterministic sampling. Requested four threads per provider; only three Claude
Code groups qualified, versus 78 Codex groups. The first attempt failed before
clustering because it required four. The revised driver preserves eligibility,
retains the shortfall explicitly and selects three Claude Code plus four Codex
threads. This is availability-selected, already exposed development data, not
held-out evaluation or proof of native history completeness.

Round-robin arrival preserves order within each thread; reversed arrival is a
diagnostic control. Every message remains present, including missing vectors.
Cosine routing uses a fixed .55 acceptance threshold, at most 128 leaves,
2,048 sources and 8 MiB represented text. It keeps sums/counts and rejects
overflow rather than inventing membership. One background worker coalesces
refreshes requested every 16 events; ingestion does not await its work.

Snapshot memberships use cosine .55, scores within .8 of the best, and at most
three tags per message. Own-centroid scoring excludes the message when its
anchor has multiple sources; singleton self-membership is provisional. The
background tree uses average linkage on source-pooled lexical TF-IDF vectors.
Source-only snapshot IDF does not change the online dense coordinate frame.
c-TF-IDF/IDF labels require support from two sources, discard terms above .4
source DF and prune redundant phrases. Thread tags union message memberships;
ancestors inherit structurally. Weak labels and zero lexical geometry remain
explicit. Snapshot versions reject edited/deleted evidence; insert-only lag is
visible. Checkpoints retain exact numerical state for restart.

## Observations

149 messages had 74 stored vectors and 75 missing vectors. Forward routing
produced 57 leaves and 113 total tree nodes; 45 leaves had one member and 47
leaves lacked two supported label terms. Two messages had multiple tags.
Reversed arrival produced one multi-tag message. Leaf co-membership pair
Jaccard was .710526 (37 forward pairs, 28 reverse, ten forward-only and one
reverse-only). These expose fragmentation and insertion-order sensitivity;
neither source-supported labels nor structural ancestors prove topic quality.

Two sequential complete runs matched every stable artifact exactly, excluding
runtime/scheduler observations. In-memory deletion removed the original source
from the tree and rejected a stale snapshot; reinsertion and checkpoint restart
reproduced the refreshed tree. Synthetic tests also cover semantic replacement,
capacity, cancellation, stale publication and blocked-worker coalescing.
An auditor importing no producer modules checked original source text hashes,
149 identities/versions, all 113 node source unions and seven thread tag/ancestor
unions. It does not reconstruct numerical routing, label scores, sampling or
embedding decoding, and is not semantic validation or Python/Rust parity.

Forward routing wall time was 124.82/68.73 ms; through final publication
719.29/438.09 ms; foreground upsert p95 3.79/1.89 ms; CPU 711.44/418.13 ms.
Three/two refresh jobs completed. Background tasks are bounded but Python GIL
contention, scheduling, caches and host load affect these measurements.
External target-process smaps RSS maxima were 151,460/162,840 KiB and PSS
143,690/155,116 KiB. Approximate status HWM was 151,948/162,856 KiB, with
297/294 samples. Sampled maxima are lower bounds; observer/children excluded,
observer overhead included in runtime and rusage can retain launcher peaks.
The target includes imports, input loading, both arrival orders and lifecycle
checks: these are neither incremental topic-only memory nor full-ingestion
benchmarks. The <=10% production ingestion overhead gate remains untested.

## Artifacts and next decision

Private outputs are under `topic-deferred-prototype-final-first-20261002` and
`topic-deferred-prototype-final-repeat-20261002` in mash-artifacts. Each contains
`view.html`, `topics.json`, `thread-tags.json`, `sources.json`, `checkpoint.json`
and `results.json`. Matching memory sidecars and
`topic-deferred-prototype-audit-20261002.json` record scoped checks. No private
text or generated labels are checked into Git.

The 620-test research suite passed in 5.382 seconds. This establishes a
testable end-to-end pipeline shape, not acceptable clustering. Next evaluate
source-backed message/episode granularity and supported leaf consolidation
against this frozen baseline, including order sensitivity and rejected/noise
coverage. Do not lower thresholds to make this cohort look better or promote
the method without independent coding-session quality, complete-ingestion
performance and selected-method differential Rust gates.
