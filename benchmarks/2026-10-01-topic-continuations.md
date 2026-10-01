# Complete continuations in topic evidence

`mash-c0d.19.2.6`, following the [exact-routing experiment](2026-10-01-topic-routing.md).
Rust test-only topic prototype; production indexing, search, CLI and MCP are unchanged.

## Source findings and narrow change

Read complete nonempty stored searchable user prose for 11 of the fixed 50 selected
cases. Assistant context was sampled or targeted, except one short pipeline case
where all searchable assistant replies were read. This is a nonblind coding-agent
review of searchable requests, not a completed full canonical transcript review
or a 50-case accuracy measurement. The other 39 cases remain pending in this batch.

Large Claude user-role histories include tool-result envelopes. The current parser
preserves the provider role and canonical content while excluding tool payloads
from searchable projections, including mixed text/tool records. Existing parsing
tests cover this. Empty projections are not evidence of missing human requests,
and stored role checks alone do not prove upstream record classification.

The review confirms complete deictic continuations becoming topic evidence. Extend
the bounded complete-phrase recognizer for observed forms such as “let's do it
then,” “let's tackle those next items then,” and “actually nvm just fix em.” Every
token must still match a known phrase. Named technical subjects, trailing requests,
option selections and unknown wording are retained. Preserve standalone `NVM`
because it can name a technical tool.

For complete-continuation recognition only, interpret six known HTML space
entities, including the observed literal `&#x20;`. Do not rewrite topic evidence,
canonical history or stored search vectors, decode arbitrary HTML, or reinterpret
unknown/malformed entities. The existing 256-character bound remains.

An independent reconstruction finds 37 newly ignored unique embedded searchable
messages across 20 threads: 30 Codex and 7 Claude Code records. All 30 distinct
complete text forms were inspected; none names a new technical subject. This
does not resolve acknowledgments attached to substantive requests or option
selections whose meaning requires conversational context.

## Fixed-corpus comparison

Same 789-thread snapshot, compatible local model, canonical reserved partition,
input cleanup, semantic labels/hierarchy, exact routing, both insertion orders,
and all representative/anchored configurations at radii 0.4 and 0.6. Independent
audits cover all 16 reports. Primary representative radius 0.4:

| Metric | Before: full | After: full | Before: reserved model | After: reserved model |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 769/778 | 769/778 |
| Reserved assigned / eligible | — | — | 136/145 | 136/145 |
| P95 tags, forward / reverse | 5 / 5 | 5 / 4 | 5 / 5 | 5 / 5 |
| Maximum tags, forward / reverse | 39 / 36 | 46 / 52 | 43 / 44 | 44 / 45 |
| Active leaves, forward / reverse | 409 / 408 | 410 / 410 | 373 / 370 | 374 / 371 |
| All-instance pair stability | 0.950 | 0.954 | 0.952 | 0.952 |
| Canonical-copy collapsed stability | 0.915 | 0.921 | 0.918 | 0.917 |
| Reserved-only canonical stability | — | — | 0.877 | 0.877 |
| Older raw-vector collapsed stability | 0.826 | 0.833 | 0.828 | 0.828 |

Assigned thread sets, eligibility and canonical source identities remain unchanged
in all four primary runs. Stability still exceeds the proposed 0.8 threshold in
both raw-vector scopes, but maximum overlap worsens. These are structural measures,
not relevance precision or recall.

Source-backed checks show the unwanted encoded-space/acknowledgment topic replaced
in the planning example and a previously missed race-issue subject recovered in
another thread. The planning example also loses a distinct quality-comparison
label and acquires weak hold/recommendation labels. Other inspected cases retain
path-heavy names, generic acknowledgments attached to requests and duplicate
subjects. A short “this pipeline” request still lacks the concrete deadlock
discovery/confirmation subject established by its assistant replies.

Do not promote the topic prototype on these results. Follow-ups
`mash-c0d.19.2.7` and `.19.2.8` cover bounded contextual subject recovery and noisy
overlap/secondary-subject loss. Full-source relevance review `.19.2.2` remains open.

## Validation and cost

528 regular tests pass with 107 skipped; 57 focused topic tests pass. Preservation
cases include technical subject tails, named requests, option identifiers, unknown
and malformed entities, and standalone `NVM`. All-target/all-feature Clippy,
doctests, formatting and diff checks pass.

Optimized final diagnostics take 17.90 seconds full and 18.06 seconds reserved for
eight configurations each. Representative fit is 440/386 ms full and 396/409 ms
reserved; assignment is 703/689 ms full and 650/805 ms reserved, forward/reverse.
Peak RSS is 1,123,944 KiB full and 1,115,236 KiB reserved, about 1.07/1.06 GiB.
These are single-machine diagnostic measurements, not production indexing,
streaming overhead, search latency or Mac guarantees; they do not establish a
speedup over the preceding runs.

Independent audits verify unchanged canonical partition/eligibility, exact distinct
thread/provider counts, consistent acyclic forest relationships and deduplicated
ancestors. All 1,892 source references per report resolve against read-only SQLite
with matching ownership, ordinal and canonical BLAKE3 fingerprints. The 138 partial
excerpts remain partial. Input rewriting/embedding counts are unchanged.

Private histories, judgments, reports and audits live outside Git under
`mash-artifacts/topic-continuations-20261001`. The initial pre-`NVM` diagnostic is
separate; final results use `exact-full` and `exact-held-out`. Python audits artifacts;
topic processing remains Rust. No source repair, reindex or readiness claim was made.

The narrow continuation fix is complete. Overall topic quality, stable per-index
model namespaces, lifecycle/persistence, scale bounds and scoped MCP tree tools,
thread tags and topic filters remain unfinished.
