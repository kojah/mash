# Bounded phrase/dense corroboration control

Frozen before implementation/results, 2026-10-02; `mash-c0d.19.2.40`.
Two arms only: unchanged `SOURCE_COVERAGE` phrase discovery and the same proposals
with one dense corroboration rule. No new labels, parameter sweep, LLM, inference
or stoplist changes. Target failure: corpus recurrence of an ambiguous phrase
can label unrelated thread contexts as one subject.

For each literal phrase and thread, average unit-normalized existing message
vectors once per distinct exact input, then normalize that mean. Compare its
direction to the normalized mean of the other threads' directions containing
that phrase. Exclude the entire target thread, not merely the target message.
If the phrase has two threads and two distinct inputs, require cosine >= 0.55;
undefined/cancelled directions abstain. Otherwise retain the lexical proposal
as provisional without claiming semantic corroboration. The cutoff is the
existing geometry control's fixed value, not calibrated against these outputs.
Lexical score, order, tag cap, evidence boundaries and phrase IDs remain intact.
Do not refill removed tags or propagate nonliteral semantic matches.

The geometry ledger updates only affected phrase/thread buckets when a source
arrives, changes or disappears. Exact duplicates have one vote; conflicting
vectors for identical thread/input are rejected before changing state. Frozen
source order and binary64 sums have explicit lexicographic unit/thread ordering.
Restore recomputes the same buckets from a typed source ledger. Test idempotence,
edits, deletes, restart, new-topic discovery, cancelled/zero/nonfinite vectors,
own-thread exclusion and exact duplicates. Do not count a geometry-only online
component as proof the complete phrase pipeline meets the online gate: its
current lexical snapshots recompute corpus statistics, an unresolved limitation.

First run uses the existing exposed 128-thread/1,850-source capture, not new gold:
`topic-128-thread-first-20261002/checkpoint.json`, SHA256
`3ab25cafacab7ef1c6c0682189740aa23c620e4eab994db36c6d36f5e0d6d1f2`, and
`results.json`, SHA256
`8d06df371de287a4e12ab92376b276af111a63c7ffaeb58f447377afc51005aa`.
Verify represented text hashes against the captured provenance. This reuses
captured decoded embeddings; it does not independently replay native parsing or
the original database. Publish full source memberships, per-tag decisions and
counts; exact repeat excludes only runtime fields. Report CPU/RSS, geometry
update latency and complete prototype cost with clear timing boundaries.

No semantic precision/recall can be inferred from cosine or these unreviewed
threads. Independent primary/secondary quality review, full ingestion cost,
complete online publication and persisted production lifecycle remain required.
At the end record reject or retain-for-review; acceptance requires every gate in
SELECTION_CRITERIA. Do not add another variant to rescue unfavorable results.
