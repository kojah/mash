# Subjects before UI annotation metadata

`mash-c0d.19.2.4`, following the
[instruction/attachment framing comparison](2026-10-01-topic-framing.md).
Rust test-only topic projection; no production indexing, search, CLI or MCP changes.

## Source evidence and projection

Grounded supporting-message inspection showed generic “submitted feedback” tags,
viewport sizes and CSS paths competing with actual requested changes. The frozen
canonical snapshot has 32 unique UI-worker feedback payloads: 30 page-annotation
batches, one smoke test and one differently structured page-feedback request.

Add typed borrowed `Batch` and `Annotation` models for the observed complete
page-feedback shape. Validate the page path, numeric viewport, sequential numbered
annotations, element descriptions, known selector syntax and nonempty feedback.
Put each requested change first, followed by its element description; retain the
page path once. Only the validated viewport/selector fields and protocol framing
are omitted from this derived topic text. Canonical sources remain untouched.

Recognition is bounded at 64 KiB and 32 annotations. Preserve multiline comments
and all annotations. Unknown fields, selectors, empty/malformed/incomplete batches,
ambiguous Markdown and oversized inputs fall back to the entire payload, rather
than partial extraction or silent truncation. Keep unknown worker instructions
and trailing requests using the existing conservative routing handling.

For an exact known worker wrapper, remove the generated `UI feedback` prefix and
submission tags regardless of whether the body matches the page schema. Unknown
payload bodies remain intact; the smoke-test heading, for example, is meaningful
source text. Real messages discussing UI feedback tools are unaffected. Every
changed topic input receives a fresh vector from the compatible installed local
model; no LLM or download is involved.

In the inspected mixed UI thread, tags now describe FAQ wording, codebase layout,
a broken table, the fact model and a pipeline smoke test. Previously most names
repeated “page feedback,” “UI feedback” or “submitted feedback.” Independent
reconstruction verifies exact derived fingerprints for eight structured payloads
in the selected review set, preserving the full feedback text, element descriptions,
page context and tails for all 15 annotations. This is a targeted source-backed
observation, not completed whole-thread relevance scoring across all 50 cases.

## Fixed-corpus comparison

Same 789-thread Claude Code/Codex snapshot, compatible local model, canonical
reserved split, both insertion orders and radii, representative/anchored modes.
All 16 reports keep canonical IDs, partitions and eligibility denominators fixed:
778 eligible threads and 145 eligible reserved threads. Representative discovery
at radius 0.4:

| Metric | Before: full | UI subjects: full | Before: reserved run | UI subjects: reserved run |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 769/778 | 769/778 |
| Reserved assigned / eligible, both orders | — | — | 136/145 | 136/145 |
| P95 direct tags, both orders | 5 | 5 | 5 | 5 |
| Maximum tags, forward / reverse | 40 / 39 | 44 / 39 | 43 / 40 | 45 / 40 |
| Active leaves, forward / reverse | 422 / 429 | 431 / 430 | 378 / 375 | 376 / 378 |
| Named leaves, forward / reverse | 421 / 428 | 430 / 429 | 377 / 373 | 375 / 376 |
| Parent groups, all named, forward / reverse | 104 / 110 | 107 / 107 | 97 / 92 | 88 / 91 |
| All-instance pair stability | 0.936 | 0.935 | 0.955 | 0.941 |
| Canonical-copy collapsed stability | 0.888 | 0.888 | 0.922 | 0.899 |
| Reserved-only canonical stability | — | — | 0.931 | 0.833 |
| Older raw-vector collapsed stability | 0.782 | 0.779 | 0.841 | 0.800 |

The inspected UI subjects improve, but the clustering tradeoff is mixed: coverage
is unchanged, maximum tag counts increase, and reserved stability decreases.
Full raw-vector stability still fails the proposed 0.8 gate; the reserved measure
barely meets it at 0.800244. Names and coverage are not relevance accuracy. Keep
all measures visible and do not promote production topics from this comparison.

## Resources, verification and next work

Input counts are unchanged at 85 omissions, 140 projected records, 15 deduplicated
records and 136 unique fresh embeddings. Full eight-run diagnostics take 19.71
seconds; reserved diagnostics take 18.35 seconds including process startup, versus
21.29/22.40 seconds in the preceding single runs. Reserved peak RSS is 1,128,880
KiB (about 1.08 GiB). These Xenia CPU runs include retained sources, model sessions,
candidate caches, hierarchy, evidence and report writes. They are not repeated
performance trials, production indexing-overhead bounds or Mac measurements.

524 regular tests pass with 107 skipped; 53 focused topic tests pass. Optimized
full/reserved real-model diagnostics, all-target/all-feature Clippy, doctests,
formatting and diff checks pass. Tests exercise full feedback/context preservation,
multiline/multiple annotations, unknown metadata, malformed dimensions, quoted/
incomplete input and oversized batches.

Independent 16-report audits verify exact direct/inherited provider and distinct
thread counts, acyclic forest relationships, ancestor deduplication and unchanged
canonical identity/split/eligibility. All 1,911 captured sources resolve against
read-only SQLite with matching ownership, ordinal and canonical BLAKE3 fingerprint.
There are still 138 partial excerpts. Private source packets and analyses remain
outside Git in `mash-artifacts/topic-ui-subjects-20261001`; no real IDs or histories
are published. Python only audits artifacts; the prototype is Rust.

The targeted UI projection is complete. Next investigate discovery-order/routing
sensitivity, then complete grounded full-thread relevance and overlap review.
Context-dependent acknowledgments, truncated terms and other generic/duplicate
tags remain defects. Stable namespaces, edit/delete/restart persistence and scoped
MCP tree exploration, thread tags and topic filters remain unfinished.
