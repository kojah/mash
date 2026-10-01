# Topic and episode annotation protocol

These guidelines govern the next development review. They do not retroactively
relabel the first four-thread boundary pilot, whose uncertainty and prior reviewer
exposure remain recorded. Version guidelines and source packets before prediction
or threshold selection. Keep development and reserved source groups separate.

## Unit and granularity

A useful episode is a contiguous, source-supported discussion of a concrete
subject/task that an agent could use to narrow a later thread/message search.
Do not treat “software”, a project name, sentiment, or an approval alone as a
sufficient episode subject. Distinct tasks within the same project can differ;
implementation, diagnosis and verification of one concrete issue can continue
the same episode. Record uncertainty where the granularity is genuinely ambiguous.

Review the complete relevant user/assistant context and native branch where
available. Preserve both the searchable projection and the canonical-source
identity. Tool results, injected instructions and assistant speculation are not
automatically user topics. A linear user-only projection is a separate study scope
and cannot establish branch-aware or full-dialogue judgments.

For each adjacent eligible user boundary, record `same`, `change` or `uncertain`
and a rationale with source evidence. Do not infer a topic change simply from
low cosine similarity, different wording, a long gap, or an ordinal gap containing
tool activity. Do not infer continuation from an approval phrase alone. Explicit
subject changes need evidence; referential replies need their actual referent.
Never silently skip nonempty unembedded turns in the reference sequence.

## Topics and context

Independently enumerate substantive subjects before viewing model labels. Mark
major and secondary subjects with supporting native turns/episodes. Preserve rare
technical identifiers and multi-subject threads. Record excluded framing and why
it is excluded. Do not create a universal task/domain label vocabulary from the
candidate clustering; this would make the evaluation circular.

For a short reply, distinguish self-contained content, resolvable reference and
unresolved reference. Record the minimal source context that resolves it, whether
that context precedes or follows the turn, and whether it introduces subjects the
user did not endorse. Native ancestry establishes provenance, not semantic
relevance. Following replies require an online publication/update policy.

For bounded preceding-reference reviews, preserve original admission and boundary
judgments and create a separate source-pinned sidecar. Classify each originally
referential anchor as `resolved-task`, `resolved-metadata`, `interaction-only`,
or `unresolved-reference`. A task resolution must identify an actual discussed
task; metadata resolution includes counts and session selectors, whose names do
not establish current substantive topics. Attention prompts need not inherit
the preceding assistant's subjects. Abstain when the specific referent is unclear,
even if the conversation's broad subject is known.

Pin exact anchor identity, context hash, source-review hash and minimal supporting
preceding record identities/hashes. Resolved cases require intact, unredacted,
displayed evidence on the captured causal path. Never infer assent to every
assistant proposal from a correction, question or selective approval. Record
known omissions, source limitations and reviewer exposure. These judgments are
evaluation labels, not an automatic resolver; rationales or reviewer-written
summaries must never be substituted for source text in model inputs.

Annotate semantic parent relationships separately from cluster topology. Check
that a parent expresses an actual broader subject of each child, rather than a
concatenation of child names or an incidental project/provider. Multiple leaf
subjects per thread are permitted; inherited ancestors should not count as
independent secondary-subject discoveries in recall.

## Review and reporting

Keep original reviewer judgments, uncertainty, evidence scope, annotator identity
and source checksums. Independent reviewers must not see model predictions during
initial annotation; retain disagreements before adjudication and report agreement.
Coding-assistant annotations are exploratory labels, not independent human ground
truth. Any known prior exposure to predictions must be disclosed.

Review model outputs only after reference annotations are frozen. Report primary
and secondary precision/recall, topic specificity, ancestor correctness, rare-topic
and provider breakdowns, abstention and source/context errors. For boundaries,
report precision and recall including abstentions as missed positive changes,
along with the excluded-uncertain count. Do not declare success from all-noise
output, high coverage, broad merging or coherence alone.

Freeze any parameter selection on development groups before reserved evaluation.
If reserved examples influence methodology, disclose that exposure and acquire an
independent test cohort; do not rename them “held-out” to hide tuning. Store private
sources and individual annotations outside Git; publish only reviewed artifacts.
