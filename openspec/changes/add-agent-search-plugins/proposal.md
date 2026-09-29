## Why

achi is useful while an agent is working, but today searching prior sessions
requires leaving the agent interaction and manually invoking the CLI. Claude
Code and Codex should each expose their native command surface for asking achi
to find relevant history without introducing another search implementation.

## What Changes

- Ship one repository-owned `achi` plugin package consumable by Claude Code and
  Codex.
- Expose `/achi:search <query>` in Claude Code and the corresponding
  `$achi-search <query>` skill invocation in Codex.
- Delegate every request to the installed `achi search` command and render its
  JSON result as concise, source-identifiable matches.
- Report missing input, unavailable tooling, typed achi failures, and empty
  results without inventing history or silently mutating the index.
- Document local installation and validate both host manifests and the shared
  search instruction contract.

## Capabilities

### New Capabilities

- `agent-search-invocation`: Host-native Claude Code and Codex plugin entry
  points for searching indexed achi history.

### Modified Capabilities

None.

## Success Boundary

With the plugin installed and `achi` available on `PATH`, a user can supply the
same query through Claude Code or Codex and receive the ordered matches returned
by `achi search`, including identifiers needed for follow-up. Invalid requests
and achi failures remain explicit, and invoking the plugin performs no indexing,
model installation, or other write operation.

## Non-Goals

- Adding a seventh achi CLI command, an achi-internal plugin framework, MCP
  server, daemon, or alternate output format.
- Installing the achi binary, downloading models, creating configuration, or
  automatically refreshing an index.
- Adding plugin commands for `index`, `view`, `status`, `forget`, or
  `models install`.
- Making Claude Code and Codex use identical invocation syntax where the hosts'
  native extension surfaces differ.

## Impact

The change adds distribution assets and documentation for the two supported
agent hosts. It depends only on the existing `achi search` JSON contract and an
`achi` executable on `PATH`; the Rust CLI, database schema, retrieval behavior,
and deployment topology remain unchanged.
