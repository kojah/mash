# Reviewed startup context: source-pinned ablation

The skill-document experiment left workflow topics on a fully reviewed,
context-only thread because its startup/persona prompt is recorded as a user
message. User-role text and a recognizable prefix are not reliable evidence
that a message was generated. Genuine requests can discuss or quote the same
agent workflows.

Add an offline experiment driven by an explicitly reviewed TOML manifest,
selected with `MASH_TEST_TOPIC_STARTUP_CONTEXT=/absolute/path/selection.toml`
and `MASH_TEST_TOPIC_CLEAN_INPUT=1`. Its absence preserves all startup records.
This is a source-pinned ablation, not an automatic startup classifier or a
production configuration change.

```toml
schema_version = 1

[[sources]]
thread = "reviewed-thread-id"
provider = "codex"
message_id = "reviewed-message-id"
canonical_blake3 = "<64 hexadecimal characters from the complete canonical text>"
category = "generated_startup"
review = "Describe the full-source review establishing generated context."
```

The example hash is a placeholder. A manifest must supply a valid BLAKE3 hash.
The typed parser rejects unknown fields/categories/providers, unsupported
schema versions, duplicates, malformed hashes, empty identifiers or review
notes, oversized identifiers/notes, more than 1,024 sources and files exceeding
262,144 bytes. The input summary records the manifest fingerprint, reviewed
source count and omitted-startup count without exposing paths or source text.

Every selected source must exist exactly once in the retained embedded user
evidence for the specified thread/provider/message identity. Its origin and
text kind must be stored canonical content, and both its canonical hash and
complete current text must match the annotation. Missing, changed or projected
sources fail before derived evidence is changed. Synthetic evidence cannot
match. This validates annotation identity, not the correctness of the review
judgment; that still requires inspecting the source.

Canonical history, stored search vectors and canonical source-group identities
remain intact. Only derived topic input is excluded. Genuine discussions,
quoted material, unknown sources and unannotated records remain. Appending a
request to an annotated record changes its hash and fails validation. Tests
verify later projected requests retain their correct source/vector positions.

The manual ignored Rust test
`topics::evaluation::input::startup::inventory::benchmark_reviewed_startup_inventory`
compares skill exclusion alone with skill exclusion plus the reviewed manifest.
It requires explicit `MASH_TEST_SEARCH_DB`, `MASH_TEST_TOPIC_REPORT_DIR` and
`MASH_TEST_TOPIC_STARTUP_CONTEXT`. It reads the corpus without model loading or
downloads and records newly empty inputs and unchanged replacement texts.

On the frozen 789-thread corpus, a manifest containing the startup records of
fully reviewed cases 25 and 43 removes two derived sources. One context-only
thread becomes empty; the other retains its actual Dolt-maintenance request.
Replacement-text inventories are identical. The private manifest, hashes and
inventory live under
`/home/james/scratch/mash-artifacts/topic-reviewed-startup-20261001`.
This deliberately small, reviewed cohort does not establish automatic detection
precision or corpus-wide primary/secondary-topic recall.

## Controlled clustering results

The release trial completed 64 topic reports and 16 focused candidate reviews:
skill exclusion alone / plus reviewed startup exclusion, BIRCH/GRINCH,
full/reserved-source discovery, representative/anchored inputs, radii 0.4/0.6
and both insertion orders. Embedding assets, label scoring, discourse masking,
exact routing, weighting and hierarchy construction stayed fixed. Representative
label scoring remained disabled. No models were downloaded.

Controls reproduced the previous skill-exclusion experiment, apart from timing
and the new startup metadata. The audits verified identical discovery inputs
between algorithms within each policy, canonical identities and generation,
direct/inherited thread/provider counts, semantic forest reciprocity and cycles,
GRINCH tree/cut coverage, focused candidate scores and exact surviving source
references. Snapshot audits check structure, not independently recomputed
centroid geometry; the Rust invariant tests cover the tree calculations.

Both algorithms leave case 25 unassigned in all 32 reviewed-policy configurations.
Its canonical identity remains in the report; eligible thread count falls from
778 to 777 because its derived input is genuinely empty. Case 43 retains exactly
its canonical maintenance request, with no startup or skill source references.
Replacement-embedding and context-enrichment summaries remain unchanged.
Full representative discovery uses 705 vectors instead of 706.

For case 43, the following counts combine two radii, two orders and two discovery
partitions, giving eight configurations per row:

| Algorithm / discovery | Control: tagged / maintenance named | Reviewed: tagged / maintenance named |
| --- | --- | --- |
| BIRCH / representative | 8 / 8 | 8 / 8 |
| GRINCH / representative | 8 / 8 | 8 / 8 |
| BIRCH / anchored | 8 / 4 | 6 / 6 |
| GRINCH / anchored | 8 / 4 | 4 / 4 |

“Maintenance named” checks that an assigned semantic label literally includes
both Dolt and maintenance. It measures label visibility for this reviewed
request, not semantic recall across a corpus. Representative discovery keeps
the maintenance topic while removing startup-driven assignments. Anchored
discovery still leaves the request unassigned in BIRCH's forward radius-0.6
runs and GRINCH's radius-0.6 runs in both orders. Its control assignments in
those configurations had generic workflow labels, so assignment coverage by
itself would have hidden poor subject representation.

The fixed 50-case bounded-source comparison changes labels or assignments in
eight cases for BIRCH and five for GRINCH, including term reordering and the
intentional empty-context case. This is not a corpus-wide quality score. The
existing 14-case review selection remains fixed; only the two explicitly
reviewed startup records are excluded.

Representative full-corpus forward/reverse canonical co-assignment Jaccard
at radius 0.4 changes from 0.8944 to 0.8777 for BIRCH and remains approximately
0.5654 for GRINCH. Reserved-source-only values change from 0.8576 to 0.8576
and 0.9114 to 0.9091, respectively. The canonical cohort is retained even when
derived inputs become empty. Stability remains a diagnostic rather than a
topic-quality measure. Reserved sources were excluded from discovery, but
label statistics still use assigned threads; this is not a fully held-out
label-quality experiment.

Representative full fit ranges on the shared host were 358–439 ms / 400–566 ms
for BIRCH control/reviewed and 1,425–1,988 ms / 1,483–2,921 ms for GRINCH.
These exclude model work, assignment and labels; neither end-to-end latency
nor a speedup is established.

Private reports, controls, audits and source-intent comparisons are under
`/home/james/scratch/mash-artifacts/topic-reviewed-startup-clusters-20261001`.
Keep this opt-in. Broader source-origin handling, independent primary/secondary
review and rare-subject assignment remain open; neither clustering algorithm
is promoted. Bead `mash-c0d.19.2.15` remains in progress.

Validation: 95 focused topic tests, 566 regular tests, the ignored read-only
inventory and all eight release corpus/candidate benchmark pairs passed.
Clippy with warnings denied, doctests, formatting and diff checks passed.
