## Context

achi already provides a synchronous, JSON-only `search` command whose response
contains ordered concrete hits. This change is an adapter and distribution
problem, not a retrieval problem. Claude Code plugins support namespaced command
documents such as `/achi:search`; Codex plugins expose skills, invoked here as
`$achi-search`. Both hosts can execute the installed CLI and interpret JSON.

The product boundary forbids adding a plugin system to achi itself. The host
plugins therefore remain static repository assets depending inward on the
public CLI contract. They do not link Rust code, open SQLite, or gain authority
to repair readiness automatically.

The current repository has no agent-plugin package or marketplace metadata.
The binary is deployed independently to Linux x86-64 and Apple Silicon hosts.

## Goals / Non-Goals

**Goals:**

- Provide one source-controlled plugin package that validates for both hosts.
- Keep the search behavior in one canonical skill instruction while giving
  Claude Code its native namespaced command spelling.
- Preserve query bytes as data, consume only the existing JSON response, and
  make failures and source identity visible.
- Test packaging and command-contract invariants without adding runtime
  dependencies to the Rust CLI.

**Non-Goals:**

- Adding Rust extension points, a seventh CLI command, MCP, or a background
  service.
- Installing or configuring achi, refreshing indexes, or downloading models.
- Wrapping other achi commands or designing a general natural-language command
  router.
- Publishing to a public plugin marketplace in the initial change.

## Ownership and Boundaries

`plugins/achi/` owns both host manifests and all agent-facing instructions.
`plugins/achi/skills/achi-search/SKILL.md` is the canonical behavior shared by
the hosts. `.codex-plugin/plugin.json` advertises that skill to Codex.
`.claude-plugin/plugin.json` identifies the Claude Code plugin, while
`commands/search.md` is a thin namespaced launcher that loads the canonical
skill with the command arguments.

The canonical skill may invoke only `achi search <query>` through the host's
command tool. `app/` remains the sole owner of configuration, federation,
readiness, retrieval, and JSON wire behavior. Plugin presentation consumes the
returned fields but does not redefine them.

## Decisions

1. **Use host-native invocation syntax.** Claude Code receives
   `/achi:search`; Codex receives `$achi-search`. Pretending Codex supports an
   arbitrary plugin slash command was rejected because its plugin extension
   surface is skills.
2. **One dual-host plugin root.** Both manifests live beside one canonical
   skill. Separate Claude and Codex repositories were rejected because they
   would duplicate behavior and release identity.
3. **Prompt adapter over wrapper executable.** The skill invokes `achi`
   directly with the query as one argument and parses its JSON. A shell wrapper
   was rejected because it adds another quoting boundary and an unnecessary
   executable artifact.
4. **Read-only failure handling.** Recommended remediation is displayed but
   never executed. Automatically running `index` or `models install` was
   rejected because a search invocation must not acquire write or network
   authority.
5. **Repository validation before publication.** Tests inspect the shared
   contract and run the available Claude and Codex manifest validators. Public
   marketplace publication is a later distribution decision.

## Tooling Compatibility

The product implementation remains Rust 2024. Native verification remains
`cargo nextest` with the configured Veritas `rust-test` evidence producer;
`cargo test --doc`, Clippy, and rustfmt retain their existing roles. Plugin
assets are Markdown and JSON consumed directly by Claude Code and Codex, so no
cross-language runtime bridge or new Rust dependency is introduced.

## Risks / Trade-offs

- **Host syntax differs.** Documentation and tests name both exact invocations;
  one shared behavior contract prevents semantic drift.
- **Agent instructions are less rigid than a wrapper program.** Keep the skill
  narrow, allow only `achi search`, and validate hostile-query and failure
  requirements with deterministic fixtures where the host tooling permits.
- **JSON fields can evolve.** Require identity fields used by current search
  results, tolerate optional ranking fields, and reject output that is not a
  valid search response rather than guessing.
- **Binary availability is external.** Detect a missing executable and give a
  direct installation/PATH diagnostic; the plugin never installs binaries.

## Migration / Rollback

This is additive. Installation points at `plugins/achi`; removing or disabling
that plugin fully rolls back the feature without touching the achi database,
configuration, models, or binary. No data migration is required.
