---
name: python-coding-conventions
description: Use when writing, revising, or reviewing Python production code, tests, fixtures, scripts, or migrations where maintainability, typing, modularity, documentation, readability, reuse, testability, or extensibility matters.
---

# Python Coding Conventions

Write Python whose behavior, boundaries, and reasons stay clear as the code
changes. Prefer the smallest design that makes current requirements easy to
test and likely changes local.

## Authority

Apply rules in this order:

1. User requirements and approved behavioral contracts.
2. Repository instructions, supported Python versions, configuration, and
   established local patterns.
3. Framework and public compatibility constraints.
4. Defaults in this skill.

Surface a correctness or maintenance risk when a higher-priority convention
causes one. Do not silently replace project policy.

For existing code, improve the touched area and its necessary seams. Avoid
unrelated cleanup, repository-wide restyling, speculative migration, or new
dependencies.

## Working Contract

### 1. Orient

Before editing, inspect nearby modules and tests plus `pyproject.toml` and active
formatter, linter, type-checker, and test configuration. Match naming, imports,
layout, error vocabulary, public API patterns, and supported Python version.

Default to the `uv` / `ty` / `ruff` stack:

- Use `uv` for environments, dependency changes, locking, and Python command
  execution.
- Use `ty check` for static type checking.
- Use `ruff check` and repository-configured `ruff format` for linting and
  formatting.
- Use `click` for command-line parsing: commands, options, arguments, and the
  exit code a command returns.
- Do not substitute `pip`, `mypy`, Black, isort, or `argparse` unless user or
  repository policy explicitly requires them.

### 2. Shape boundaries

- Give each module, class, and function one coherent responsibility.
- Keep public APIs small. Make inputs, outputs, mutation, I/O, and failure modes
  apparent from the signature and contract.
- Separate domain decisions and transformations from filesystem, network, DB,
  clock, environment, CLI, and framework adapters.
- Pass replaceable external collaborators explicitly. Avoid mutable global
  registries, service locators, import-time work, and hidden singleton state.
- Prefer composition and plain functions. Add classes, protocols, factories,
  or plugin mechanisms only for observed variation or a stated extension seam.
- Keep related code together. Extract only when the new unit gains a clear name,
  contract, independent test value, or reused behavior.
- Encode and decode JSON with `orjson`, not `json`. It is several times faster,
  returns bytes, and matches `json.dumps(ensure_ascii=False, separators=(",",
  ":"))` byte for byte, so it is safe under content hashing. Keep `json` where
  `orjson` cannot follow, and say which in a comment: decode hooks
  (`object_pairs_hook` for duplicate keys, `parse_constant`), `ensure_ascii=True`,
  incremental `JSONDecoder`, integers past 64 bits, and code that runs inside a
  caller's interpreter and cannot add a dependency. Prefer `json` for input a
  foreign toolchain produced when the decode error reaches a user, because the
  two libraries word that message differently.

### 3. Model data and types

- Type changed interfaces and non-obvious locals at the repository's enforced
  level. Do not spread `Any` across an internal or public boundary.
- Validate untrusted or loosely typed data once at the boundary; convert it to a
  typed stable representation and keep it typed through application layers. A
  mapping whose keys are known and fixed is a record with its type erased: give
  it a class, and reserve `dict[str, Any]` for genuinely open-ended keys. Decode
  once into that class rather than threading the decoded mapping onward.
- Treat Pydantic as the presumptive default for modelable structured data when
  repository policy permits.
  Replace `isinstance` chains, loose mappings, parallel codecs, and
  duplicated schemas. Hand-roll only for a concrete semantic or compatibility
  reason.
- Let each Pydantic model own validation, aliases, conversion, serialization,
  and schema. Validate once at ingress, keep values typed, and translate failures
  into the boundary's error vocabulary. Separate genuinely different contracts.
- Generate JSON Schema from its owning Pydantic model, never as an independent
  source. Test custom validators and byte-level rules separately.
- Choose by semantics: Pydantic models for records needing validation or
  conversion, dataclasses for trusted values, enums for closed choices,
  `TypedDict` only when a mapping is required, and protocols for consumer-owned
  boundaries.
