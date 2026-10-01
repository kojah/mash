# Instruction and pasted-content topic framing

`mash-c0d.19.2.3`, following the
[grounded assignment diagnostic](2026-10-01-topic-evidence.md). Rust test-only
topic projection; production ingestion, search, CLI and MCP are unchanged.

## Source-backed changes

Supporting-message inspection found a direct Git/codebase-policy tag on an
analyzer thread from a generated `# AGENTS.md instructions` envelope. The existing
projector recognized only the heading with `for /path`. It also left generated
environment metadata after removing the instruction body. Pasted-content framing
was another unrecognized shape, contributing attachment paths and protocol text.

The frozen canonical corpus has 65 unique instruction envelopes starting with
that heading and two unique pasted-content envelopes. Many canonical records do
not contribute compatible embedded topic evidence; these counts are not the
number of newly omitted inputs.

Move framing recognition into a small module and add the exact plain heading.
Require complete, unnested instruction tags. After them, recognize only the
observed complete environment schema: optional absolute Unix cwd and known shell,
date/timezone fields, bounded absolute workspace roots, and the exact observed
disabled/unrestricted filesystem profile. Remove that generated prefix while
retaining any trailing request. Unknown fields, attributes, permissions, path
formats, duplicated tags and incomplete context remain evidence. This is not a
general XML parser or a claim to support all environment variants.

Recognize the observed pasted-file envelope with quoted descriptive titles and
absolute paths. Keep those descriptions and the human request; remove the
generated path and headings. Descriptions can carry actual subject information
and must not be discarded with metadata. Existing mentioned-file/image behavior
shares this bounded parser, with stricter validation of unknown paths. Nonempty
image text remains evidence.

Framing recognition is capped at 64 KiB and 32 attachment/workspace entries.
Quoted, nested, malformed, unknown and oversized shapes are preserved. Changed
text receives fresh vectors from the installed compatible local model. Canonical
history, stored search vectors, canonical source fingerprints and partition IDs
are untouched.

## Controlled comparison

Same frozen 789-thread Claude Code/Codex corpus, model generation, both insertion
orders and radii, representative/anchored modes and canonical reserved split.
All 16 reports retain the same source identities and eligibility denominators:
778 eligible threads and 145 eligible reserved threads. The table shows
representative discovery at radius 0.4; naming counts describe generated names,
not measured usefulness.

| Metric | Before: full | Framing: full | Before: reserved run | Framing: reserved run |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 770/778 | 769/778 |
| Reserved assigned / eligible, both orders | — | — | 137/145 | 136/145 |
| P95 direct tags, forward / reverse | 5 / 5 | 5 / 5 | 5 / 5 | 5 / 5 |
| Maximum tags, forward / reverse | 42 / 40 | 40 / 39 | 46 / 41 | 43 / 40 |
| Active leaves, forward / reverse | 429 / 430 | 422 / 429 | 373 / 386 | 378 / 375 |
| Named leaves, forward / reverse | 428 / 429 | 421 / 428 | 372 / 384 | 377 / 373 |
| Parent groups, all named, forward / reverse | 116 / 110 | 104 / 110 | 90 / 95 | 97 / 92 |
| All-instance pair stability | 0.938 | 0.936 | 0.936 | 0.955 |
| Canonical-copy collapsed stability | 0.897 | 0.888 | 0.894 | 0.922 |
| Reserved-only canonical stability | — | — | 0.855 | 0.931 |
| Older raw-vector collapsed stability | 0.793 | 0.782 | 0.796 | 0.841 |

The inspected analyzer thread loses its generated Git-policy tag. The pasted
description still contributes its analyzer subject and joins related evidence;
truncated-title fragments can still become labels. Context-only acknowledgments,
generic UI framing, duplicate labels and weak leaves remain.

One reserved thread loses its previous only tag. Source inspection shows real
questions about an AEO consultancy, business models, crawling and a website; the
old tag instead described codebase/Git instructions in generated context. This
removes an unrelated match but does not recover the thread's real subjects. Do
not interpret either assignment coverage or that one observation as whole-corpus
precision/recall. Full-source review of the 50 selected cases remains unfinished.

Reserved stability improves, while full-corpus stability declines. The older
raw-vector full measure still fails the proposed 0.8 gate. These measures have
different populations and must all remain visible; the more favorable reserved
metric does not establish adoption readiness.

## Work, resources and validation

The input pass omits 85 records, projects 140, deduplicates 15, and embeds 136
unique replacements, versus 78/136/15/134 before this change. Whole eight-run
diagnostics take 21.29 seconds for the full corpus and 22.40 seconds for the
reserved model. Reserved peak RSS is 1,139,368 KiB (about 1.09 GiB), including
models, retained corpus, caches, topic evidence and report writes. These single
Xenia CPU runs do not measure production indexing overhead or Mac throughput.

521 regular tests pass with 107 skipped; 50 focused topic tests pass. Optimized
full/reserved diagnostics, all-target/all-feature Clippy, doctests, formatting
and diff checks pass. Tests preserve unknown context, trailing real requests,
descriptive titles, canonical text/vector behavior and malformed/quoted/nested/
oversized inputs.

Independent audits of all 16 reports verify source identities/split/eligibility,
exact distinct-thread/provider counts, acyclic forest relationships, and deduplicated
semantic ancestors. All 1,911 captured user-message references resolve against
read-only SQLite with matching ownership, ordinal and canonical BLAKE3 fingerprint.
There are 138 incomplete excerpts. Private artifacts and judgments stay outside
Git in `mash-artifacts/topic-framing-20261001`; no real IDs or raw histories are
published. Python audits the artifacts; runtime prototype work remains Rust.

The targeted framing bead is complete. Topic relevance, unknown-subject coverage,
order stability, stable namespaces/lifecycle/persistence and scope-safe MCP
navigation/tags/filters remain open in the parent issues.
