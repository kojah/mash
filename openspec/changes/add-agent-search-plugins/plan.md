# achi agent search plugins implementation plan

## Outcome

Ship one repository-owned plugin package at `plugins/achi/` that exposes
`/achi:search <query>` in Claude Code and `$achi-search <query>` in Codex. Both
host entry points use one canonical instruction contract and invoke only the
installed `achi search` CLI. This plan adds distribution, documentation, and
proof assets; it does not change the Rust CLI, its six-command surface, search
implementation, database, models, configuration, or deployment.

## Preservation and delivery decisions

- Preserve the existing JSON contracts in `app/cli.rs`, `app/lib.rs`, and
  `app/storage.rs`: successful search output remains one `SearchResponse` with
  ordered `SearchHit` values, while failures remain one typed JSON error on
  standard error.
- Keep `plugins/achi/skills/achi-search/SKILL.md` as the only behavioral
  instruction source. Host manifests and the Claude command may identify or
  launch it, but must not restate a second search policy.
- Pass the complete non-whitespace query to `achi search` as one argument.
  Shell-like text is data, never command syntax. Do not add a wrapper executable
  or another quoting boundary.
- Treat search as read-only even when achi recommends `index` or
  `models install`. The plugin reports remediation but never runs it.
- Preserve result order and expose each match's `id`, `provider`, `role`,
  `content`, and nonempty `origins`. Ranking fields may remain hidden unless
  requested.
- Keep public marketplace publication out of scope. Initial distribution is
  the source-controlled plugin directory plus documented local installation.

## Implementation units

### Canonical read-only search skill

Create `plugins/achi/skills/achi-search/SKILL.md` as the complete host-neutral
contract for one search invocation.

- **Responsibility:** Validate that a query is present, resolve `achi` from
  `PATH`, invoke only `achi search` with the full query as one positional
  argument, parse exactly one JSON response, render ordered source-identifiable
  matches, and distinguish empty results, a missing binary, typed achi errors,
  and malformed output.
- **Likely seams:** The new skill plus the existing success and error shapes in
  `app/cli.rs`, `app/storage.rs`, and `app/lib.rs`. No Rust source changes are
  expected.
- **Dependencies:** The shipped `achi` executable and its existing JSON-only
  search contract.
- **Preservation:** Never invoke `index`, `view`, `status`, `forget`, or
  `models install`; never override database, configuration, federation, node,
  provider, limit, or model settings; never reinterpret or fabricate output.
- **Verification:** Cover missing and whitespace-only queries, multiword and
  shell-like queries, ordered populated results, empty results, missing `achi`,
  typed `search-not-ready` output with its recommendation, malformed success
  output, and the prohibition on automatic remediation. These scenarios own
  the nine non-discoverability claims under `agent-search-plugins/*`.

### Dual-host package and installation surface

Add the host metadata and thin Claude adapter around the canonical skill.

- **Responsibility:** Make the same package discoverable as `/achi:search` in
  Claude Code and `$achi-search` in Codex, and document local installation,
  prerequisites, invocation examples, output identity, and read-only failure
  handling.
- **Likely seams:**
  `plugins/achi/.claude-plugin/plugin.json`,
  `plugins/achi/.codex-plugin/plugin.json`,
  `plugins/achi/commands/search.md`, and the plugin installation section of
  `README.md`.
- **Dependencies:** The canonical `achi-search` skill. The Claude command is a
  namespaced launcher carrying `$ARGUMENTS`; the Codex manifest advertises the
  sibling `skills/` directory.
- **Preservation:** Keep host-specific syntax explicit. Do not claim that Codex
  provides an arbitrary plugin slash command, install the achi binary, add MCP,
  or publish marketplace metadata.
- **Verification:** `claude plugin validate plugins/achi` must accept the
  Claude package, and Codex's local plugin validator must accept the same root.
  Host inventory or a bounded installation smoke must show the exact two
  invocations. This unit owns
  `agent-search-plugins/claude-command-is-discoverable` and
  `agent-search-plugins/codex-skill-is-discoverable`.

### Contract validation and Veritas proof

Add deterministic repository checks for package structure and the shared
contract, then exercise observable invocation behavior where the host tooling
supports it.

- **Responsibility:** Detect manifest drift, duplicated host behavior, missing
  required identity/error handling, forbidden write commands, and accidental
  changes to the Rust CLI surface. Supply fixtures or bounded host evaluations
  with a fake `achi` executable for success, empty, typed-error, malformed, and
  hostile-query cases.
- **Likely seams:** A focused plugin-contract test under `app/tests/` with its
  explicit `Cargo.toml` test target, host validator invocations in CI, and
  host-evaluation fixtures under `plugins/achi/` only if they can run without a
  new product runtime dependency.
- **Dependencies:** Both preceding units and the existing `cli_contract` test
  as the authority for the six-command achi surface.
- **Preservation:** Static inspection may prove package shape and instruction
  prohibitions, but must not be cited as runtime proof that an agent obeyed the
  instruction. Claims requiring observable invocation or presentation remain
  unapproved until a supported host evaluation or equivalent behavioral
  boundary exercises them.
- **Verification:** Run the focused plugin-contract tests and both host
  validators, then the repository's normal `nextest`, Clippy, rustfmt, and doc
  gates. Reconcile Veritas afterward. The 11 currently added
  `agent-search-plugins/*` claims begin as `MISSING_LOCK_ENTRY`; locking,
  evidence discovery, semantic link review, and approval remain explicit
  authority-gated transitions rather than implied completion.

## Risks and open questions

- Prompt-driven command execution is less mechanically constrained than a
  wrapper binary. Keep the canonical skill narrow and require behavioral host
  evaluation for claims that static files cannot prove.
- Host plugin schemas may evolve independently. Treat each host's current
  validator as authoritative and keep host-only metadata out of the shared
  search instructions.
- If Codex lacks a behavioral plugin-evaluation boundary comparable to Claude's,
  leave Codex runtime claims pending or perform an explicit bounded smoke; do
  not weaken the claim or substitute manifest validation for behavior.
