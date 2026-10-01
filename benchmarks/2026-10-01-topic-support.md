# Conserved support for overlapping topics

Work on `mash-c0d.19.2.8`, following the
[continuation cleanup](2026-10-01-topic-continuations.md). Rust test-only experiment;
production indexing, search, CLI and MCP are unchanged.

## Evidence and method

Capture the fixed 50 review cases plus three additional worst-overlap threads,
within the existing 64-thread evidence limit. The busiest reverse-order example
has 52 direct tags. A single pasted lifecycle proposal supplies full independent
support to 12 topics; two other requests supply support to eight each.

Read those three complete searchable source requests: loop-resource settlement
and iterator contracts, a CI/false-positive question, and Lockbud/lock-order
improvements. These contain substantive technical subjects. Dropping pasted text
or imposing a tag-count cap would discard useful evidence. This is a targeted,
nonblind coding-agent review, not a completed full-thread precision/recall audit.

Add typed `Independent` and `Conserved` assignment weighting. For accepted matches
with positive cosine scores, conserved support is:

```text
weight = best_cosine / sum(accepted_cosines)
topic_support += segment_messages * topic_cosine * weight
```

Each segment distributes at most `segment_messages * best_cosine` confidence mass
across its matches, subject to floating-point rounding. A single match keeps its
previous support. Genuine overlapping matches remain possible; the existing
relative thread-support threshold is unchanged. Discovery, centroid matching and
canonical partition are unchanged. Labeling and semantic ancestor algorithms are
unchanged; retained label contributions and inherited memberships can change.

The ignored diagnostic opts in with `MASH_TEST_TOPIC_CONSERVED_SUPPORT=1` for
segmented modes. Unset it for the independent control; presence, including `=0`,
selects the experiment. Both variants use exact nearest-leaf routing. Reports
serialize the weighting enum. Private evidence schema version 2 records each
score's weight, making weighted support reproducible without changing source
identity or excerpt-completeness semantics. There is no production setting.

Separate discovery fitting from assignment/report construction into its own module.
Controls verify that this refactor preserves all mathematical outputs.

## Fixed-corpus comparison

Same 789-thread snapshot and local embedding generation, all 16 configurations per
variant, both insertion orders and radii, representative/anchored modes, and the
fixed canonical reserved partition. Primary representative radius 0.4:

| Metric | Independent: full | Conserved: full | Independent: reserved model | Conserved: reserved model |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 769/778 | 769/778 |
| Reserved assigned / eligible | — | — | 136/145 | 136/145 |
| P95 tags, forward / reverse | 5 / 4 | 4 / 4 | 5 / 5 | 5 / 5 |
| Maximum tags, forward / reverse | 46 / 52 | 34 / 32 | 44 / 45 | 31 / 28 |
| All-instance pair stability | 0.954 | 0.959 | 0.952 | 0.954 |
| Canonical-copy collapsed stability | 0.921 | 0.927 | 0.917 | 0.921 |
| Reserved-only canonical stability | — | — | 0.877 | 0.876 |
| Older raw-vector collapsed stability | 0.833 | 0.841 | 0.8284 | 0.8282 |

Assigned thread sets and discovery counts remain unchanged. Full runs remove
94/88 memberships from 14 threads in each order; reserved-model runs remove
78/79 from 11/12 threads. No memberships are added in these four runs. That is an
observed result, not a general monotonicity guarantee for relative support pruning.

Of the 12 tags whose captured samples include the loop-lifecycle proposal, three
remain on that thread, including a broad lifecycle subject. The inspected UI,
race/tracing, streaming-workflow and shared-core subjects remain represented.
Fine-grained lifecycle names, generic acknowledgment/option labels, the missing
planning-pack quality-comparison label and the context-dependent pipeline domain
still need work. Fewer tags and structural stability do not prove relevance.

Prefer this opt-in candidate for the next quality review, while retaining the
independent control. The broader bead remains in progress: full-source accuracy,
useful secondary-subject recall and bounded contextual subject recovery are unproven.

## Cost and validation

530 regular tests pass with 107 skipped, including 59 topic tests. New fixtures
cover ambiguous many-way matches without losing a genuine secondary subject, and
a single request matching two subjects with conserved confidence mass. Captured
weights retain explicit source alignment. All-target/all-feature Clippy, doctests,
formatting and diff checks pass.

Eight-configuration diagnostic times on Xenia:

| Pair | Independent | Conserved |
| --- | ---: | ---: |
| First full pair, control first | 18.17 s | 24.49 s |
| Repeated full pair, candidate first | 24.32 s | 25.64 s |
| Reserved-model pair | 22.18 s | 22.60 s |

Unchanged input preparation and discovery also vary substantially between runs.
The repeat observes about 5.4% whole-diagnostic overhead, versus about 35% in the
first full pair; reserved overhead is about 1.9%. Keep both results visible. These
few measurements do not establish production indexing overhead or search latency.
Peak RSS is about 1.08–1.09 GiB in all pairs. Weight calculation adds two linear
passes over accepted matches and allocates no vectors; changed label contributions
can change phrase-embedding work.

Independent audits reproduce all 18 control reports/input summaries against the
preceding baseline, excluding timing and new diagnostic metadata. The reversed-order
repeat reproduces all 18 full artifacts exactly except timing, including evidence.
Audits cover direct/inherited distinct thread/provider counts, forest consistency,
canonical identity/partition/eligibility and score calculations for all 32 primary
reports. All 2,933 captured source references per report resolve to canonical
ownership, ordinals and BLAKE3 fingerprints. The 187 partial excerpts remain partial.
Selected-thread retained confidence mass is bounded by its source-message count
in all four primary conserved runs.

Private reports and audits live in `mash-artifacts/topic-overlap-20261001`; no real
IDs or histories enter Git. Python audits artifacts; runtime topic processing is
Rust. Overall quality review, stable per-index model namespaces, lifecycle and
persistence, scale bounds, and scoped MCP tree tools/thread tags/filters remain open.
