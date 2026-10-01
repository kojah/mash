# Visible-dialogue budget and source-backed reference review

2026-10-01; `mash-c0d.19.2.28`, following the
[fresh context audit](2026-10-01-fresh-referential-context-audit.md).
No topic method or production change is promoted.

## Frozen procedure

Retain exactly the prior four source groups/21 referential anchors and the original
three-user/64-raw-record/16KiB-message/32KiB-display control. Before new captures,
freeze a separate three-user/16-visible-record control with a 4,096-raw-record
safety ceiling and identical display limits. This changes source scope explicitly.
Tools, developers and empty assistant projections do not consume the visible
budget but remain exact traversed identities/hashes. Omitted/redacted visible
records do consume it. Stop on the first user/visible/raw bound or root; visible/raw
stops remain incomplete. Broken/mixed native parents never fall back to linear.

`dialogue_budget.py` requires the immutable original capture and regenerates every
original context exactly before accepting results. Native canonical identity/role/
content checks extend to every newly displayed user/assistant record; current
native file hashes must remain unchanged. Independent native order/tools/all-edge
verification remains unclaimed. Source bodies and judgments stay private.

## Results

Two final captures exactly match every nonmeasurement field across both controls.

| Measure | Original raw64 | Visible16/raw4096 |
| --- | ---: | ---: |
| Anchors | 21 | 21 |
| Root stops | 3 | 3 |
| Three-prior-user stops | 0 | 12 |
| Raw-limit stops | 18 | 0 |
| Visible-limit stops | none configured | 6 |
| Prior-user count distribution (0/1/2/3) | 3/9/9/0 | 0/2/7/12 |
| Traversed records | 1,191 | 3,070 |
| Displayed records | 143 | 261 |
| Display bytes | 68,651 | 113,350 |
| Cited evidence available for reviewed task references | 20/20 | 20/20 |

All178 unique newly audited anchor/display identities match native canonical
content/role. More complete user history does not imply better semantic evidence.

The source-backed sidecar reviews all21 anchors:20 task references and one unresolved
readiness referent whose root context contains only injected instructions. The
current question's own topic-bearing words remain usable; unresolved context does
not imply dropping the current message. Each resolved judgment cites exact intact,
causal, displayed original identities. The shared validator rejects missing/duplicate
judgments, stale pins, changed bounds, edited/omitted/redacted evidence, and unknown
fields. Judgments/names/rationales are evaluation-only, never model text.

Examples require scope rather than blanket context pooling: selecting numbered
actions1/2 does not approve the rest; fixing remaining cases does not request already
completed work; deferred analyzer reorganization points to an earlier boundary
proposal rather than the nearest MCP-installation result; rejecting migration is
not acceptance of the preceding compatibility plan. These are exposed assistant
interpretations, not independent human intent judgments or automatic resolutions.

Every cited antecedent is already in the original raw64 display. Increasing the
raw ceiling adds65.1% displayed bytes without improving this oracle evidence
availability metric. The original cap is therefore not the limiting factor for
these selected references. Do not promote the larger window based on user-count
completeness. Next prioritize selecting relevant/accepted source spans from the
existing context; keyword quality and automatic resolution remain unmeasured.

Capture sums: raw28.76–51.96ms, visible34.92–74.24ms across21anchors. Raw timing
also includes source-sequence validation; concurrent test activity/host noise varies.
These are descriptive capture costs, not a fair kernel comparison, distributions,
full ingestion or model-window costs. Zero inference/LLM calls; byte limits do not
replace the encoder's separate strict token preflight. No speed/memory/default claim.

## Reproduction and limits

136 lab tests pass. New tests cover distinct raw/visible limits, omitted records
consuming the visible budget, sibling exclusion, exact original control, corrupted
repeat grids, source-backed complete reviews and actual text-hash availability.
Existing legacy reference validation shares the same evidence checks.

Run `dialogue_budget.py` with database/partition/packets/review/baseline and fresh
private output paths. `compare_dialogue_budget.py --first ... --second ... --output ...`
requires complete grids and every nonmeasurement field. `packet_reference_review.py`
accepts contexts/packets/source-review/annotations and reports validated judgment
counts plus oracle availability, not automatic quality.

Private root `topic-python-review-v3-20261001`:
`dialogue-budget-final-{first,second}.json`, capture SHA256
`9f5caec701b5b2b7570817583928422b3b3389063268bf0c5ca4360776643546`,
`a921f013656a520848bb7f78ebbb91b302fcdaaed7f8e1caeb81f19aa194b3f1`;
`visible-reference-review.json` SHA256
`0701723d4edbd6ba04c9a910f5b1332d31d4540af35a413bbabaac5c7db522b2`.
An earlier capture predates the shared validator refactor and is excluded.

Only two exposed Codex source groups contain references, one mostly analyzer work.
Review follows source inspection and the coverage control, before any new semantic
ranking, and is single coding-assistant development evidence. No independent
annotation, provider-general quality, topic hierarchy/inherited tags, guided-search
gain, Rust promotion or complete <=10% ingestion gate is established.