- Keep byte-level requirements such as member order, duplicate-key policy,
  canonicalization, framing, and hash inputs explicit in tested codec functions
  even when a library owns model conversion.
- Preserve useful types. Do not collapse paths, timestamps, money, identifiers,
  or domain states into strings or dictionaries without a boundary reason.
- Make optionality real. Use `None` only when absence is a valid state, then
  handle it explicitly.

### 4. Keep behavior legible

- Use domain names and straightforward control flow. Prefer guard clauses over
  deep nesting and named intermediate values over dense expressions.
- Name constants `UPPER_CASE` without a leading underscore, including constants
  internal to a module or class and sentinel objects. Use `_UPPER_CASE` only
  when a language, framework, interoperability, or compatibility constraint
  requires that exact name; privacy alone is not a reason.
- Keep mutation local. Prefer pure transformations for policy and calculation.
- Encode invariants once, near the owning boundary. Remove duplication only
  when the shared concept and change cadence are genuinely the same.
- Optimize after evidence. Retain a simpler implementation unless measured
  constraints require complexity.

### 5. Handle failures deliberately

- Reject invalid states at the earliest owning boundary.
- Raise specific exceptions callers can act on; preserve causes with
  `raise ... from error` when translating layers.
- Catch the failures a boundary can actually produce. A bare `except Exception`
  around foreign code also swallows defects in your own and reports them as the
  foreign failure, so name the types and let everything else surface.
- Include safe, useful context in messages. Never expose secrets or silently
  swallow failures.
- Use context managers and explicit ownership for resources and cleanup.

### 6. Document contracts and reasons

- Document public APIs and non-obvious extension seams, invariants, side effects,
  failure modes, units, and ordering guarantees.
- Comment where the code cannot state its own reason: a choice whose obvious
  alternative is wrong, a constraint imposed from outside the file, a value
  derived rather than picked, surprising platform or library behavior, and a
  deliberate omission a reader will look for and not find.
- Say why, and what goes wrong otherwise. Never restate the mechanism; the code
  already carries it. Reasoning left only in a test name, a commit message, or
  a reviewer's head is unavailable at the call site.
- Keep it proportionate, usually one or two lines. Where a specification or
  other normative document owns the rule, cite it instead of restating it.
- Do not add docstrings to every private helper mechanically. Clear names and
  types are better than duplicated prose.

### 7. Stack test guidance

These conventions govern test code, fixtures, and helpers too. Follow the
repository's test strategy for proof scope, fixture design, mocking, and
coverage; keep Python-specific quality rules in force unless a more specific
test contract explicitly overrides them.

## Quick Review

| Concern | Completion question |
|---|---|
| Local fit | Does code follow repository version, tools, and nearby patterns? |
| Boundary | Are validation, domain logic, and side effects separated clearly? |
| Coupling | Can external collaborators be replaced without global mutation? |
| Types | Do interfaces express valid data and actual optionality? |
| Extension | Is current variation easy without speculative machinery? |
| Errors | Can callers distinguish and diagnose expected failures safely? |
| Documentation | Would a refactor that breaks an invariant be warned in code? |
| Scope | Did cleanup remain within the touched area and necessary seams? |

## Test scope

While iterating, judge test scope from the reach of each change. Keep to the
changed module for a body-local or private change.
Widen to importing modules when a public name, signature, dataclass field, or
exception type changed. Run the full suite for import-time side effects,
module-level state, or a change spanning several packages. When reach is
unclear, choose the narrower scope; the completion gate still runs everything.

## Verify

While iterating narrowly, run the tests covering the changed modules by
path or `-k` filter, and `--last-failed` while working through a failure. Python
imports at runtime, so selection by path misses callers: include the tests for
modules that import a changed public name.

Run the full suite once before reporting the work complete, with
repository-configured `uv`, `ty`, and `ruff` checks. When repository policy
explicitly selects another tool, follow it and report the deviation. Add no
tool only to satisfy this skill. Report unrun checks and intentional convention
deviations.

Go straight to the full suite when a dependency, lockfile, `pyproject.toml`,
conftest, or fixture shared across suites changed.
