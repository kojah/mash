# CLI UX acceptance synthesis: <journey>

- Date: `<YYYY-MM-DD>`
- Pair ID: `<collision-free ID>`
- Synthesizer: `<fresh subagent>`
- Product: `<name>`
- Revision: `<shared fixed revision>`
- Goal: `<shared outcome>`
- Stop condition: `<shared stop>`
- Journey seed/family/variation: `<shared selection>`
- Raw journal A: `<absolute path>`
- Raw journal B: `<absolute path>`

## Pair validity

- [ ] Exactly two fresh testers ran concurrently.
- [ ] Goal, stop, journey, binary, and revision match.
- [ ] Distinct sessions satisfy every profile isolation field.
- [ ] Neither tester received peer or implementation context.
- Decision/evidence: `<valid | invalid; citations>`

## Accepted findings

Ranked most severe first. Complete on its own — a reader acts from this table
and reads the decisions below only to check the reasoning.

| ID | Finding | Class | Pri | Owner | Suggestion | Evidence |
|---|---|---|---|---|---|---|
| `<S-001>` | `<one line>` | `<class>` | `<P0-P2>` | `<owner>` | `<concrete change, or the decision it needs>` | `<tester/finding IDs>` |

## Candidate decisions

### S-001 — <candidate> (`<P0 | P1 | P2>`)

- Raw evidence: `<A/B finding and timeline IDs>`
- Comparison: `<overlap, non-contradiction, contradiction, or gap>`
- Decision: `<accepted-reproduced | accepted-valid | rejected | needs-more-evidence>`
- Rationale: `<evidence and impact>`
- Observation/impact: `<behavior and user effect>`
- Suggested fix: `<concrete change | none>`

## Publication gate

- Accepted: `<S-... | none>`
- Rejected: `<S-... | none>`
- Needs more evidence: `<S-... | none>`
- External updates: `<authorized links | none>`

Only accepted findings may be published as product feedback.
