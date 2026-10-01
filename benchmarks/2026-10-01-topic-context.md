# Grounded context for vague topic requests

Test-only experiment for `mash-c0d.19.2.7`, following
[conserved overlap support](2026-10-01-topic-support.md). Topic and label quality
is the priority before production MCP integration. This experiment is not a
production indexing setting.

## Method and boundaries

A reviewed thread asks for an opinion on "this pipeline". Its searchable replies
explain deadlock discovery, runtime confirmation and recall measurement, but
user-only discovery labels it generically as pipeline/current state.

The opt-in `MASH_TEST_TOPIC_CONTEXT=1` stage runs after input cleaning. Presence,
including `=0`, enables it; unset it for the control. It recognizes only short
deictic requests containing a conservative generic vocabulary. Named subjects,
paths, unfamiliar words and numbered choices remain unchanged. It borrows one
substantive assistant reply from the same user turn, replacing that request's
vector and label terms with a freshly embedded combined text. It adds no message
or confidence votes and preserves canonical source identity and the reserved split.

Selection stops at the next nonempty searchable user request, including one with
no stored embedding. Empty tool-result projections do not stop the turn. Tool,
system and developer text cannot supply subjects. Planning preambles, mixed
linear/native ancestry, sibling assistant branches and incomplete scan windows
are rejected. Native replies must descend from the anchor along one lineage.
The longest suitable reply wins, with ordinal as tie-breaker. This heuristic can
still prefer a broad or speculative reply; provenance does not establish accuracy.

Limits: 64-record window with a 65th-record truncation sentinel, 256-character
user request, 2,048-character assistant prefix, 64 KiB canonical source loads,
1,024 unique combined embeddings and inference batches of eight. Explicit
generation-compatible installed models are required. Preparation and inference
finish before mutating the in-memory diagnostic corpus; SQLite is read-only.

Private evidence schema 3 records both canonical identities and BLAKE3 hashes,
the user derived hash, assistant projection kind, prefix hash/length/completeness,
and the combined derived hash. Each contextual source consumes two references
from the existing 16,384-reference budget. Excerpts retain the existing 512-character
and shared 8 MiB limits, with explicit omissions and partial-text flags.

## Results and unresolved quality

Same 789-thread snapshot, canonical reserved split and embedding generation;
conserved support and exact leaf routing in both variants. Run all 16 configurations
per variant: two modes, two radii, two orders, full and reserved discovery models.
Only three requests qualify, and one receives context; two have no suitable answer.
This is targeted, nonblind source review, not a complete relevance evaluation.

For representative radius 0.4, the affected thread changes from generic pipeline
labels to recall-audit, inspectable-confirmation and evaluation subjects. Three
of the four full/reserved and forward/reverse combinations also surface deadlock
and harness subjects. Full forward still omits the specific deadlock domain.
Path-derived phrases contaminate some names, and the broader missing secondary
subjects remain unresolved. Do not promote this experiment as a general quality fix.

| Metric | Control full | Context full | Control reserved model | Context reserved model |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 769/778 | 770/778 |
| Reserved assigned / eligible | — | — | 136/145 | 137/145 |
| P95 tags, forward / reverse | 4 / 4 | 4 / 4 | 5 / 5 | 5 / 5 |
| Maximum tags, forward / reverse | 34 / 32 | 34 / 32 | 31 / 28 | 31 / 28 |
| Canonical-copy collapsed pair stability | 0.927 | 0.918 | 0.921 | 0.921 |
| Reserved-only canonical pair stability | — | — | 0.876 | 0.879 |

Full-corpus order stability regresses even though only one source is enriched.
Discovery and grouping changes can propagate beyond that source. Keep the
unenriched control and evaluate representative/routing stability alongside label
precision. Assigned coverage, named-cluster counts and stability are not accuracy.

The next quality work should suppress link-target/path noise while retaining
visible technical subjects, recover concrete domain names consistently across
orders, and complete grounded major/secondary-subject review. Avoid widening
assistant context indiscriminately: replies may introduce subjects the user did
not ask about. Production lifecycle, stable namespaces and MCP tools remain open.

## Validation and cost

539 regular tests pass, with 107 skipped. All 68 focused topic-related tests,
all-target/all-feature Clippy, doctests, formatting and diff checks pass.
Fixtures cover source boundaries, empty tool envelopes, unembedded next requests,
planning text, sibling/mixed/invalid ancestry, truncation, oversize data, changed
canonical anchors, two-origin evidence budgets, and missing-model atomicity.

Independent audits reproduce all 16 control reports against the preceding
candidate, excluding timing and the evidence schema version. All 16 contextual
reports retain canonical identity, split and eligibility. Captured user/assistant
ownership, ordinals, canonical/derived/prefix fingerprints, source alignment,
weighted score calculations, source and excerpt budgets, distinct thread/provider
counts and semantic forest consistency pass. These checks do not measure relevance.

Context preparation and one warm embedding take about 56–60 ms. Eight-configuration
full diagnostics take 24.51 s control and 21.78 s context; reserved runs take
22.10 s and 22.24 s. Peak RSS is approximately 1.09–1.12 GiB. Other jobs were
actively consuming CPU: the apparent full-run speedup is not evidence of a
performance improvement, nor are these production indexing/search measurements.

Private reports, source review and audits are in `mash-artifacts/topic-context-20261001`.
No real identifiers or raw histories enter Git. All runtime processing is Rust;
Python is used only for offline artifact audits.
