# `vspec` schema

OpenSpec artifact graph with Veritas-gated delivery planning.

```text
proposal -> specs -> design -> plan -> apply
```

`specs/**/*.md` may carry provisional claim markers from first draft onward.
Wording revisions preserve stable IDs when behavior identity remains. Claim
locking, coverage modeling, acceptance-test generation, evidence-link review,
and evidence approval are selected proof work, not universal pre-execution
stages. No hand-written evidence-readiness artifact duplicates Veritas state.
Optional generated reports are audit output only. `review-spec` remains
available on demand, but creates no required artifact. `plan.md` owns approved
delivery intent when durable planning is needed. Its structure is selected by
`create-implementation-plan`, which provides a light adaptable template as a
nonbinding checklist. VSpec requires no exact headings, frontmatter, checklist,
or progress-marker syntax.

Veritas decides completion from current evidence and findings. Consumer-
relevant promises no supported boundary can falsify stay in the spec and may
enter the plan as `[[coverage.exclude]]` entries carrying the claim and a
required `reason`; exclusions grant no coverage and no approval.

`design.md` also owns tooling compatibility. Missing native Veritas evidence
support stops design until the user approves a bridge, adds a producer, changes
stacks, or explicitly defers evidence with the workflow still blocked.

VSpec prefers active Veritas MCP bound to current project. When MCP is
unavailable, project-bound `vtas` CLI is accepted fallback. Proposal setup and
planning gate verify one path works; project context records selected mode.

Current accepted state lives under `openspec/product/`,
`openspec/architecture/`, and `openspec/specs/`. Product documents split by
area, architecture documents split by system, and behavioral specs retain
OpenSpec capability directories. Their README files map ownership without
creating placeholder documents. Changes reconcile affected product and
architecture documents as explicit plan work; OpenSpec archive alone promotes
capability delta specs and owns temporal history. VSpec creates no parallel
requirements, behavioral-test-requirements, discovery, task, or decision-log
artifact.
