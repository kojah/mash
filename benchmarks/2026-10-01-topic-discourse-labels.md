# Label-only conversational masking experiment

This follows the support-aware label experiment and remains opt-in under
`MASH_TEST_TOPIC_LABEL_DISCOURSE=1`. It is not a production topic feature or a
default-policy change. The research plan is
[topic-quality-research](2026-10-01-topic-quality-research.md).

## Boundary and safeguards

The existing source-admission parser deliberately preserves unknown option
identifiers. These short replies can supply misleading word fragments to topic
labels: an acknowledgment selecting “(1), followed by (2)” is meaningful history,
but “followed” is not its subject.

The new projection changes only label token extraction. It recognizes complete,
short option acknowledgments and complete acknowledgment sentences, preserving
unknown subjects, incomplete lists and quoted literals. Removed sentences leave
a separator so phrases cannot join across them. It does not ban words such as
“Go” or “hello” globally. Unit checks preserve technical subjects, mixed requests,
numbers, malformed choices, function calls and paths/extensions.

Projection work is limited to the existing 4096-character label prefix; option
acknowledgments must be at most 256 characters. Quoted/backtick-containing text
is retained conservatively. The mode is serialized explicitly in both corpus
reports and candidate reviews. Source admission, canonical text/hash evidence,
document embeddings and assignment scoring are unchanged. Label word bags,
support and corpus c-TF-IDF values can change, including labels outside the
directly masked topic.

## Comparison protocol

Use the frozen 789-thread SQLite snapshot from the previous experiment and
installed local CPU models. Compare Supported-policy controls with masked
labels, keeping singleton label inference, visible-link extraction, context,
conserved support, exact routing, representatives and semantic ancestors fixed.
Run full and reserved partitions, forward and reverse insertion order, both
input modes and radii 0.4/0.6: 32 corpus reports. Eight focused candidate reviews
check probe states, support counts, score arithmetic and agreement with the
corresponding full benchmark.

Compare all 50 previously selected cases, retaining their existing bounded
source excerpts. This is a regression comparison, not a blinded or full-source
50-thread precision/recall review.

## Initial result

The first trial reproduced the previous Supported controls and preserved all
assignment evidence, direct memberships and ancestor memberships. The AGENTS
topic lost “sounds” while retaining “agents” and “update.” The option-selection
topic lost “followed,” “ahead” and “go,” but retained “sounds” and “good.” Seven
of the 50 selected cases changed; the inspected deadlock, memory and cBioPortal
labels were retained. This is a limited improvement, not a solved quality gate.

Further literal/path safeguards were added after this trial. Final validation
reran the complete protocol against the final source revision: 32 corpus reports
and eight focused reviews reproduced controls and preserved source evidence,
memberships and ancestry. Candidate score arithmetic, probe support and agreement
between focused/full labels passed. The seven changed cases and the inspected
label observations above were reproduced; named-leaf counts were unchanged in
all configurations. The mode therefore stays an experiment: reducing these
fragments does not establish broad topic relevance or justify changing defaults.

The final source passed 553 regular tests using Cranelift, Clippy across all
targets/features with warnings denied, the doctest, formatting and diff checks.
Private reports remain outside the repository under `mash-artifacts`; no raw
transcripts or private source excerpts are committed. Runtime measurements were
collected on a contended host and are not a speed claim.

The next representation experiments are bounded representative-segment scoring
and MMR, tracked in `mash-c0d.19.2.12`. Generic labels, secondary-subject recall,
semantic parent-child quality, source-independent support and production topic
lifecycle/MCP integration remain open.
