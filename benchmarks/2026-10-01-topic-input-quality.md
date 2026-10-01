# Topic evidence quality

Continuation of `mash-c0d.19.2`. The topic implementation remains a test-only
Rust prototype; production CLI, MCP and indexing behavior are unchanged.

## Exploratory review

Prepared 50 private review cases from the frozen 789-thread Claude Code/Codex
corpus, stratifying by provider and embedded user-message count, excluding exact
ordered raw-vector copies, and adding named-project/storage examples. Reviewed
sampled source excerpts and predictions for seven cases so far. This is coding-agent
inspection, not independent human review or the required 50-thread relevance gate.
Sparse excerpts cannot establish full-thread topic recall. No accuracy percentage
is inferred from these observations.

The inspection identified these concrete defects:

- Background task notifications stored with a user role produce delivery-event
  topics rather than the actual conversation subject.
- Supplied project instructions can contribute incidental Git-policy topics.
- Separate discovery leaves sometimes yield nearly identical tags.
- Some long, mixed-subject threads receive no tags, including the mash discussion.
- Interruption/tool-delivery vocabulary still contributes noise.

## Change and bounds

Exclude complete standalone `<task-notification>` envelopes from user-topic
discovery and assignment. Require task-ID markers, no nested notification envelope
and no trailing human text. Also recognize the exact observed system-notification
header preceding that envelope. Preserve quoted examples, incomplete envelopes,
unknown header formats and mixed human requests.

This is an intentionally conservative input rule, not a general XML validator.
Canonical history, source databases, vectors and ordinary search remain unchanged.
The corpus reader regression checks that filtered event vectors/text stay available
to the canonical baseline while contributing no user-topic segment.

The frozen corpus now excludes 851 distinct user-message contributions from topic
evidence, versus 106 with only the previous acknowledgment rule: 745 additional
notification records. This is a deduplicated message count, not a deleted-record
count or a count of affected threads.

## Optimized corpus results

Same explicit installed 384-dimensional local model and frozen corpus as
[the label comparison](2026-10-01-topic-labels.md); no downloads or remote calls.

| Radius / order | Discovery centroids | Active labeled topics | Assigned threads | P95 tags | Maximum tags | Assignment ms | Semantic-label ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0.4 / forward | 159 | 126 | 765 | 3 | 21 | 388 | 1,208 |
| 0.4 / reverse | 153 | 118 | 762 | 3 | 17 | 364 | 247 |
| 0.6 / forward | 61 | 45 | 753 | 3 | 6 | 236 | 19 |
| 0.6 / reverse | 66 | 47 | 743 | 3 | 5 | 253 | 19 |

Radius-0.4 pairwise co-membership Jaccard is 0.884 across insertion orders,
versus 0.874 before notification filtering. Radius 0.6 remains below the proposed
0.8 stability target at 0.749. The four-run diagnostic takes 5.58 seconds after
compilation. First-pass naming embeds 4,658 candidates; reverse order adds 752.
Every active topic receives a semantic label. Coverage and stability do not measure
relevance or label usefulness, and these single-run times are not a search latency
benchmark or an isolated throughput comparison.

The updated copy-aware held-out partition reserves 169 threads. Radius 0.4
assigns 150/169 in forward order and 149/169 in reverse order, with overall
coverage 753/752 and pairwise Jaccard 0.876. All 106/107 active topics receive
labels. The four-run held-out process takes 6.52 seconds and peaks at 531,636 KiB
(519.2 MiB) RSS, including corpus retention, model sessions and label caches.
Filtering changes the held-out partition; this is not the same split as the
previous 170-thread run. Labels still incorporate assigned held-out text, so
these are assignment coverage results, not independently held-out label accuracy.

Reinspection of affected cases confirms that the task-notification topic disappears.
However, one thread still receives interruption topics instead of its requested
subject, and the unassigned long discussion remains unassigned. The remaining
review, template/instruction handling and mixed-thread discovery defects need work
before promotion to persistent topics or MCP navigation/filter tools.

## Validation

492 regular tests pass, including two notification-boundary tests and the extended
corpus-reader regression. The optimized real-model corpus diagnostic passes.
Clippy with all targets/features, formatting and doctests pass. External-service
tests are excluded from the regular run; no production backend path changed.

Private source excerpts, thread identifiers, annotations and detailed reports remain
outside Git in `mash-artifacts/topic-review-20261001`. Production stable topic IDs,
revision lifecycle, semantic ancestry and inherited tags, scoped labels/counts,
MCP navigation and composed topic filters remain unimplemented.
