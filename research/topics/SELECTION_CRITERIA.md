# Flat multi-label topic selection: acceptance and stopping rules

Adopted 2026-10-02 after the user's request for a finite decision process.
Earlier protocols contain individual gates, but did not cap successive topic
experiments. These are prospective engineering targets, not research-derived
constants or a claim that earlier results pass. Exposed development reviews
cannot satisfy the independent quality gate below.

## Finite current round

1. Finish the already frozen NMF16/32, one/five-pass, forward/reverse comparison,
   including exact repeat, fixtures and results. No extra K/seed/threshold grid.
2. Compare with the existing source-aware phrase candidate; use whole-thread
   phrase weighting as the unchanged baseline. Do not add algorithm families,
   stoplist expansions, background-weight variants or new clustering sweeps.
3. Publish a decision table: rejected, retained for independent review, or accepted
   for Rust implementation. A retained candidate is not production-selected.
4. Permit at most **one** explicitly specified corrective experiment within the
   retained approach before that review: one identified failure, at most two
   arms, frozen rules, one exact repeat. No iterative follow-on tuning.
5. End this round with an acceptance or failed-gate report. If no candidate passes,
   stop topic-algorithm experimentation in this round. Record unmet requirements;
   do not silently lower targets, relabel proxies as quality, ship weak topics,
   or mark the whole production-readiness goal complete. Continue actionable
   search/provider/observability work while independent review is pending.
   A further research round needs an explicit scope/budget decision, rather than
   being an automatic continuation of this one.

The cap applies to algorithm/parameter experiments. Fixing implementation bugs,
auditing results, performing predefined review/performance/parity checks and
correcting factual documentation remain required work, not new search rounds.

## Accept for Rust implementation only when all gates pass

| Gate | Required evidence |
| --- | --- |
| Useful topic labels | At least 85% of emitted thread tags are independently judged accurate, sufficiently specific for locating related work, and supported by their cited original messages. |
| Multi-topic coverage | At least 80% of reviewed primary subjects and 60% of reviewed secondary subjects have an appropriate independently judged tag. Abstentions and missing tags count as misses. Different subjects cannot receive credit solely because an unrelated phrase occurs in their supporting message. |
| Evaluation scope | Frozen, previously unexposed original coding-chat threads, at least 20 threads/60 certain subjects, with at least 20 primary and 20 secondary subjects, across at least two supported providers. Annotate subjects before showing candidate outputs; blind candidate identity/order for tag review. Exact/near duplicates and development overlap must be disclosed and excluded by a frozen rule before scoring. No assistant/LLM-created gold. Report every thread, denominator, ambiguity and thread-level confidence intervals; point targets are engineering gates, not population guarantees. The existing exposed eight-thread review and prepared smaller review packet cannot by themselves meet this scope. |
| Correct lifecycle | Original provider/node/group ownership and source identities remain correct; duplicates do not create independent votes; edits/deletions remove stale evidence; idempotent updates and restart preserve the defined snapshot. Topic IDs must have a documented version/reconciliation contract; model component indices alone are insufficient. Run adversarial fixtures and actual persisted edit/delete/restart checks. No semantic hierarchy is required. |
| Ingestion cost | Matched complete initial/incremental ingestion with topics enabled versus disabled loses at most 10% throughput. Include source/context processing, inference if any, clustering, labels and persistence. Report CPU, RSS and p95 updates on supported node classes. Isolated Python refresh/fit times do not pass this gate. This retains the existing provisional target. |
| Search safety | Ordinary search remains available without topics. Enabling topic metadata cannot introduce automatic topic gates or discard direct candidates. Any future automatic topic-assisted ranking needs its own held-out relevance/no-regression gate; the prior failed guidance experiment is not a pass. |
| Executable reference | Frozen source/numeric/tie/abstention/lifecycle contract, immutable fixtures and exact reference replay. Dependencies/assets and tolerance boundaries recorded. Reference parity alone does not prove quality. |

If more than one candidate passes, prefer the simplest implementation and lower
complete-ingestion cost. Do not choose a marginal proxy improvement that misses
an acceptance gate. No hierarchy, LLM-generated labels, fine-tuning or unlimited
quality optimization is needed for this milestone.

## Stop optimizing after acceptance

Freeze the chosen method and implement it in Rust using PORT_CONTRACT.md. Ship
only after fixed-vector and complete-path differential CI, the same reviewed
quality targets and actual Rust ingestion performance pass. Further topic
algorithm tuning is backlog work triggered by a measured acceptance regression
or a new user requirement, not by an opportunity to improve a benchmark.

Current authoritative status: **no candidate has passed every gate**. Topic
quality review, actual full-ingestion performance and selected Rust parity remain
open. A finite failed decision is preferable to indefinite exploratory claims.

## Current-round decision ledger

| Candidate | Decision | Evidence / missing gates |
| --- | --- | --- |
| Fixed incremental NMF16/32, one/five passes | Reject this configuration; no parameter expansion | All tested reviewed primary/secondary distinct support F1 pairs are below source-aware phrases. Broad order ARI .585--.809; fixed feature/K, edits/deletes and generation identities unresolved. Exact repeat passes but independent semantic utility/performance/lifecycle/parity do not. |
| Source-aware phrase subjects | Retain for independent review, not production-selected | Primary/secondary distinct support F1 .735/.515 and diversified local evidence. Names still weak; exposed review cannot establish semantic acceptance. Full-ingestion, shared lifecycle/ID behavior and selected Rust parity remain open. |
| Whole-thread phrase weighting | Keep diagnostic baseline | Secondary support F1 .386 and high identical local evidence; no acceptance claim. |

Recorded experiment: `benchmarks/2026-10-02-incremental-nmf-topics.md`.
The finite method comparison is complete. No new method family is needed to
explain this outcome. Any permitted correction must name the retained candidate's
specific failure before execution and fit the single-correction/two-arm cap.
