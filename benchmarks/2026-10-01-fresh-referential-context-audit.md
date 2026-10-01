# Fresh referential context and native identity audit

2026-10-01; `mash-c0d.19.2.20`. Source recovery before semantic reference review
or additional inference. Same v3 sampling cohort and origin review as the
[payload pilot](2026-10-01-feedback-payload-controls.md); no resampling.

## Procedure

`packet_contexts.py` validates the pinned packet/origin review, frozen generation,
partition and canonical development groups. Reconcile every source group's entire
visible user sequence, exact message IDs, ordinals, projections and source hashes
against the read-only frozen database. Select only the prior 21 referential origins;
do not use facets/names/rationales as model inputs. No model calls occur.

Reuse `preceding_context.preceding`: follow stored parent ancestry when present,
otherwise explicit stored linear order. Missing/mixed/invalid native edges never
fall back to linear order. Fixed limits remain three prior visible users, 64 total
predecessor records, 16 KiB per visible message and 32 KiB displayed per anchor.
Tool/developer records enter traversal provenance but not displayed task text.
Omit, never truncate, oversized display records; preserve redaction status.

For every selected anchor and displayed user/assistant record, audit current native
file identities, roles and canonical content using `native_audit.read_native/verify`.
Keep source-file/record hashes and explicit conflict/missing/unsupported statuses.
The native audit cap stays 64 MiB/file, 8 MiB/record and 100,000 records. Current
source bytes are reread to detect edit/append races. This does not independently
verify tool/developer bodies, complete native ordering or every causal edge;
`native_log_verified` remains false. A matching role does not establish human intent.

## Findings

Two fresh captures are byte-identical: SHA256
`19a1c0dcdd99768d2fb8b4d644adfd95906a738781d09b5983ad2c3a2af8e3be`,
724,515 bytes. Source bodies remain private.

| Quantity | Result |
| --- | ---: |
| Complete source groups validated | 4 |
| Selected referential anchors | 21 |
| Groups containing selected anchors | 2, both Codex |
| Unique anchor/displayed native identities | 119 |
| Exact identity/role/canonical-content matches | 119 |
| Intervals reaching root | 3 |
| Intervals hitting 64-record cap | 18 |
| Display omissions | 0 |
| Display size range per interval | 729–7,881 bytes |

Of the 18 capped intervals, three contain zero visible preceding users; eight
contain one and seven contain two. Many traversed assistant records have empty
search projections, and tool records also consume the physical-record budget.
Reaching a raw-record cap is distinct from exceeding display/model capacity.
These counts reveal a retrieval limitation, not a semantic-resolution failure rate:
nearby assistant replies can still provide a precise antecedent in incomplete
intervals. Conversely, a root-complete interval containing only injected protocol
does not establish a task referent.

No source-backed resolution judgments, context-selection scores or topic-quality
gain are claimed yet. Prior displayed previews are partial reading aids; frozen
evidence remains intact. The display guard is not a general secret scanner.
The cohort is exposed single-assistant development evidence, not independent human
annotation or held-out evaluation; provider coverage for references is Codex only.

## Validation and next experiment

130 lab tests pass. New tests reject changed IDs, omitted source users, duplicate
anchors and non-user anchors, and verify causal sibling exclusion. Existing tests
cover root/record limits, redaction/omission, native canonical identity mismatches,
parent errors, and unsupported content. Output and native source hashes repeat
exactly; the source database and native logs were not modified. No Rust changes.

Private captures: `topic-python-review-v3-20261001/referential-contexts-{first,second}.json`.
Invocation:
`packet_contexts.py --database ... --partition-report ... --packets ... --review ... --output ...`.

Next compare tool-aware causal predecessor metadata with the original raw-record
budget, preserving explicit total source-work and visible/model bounds. Freeze
new parameters before inference, retain this control and do not relabel an enlarged
scope as equivalent. Complete reference judgments with intact source evidence
before selection/clustering. Native order/branch completeness requires its own
evidence; source identity agreement is not sufficient. No topic method, semantic
tree, inherited tags, ingestion gate or Rust promotion follows from this audit.
