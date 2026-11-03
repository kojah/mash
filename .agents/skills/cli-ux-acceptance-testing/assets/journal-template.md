# CLI UX acceptance journal: <journey>

- Date: `<YYYY-MM-DD>`
- Mode: `<live | transcript | performance-add-on>`
- Product: `<name>`
- Tester: `<subagent | supplied transcript>`
- Pair ID/member: `<ID and A/B | not applicable>`
- Goal: `<user outcome>`
- Stop condition: `<success or bounded impasse>`
- Journey seed: `<unsigned integer | not applicable>`
- Journey family: `<selected family | retrospective>`
- Journey variation: `<selected variation | not applicable>`
- Journey exclusions: `<recent/batch families | none | not applicable>`
- Session: `<validated ID | not applicable>`
- Binary: `<session-local path or supplied artifact>`
- Revision: `<fixed revision | unknown>`
- Working/product-state roots: `<profile-reported paths | not applicable>`
- Isolation: `<every required profile field | not applicable>`
- Prior knowledge: `<none supplied | unavoidable context>`
- Paired journal: `<path | not applicable>`

## Guardrails

- [ ] Session is fresh, disposable, offline, bounded, and source-free.
- [ ] User, host, credential, and production state are absent.
- [ ] Peer uses a distinct session with the same goal, journey, binary, and revision.
- [ ] No implementation context or peer evidence was consulted before exploration ended.

## Timeline

### J-001 — <action>

- Immediate goal: `<goal>`
- Expectation: `<expected result>`
- Command: `<exact command>`
- Exit status: `<integer | unknown>`
- Stdout: `<exact relevant output | none | unavailable>`
- Stderr: `<exact relevant output | none | unavailable>`
- Perceived latency: `<instant | noticeable | slow | not recorded>`
- Friction: `<none | tester reaction>`
- Next decision: `<choice and reason>`

## Exploration boundary

`Exploration complete. Repository context may now be consulted.`

## Findings

### F-001 — <finding> (`<P0 | P1 | P2>`)

- Evidence: `<J-...>`
- Observation: `<observed behavior>`
- Impact: `<effect on outcome>`
- Suggested fix: `<concrete change>`
- Existing coverage: `<new | duplicate | not checked>`

## Performance add-on

- Perceived-slow evidence: `<J-... | none>`
- Performance question: `<workload, scale, metric>`
- Reproduction: `<command and environment>`
- Dominant cost: `<trace/profile evidence | not measured>`
- Result: `<measurement with uncertainty | not measured>`

## Session close

- Outcome: `<achieved | bounded impasse>`
- Cleanup: `<completed | retained artifact and reason>`
- Limitations: `<not established>`
- Synthesis: `<path | not applicable>`
