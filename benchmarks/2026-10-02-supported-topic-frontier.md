# Supported/cohesive consolidation does not resolve topic fragmentation

Historical control: the frontier prototype, artifact auditor and their test
were retired after the hierarchy requirement was dropped and the user requested
removal of unnecessary machinery. Their source remains reproducible by checking
out commit `8304e2d`; the captures, negative results and manuscript evidence are
retained. This report does not authorize more hierarchy experiments.

This experiment reuses the frozen deferred prototype's 149 sources, including
74 stored embeddings and 75 absent vectors. All 75 have explicit empty search
projections, so admitted-text coverage is complete (74/74); they must not be
treated as unfinished embeddings. See the [projection audit](2026-10-02-projection-coverage.md).
It changes no source units,
embeddings, routing, hierarchy or labels, and makes no model calls. Production
is unchanged. This is exploratory development, not held-out quality evidence.

`topic_frontier.py` scores every existing node and selects the deepest eligible
nodes with no eligible descendants. The selected nodes form a tree antichain,
but their source sets can overlap because base memberships are multi-topic.
Eligibility requires at least two distinct sources and two existing supported
label terms. A paired arm additionally requires **every** member's cosine with
the centroid of its other members to reach the original routing threshold .55.
Cancellation gives score zero; singletons cannot qualify. No threshold grid,
fallback parent assignment, model refit or source filtering was used.
Unsupported residual branches abstain; all original sources remain in output.
Thread direct tags and deduplicated structural ancestors are recomputed.

| Arm | Forward topics | Forward covered vectors | Reverse covered vectors | Order pair Jaccard |
| --- | ---: | ---: | ---: | ---: |
| Original leaves | 57 | 74/74 | 74/74 | .710526 |
| Label support only | 16 | 42/74 (56.76%) | 43/74 (58.11%) | .769231 |
| Label support + cohesion | 10 | 26/74 (35.14%) | 27/74 (36.49%) | .675676 |

The reverse arms selected 17 and 11 topics respectively. Each candidate
retained one multi-topic source. Coverage-set Jaccard was .976744/.962963;
this indicates stable abstention sets, not stable topic assignments or good
coverage. Pair Jaccard includes all retained co-membership pairs; fewer sources
and changing pair universes make it inappropriate to claim a quality gain from
the label-only increase. Filtering exposes unsupported content rather than
repairing it. The cohesion arm is therefore rejected as the main correction
for this prototype; neither arm is selected for production.

Two sequential runs matched all non-runtime outputs. Four fixtures verify
deep supported selection, ancestor inheritance, cross-topic centroid rejection,
explicit uncovered residuals and missing geometry rejection. The research suite
passed 624 tests in 4.717 seconds. `topic_frontier_audit.py` imports no producer
code: independent `math.fsum` leave-one-out geometry, eligibility, exhaustive
descendant frontier and memberships checked 452 node/arm/order combinations.
Maximum scalar-versus-NumPy score difference was 1.11e-16; the closest score
to .55 remained .009648 away. This validates the recorded threshold decisions
here, not universal numerical parity, original routing, labels or semantics.

Both-order computation took 23.86/30.97 ms for label-only and 25.26/35.71 ms
for cohesion. External smaps target RSS maxima were 43,140/43,192 KiB and PSS
35,567/35,786 KiB (11/13 samples); approximate status HWM 45,096/44,960 KiB.
These short target processes include imports, loading and both arms; samples
are lower bounds, observer/children memory excluded and monitoring intrusive.
They are not complete-ingestion or continuous online performance evidence.

Private artifacts: `topic-frontier-first-20261002.json`,
`topic-frontier-repeat-20261002.json`, matching `*-memory-*` sidecars,
and `topic-frontier-audit-20261002.json`, all under mash-artifacts.
The baseline SHA and method SHA are pinned in both results. Existing labels and
private source identities remain outside Git. Next address source-backed topical
unit formation and cluster refinement rather than hiding poor leaves with a
hard support/cohesion gate. Independent coding-session judgments, full-ingestion
overhead and selected-method Rust parity remain open.
