# Current topic worklist

Use this worklist with [SELECTION_CRITERIA.md](SELECTION_CRITERIA.md).
Historical experiment descriptions and their suggested next steps do not
authorize another algorithm round. The first milestone is flat multi-label
topics; no semantic hierarchy or ancestor inheritance is required.

The user's updated priority is topic quality with genuine online discovery.
There is no fixed percentage overhead rejection gate; report full ingestion
cost, memory and publication lag, and require bounded sustained-arrival backlog.
The finite experiment cap remains in force.

The obsolete hierarchy-frontier prototype, auditor and test have been removed
from the active tree. Their source is retained in Git at `8304e2d` and their
[negative-results report](../../benchmarks/2026-10-02-supported-topic-frontier.md)
remains available. Current flat-topic references and quality/parity checks are
unchanged; historical experiment reports are evidence, not the execution queue.

Explicit follow-up requested by the user: `mash-c0d.19.2.40` tracks a bounded
online ensemble evaluation (unchanged phrase baseline versus one frozen
phrase/dense hybrid, plus exact repeat). This is a separate authorized comparison,
not reopening the completed NMF grid. Freeze its rules before execution and
apply the same independent quality, online, lifecycle and reference gates.
The frozen control and exact repeat are now recorded in
[the ensemble report](../../benchmarks/2026-10-02-topic-ensemble.md): 720 tags
become 538, untagged sources rise 769 to 1,022 and six threads lose all tags.
This is a coverage risk, not an independent quality verdict. No promotion or
cutoff tuning; complete online publication, quality and ingestion gates remain.

## Decisions already made

The fixed NMF comparison is complete and its tested configurations were rejected.
Source-aware phrase subjects remain a candidate for independent review. Neither
candidate has passed the production acceptance gates. Keep the original captures,
reports and manuscript results; do not relabel development evidence as held-out
quality or erase negative outcomes.

Two previously active hierarchy tasks were closed as superseded on 2026-10-02:

| Bead | Disposition and retained evidence |
| --- | --- |
| `mash-c0d.19.2.13` | BIRCH/GRINCH hierarchy comparison remains a historical control. Its [recorded comparison](../../benchmarks/2026-10-01-topic-grinch.md) did not justify default promotion. Further semantic-parent/tree selection is outside this milestone. This does not universally reject either algorithm. |
| `mash-c0d.19.2.20.8.3.3` | Hierarchy/frontier selection is superseded. Its [frozen QMSum diagnostic](../../benchmarks/2026-10-02-qmsum-hierarchy.md) did not justify further frontier tuning. Other source-unit/label work is governed by the flat acceptance contract. |

Closing those issues records a scope decision, not successful hierarchy quality,
completion of their descendants, or completion of the topic feature. Related
flat-quality/lifecycle work remains open under `mash-c0d.19` and
`mash-c0d.19.2`. Earlier follow-up suggestions for cuts, linkage or tree semantics
are historical hypotheses rather than the current execution queue.

## Remaining gates and next actions

| Work | Current evidence and required next action |
| --- | --- |
| Independent semantic review | Pending. Existing exposed development reviews and the smaller prepared packet cannot meet the scope gate. Review at least 20 previously unexposed threads across two providers, with at least 60 certain subjects, 20 primary and 20 secondary. Freeze source-backed subjects before exposing blinded candidate output. No assistant-created gold. `mash-c0d.19.2.33.1` and the retained candidate `mash-c0d.19.2.38.9` track this work. |
| Declared score and provenance audit | The [scorer](TOPIC_QUALITY_REVIEW.md) exists. Once review is complete, report all tags/subjects, uncertainty and thread-level intervals; separately verify actual source/candidate pins, chronology, independence and duplicate exclusions. A passing declaration is insufficient. |
| Online discovery | Pending acceptance. Demonstrate topics published before full indexing, bounded incremental updates and new-topic discovery without full-corpus reruns. Measure sustained-arrival backlog and publication lag; test edits/deletions/restart. Assignment into a frozen taxonomy alone cannot pass. |
| Complete ingestion cost | Pending. Measure matched initial/incremental indexing with topics on/off, including context, inference, labels, clustering, persistence and draining topic work on supported node classes. The user permits a generous budget: no fixed percentage rejection gate. Record elapsed time, throughput, CPU, peak RSS, p95 updates and publication lag, and verify bounded backlog without blocking ordinary search. Refresh-only Python timings cannot pass. `mash-c0d.19.2.38.7` tracks bounded publication/cost evaluation. |
| Scoped lifecycle | Pending acceptance. Verify stable/versioned identities, ownership, duplicate votes, updates/deletes, publication and restart against actual persisted local/shared state. Small prototype fixtures establish only their own coverage. |
| Reference and Rust | Conditional on all selection gates passing. Freeze the executable [port contract](PORT_CONTRACT.md), then implement Rust and require fixed-vector/full-path differential CI and the same quality/performance checks. No new unselected Rust topic prototype. |
| Flat MCP integration | Conditional on selection and Rust validation. Shared core supplies flat topic browsing/detail, thread metadata tags, and composable any/all topic filters. Search remains available without topics; topic metadata cannot silently discard direct search candidates. |
| Guided retrieval and paper | `mash-afc.1` and `mash-afc` stay open. Record reviewed retrieval/no-regression evidence and actual procedures/results/limits before claiming guided-search gains in the manuscript. |

The predefined [captured publication control](../../benchmarks/2026-10-03-topic-publication.md)
now publishes intermediate snapshots with one job and one pending revision, and
drains to the unchanged phrase baseline exactly. At 100 arrivals/second on 1850
captured sources, p95 publication lag is 5.887/6.005s and final drain 4.781/4.544s.
This finite replay is useful publication evidence; it does not pass indefinite
streaming, complete native ingestion, search isolation, semantic quality or
production lifecycle gates. Snapshots still recompute lexical corpus statistics,
and the reference has a 2048-source/8MiB capacity. No new algorithm was selected.

The frozen [longer captured-source churn control](../../benchmarks/2026-10-03-topic-publication-churn.md)
adds6000 delete/reinsert events over60 seconds after initial admission. First/repeat
publish30/31 snapshots, drain to exact revision7850/final baseline, and retain
one job/one pending marker. Six-window p95 ledger advancement lag spans
6.172--7.144/5.926--6.436s; peak revision backlog754/674. Approximately half
the rapid mutations are superseded before visibility; the control distinguishes
latest captured source versions from mere revision advancement. This strengthens
finite scheduler/lifecycle evidence within the same bounded cohort, not semantic
quality, indefinite stability, native ingestion/search isolation or production
persistence. No algorithm/threshold changed; full gates remain open.

## Finite execution rule

The method comparison is finished. There are no automatically queued new
algorithm families, parameter sweeps, stoplist expansions or weighting variants.
At most one named correction within the retained approach is permitted: one
identified failure, at most two frozen arms and an exact repeat. This allowance
is unused; it is not a requirement to spend it.

Complete the predefined review, lifecycle, cost and reference checks, then record
acceptance or the failed gates. If no candidate passes, end this research round
and leave the feature unselected. Another round needs an explicit scope/budget
decision. Continue actionable broader production work while independent review
is pending. After acceptance, freeze the algorithm and implement/validate it;
further optimization requires a measured regression or new requirement.

Tracking cleanup: `mash-c0d.19.3`. This document creates no new research
experiment, semantic judgment, performance result or acceptance claim.
