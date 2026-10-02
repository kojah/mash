# Declared semantic topic review scoring

`topic_quality.py` is a standard-library scorer for the finite targets in
`SELECTION_CRITERIA.md`. It **does not generate judgments** or declare semantic
gold, independence, source provenance or production selection verified.

Run `python3 research/topics/topic_quality.py --review PRIVATE/completed.json`.
Input must be complete known-schema JSON, bounded to16MiB, with no duplicate
keys/nonfinite values. Aggregated stdout does not expose reviewers, subjects,
tag IDs, quotes or source content. Invalid input returns an actionable static typed error
and exit1; valid scoring exits0 even if targets fail. Callers must inspect
individual metric/scope fields; exit0 means schema/scoring succeeded.

Output also includes `review_sha256`, computed from the actual submitted bytes,
so the reported counts can be traced to the exact review artifact. Source and
candidate pins remain declarations until their original files are audited.

The schema is:

```json
{
  "schema_version": 1,
  "scope": "topic-quality-declared-counts-v1",
  "packet_sha256": "64 lowercase hex characters",
  "candidate_sha256": "64 lowercase hex characters",
  "exposure": "declared-heldout-human",
  "reviewer": {
    "identity": "human reviewer identity",
    "human_attestation": true,
    "prior_source_exposure": false,
    "outputs_seen_before_subjects": false
  },
  "threads": [{
    "id": "original scoped thread ID",
    "provider": "codex",
    "source_ids": ["original-source-id"],
    "emitted_tag_ids": ["candidate-topic-id"],
    "tags": [{
      "id": "candidate-topic-id",
      "accurate": true,
      "specific": true,
      "source_supported": true,
      "evidence_source_ids": ["original-source-id"]
    }],
    "subjects": [{
      "id": "frozen reviewed subject ID",
      "importance": "primary",
      "uncertain": false,
      "evidence_source_ids": ["original-source-id"],
      "matched_tag_id": "candidate-topic-id"
    }]
  }]
}
```

This is an illustrative schema, not an actual completed review or passing
evaluation. Provider names use the13 serialized alternatives in app/provider.rs.
Other exposures are `unreviewed` and `exposed-development`; only the declared
held-out human mode with literal human/no-prior-exposure/no-output-before-subjects
declarations can be declared eligible. Self-reports do not verify independence.

Reviewers first annotate primary/secondary subjects and freeze that source-backed
annotation artifact **before** seeing candidate outputs. Then review blinded
candidate tags for accuracy, specificity sufficient to locate related work,
and supporting original source evidence. A useful tag needs all three judgments.
Keep every emitted tag, including rejected/ambiguous ones: tags cannot disappear
from the precision denominator. Unknown/missing/duplicate tag judgments fail
against the declared emitted roster. Scope original source IDs within each thread;
source-supported tags need nonempty evidence, and subject evidence is nonempty.

For certain subjects, `matched_tag_id` is a useful tag judged to represent that
subject, or null. Require a shared original source between subject and tag
evidence. That intersection proves only declared provenance compatibility, not
semantic equivalence. Require distinct subject-to-tag matches inside a thread:
one broad tag cannot receive credit for several separately annotated subjects.
This makes the earlier distinct-tag diagnostic operational for the acceptance
review. Uncertain subjects must be unmatched and are counted separately. Null
matches for certain subjects count as misses, not exclusions. Do not tune
discovery using these judgments.

Targets use exact fractions17/20 tag precision,4/5 primary coverage,3/5 secondary
coverage. Undefined ratios are null and fail. Scope requires20 threads **with
certain subjects**,60 certain subjects,20 primary/20 secondary and at least two
providers among those threads. Empty or uncertainty-only threads cannot pad the
quota; they remain in reporting and bootstrap resampling. Emit each numerator,
denominator, scope count and uncertainty count.

Confidence intervals use5000 seed718 cluster resamples of canonical-ID-ordered
threads with replacement, recomputing pooled ratios. Empirical lower/upper ranks
are floor((n-1)*.025)/ceil((n-1)*.975) of defined sorted resamples. Report valid
and total draws; do not replace zero-denominator ratios with0 or falsely report
an interval when none is defined. These intervals are descriptive and dependent
on the reviewed sample/annotation process, not population-quality guarantees.

`numeric_targets_met`, `minimum_scope_met`, `declared_review_eligible` and
`declared_quality_gate_met` are distinct. Even all true leave
`independence_verified`, `source_provenance_verified`, `semantic_quality_verified`
and `production_selected` false. A separate provenance audit must compare the
declared pins/rosters/providers/source IDs to the actual immutable source,
pre-candidate annotations and candidate capture, verify exact quotes where used,
reviewer chronology/blinding and held-out/exposure/duplicate rules. This scorer
does not authenticate metadata, inspect raw packet/candidate files, detect
semantic falsehood or prove review declarations. Existing `chat_review.py` checks
quote provenance for its development review scope; that scope cannot be renamed
held-out and used as independent gold.

Lifecycle/ownership, complete ingestion, ordinary search safety and selected
executable-reference/Rust parity remain separate mandatory gates. A valid or
numerically passing JSON file must never automatically choose the algorithm.
No real completed held-out topic review exists in this change. Unit fixtures
are explicitly synthetic and never populate private review packets.

Validation on2026-10-02: seven standard-library fixtures are included in CI;
all677 research tests pass in5.085s in the frozen science environment with author
reference paths supplied. Validation log:
`/home/james/scratch/mash-artifacts/topic-quality-hash-tests-20261002.log`.
These are parser/scoring tests, not actual acceptance judgments or performance
measurements of production ingestion.
