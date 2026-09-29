## Purpose

Define host-native plugin entry points that let Claude Code and Codex search
indexed achi history through the existing JSON CLI without acquiring any write
authority.

## ADDED Requirements

### Requirement: Each supported host exposes a discoverable search invocation

An installed achi plugin SHALL expose `/achi:search <query>` in Claude Code and
`$achi-search <query>` in Codex. Each entry point SHALL identify itself as a
search over indexed Claude Code and Codex history and SHALL accept the remainder
of the invocation as one search query.

#### Scenario: Claude Code invocation is available

<!-- claim: agent-search-plugins/claude-command-is-discoverable -->
- **WHEN** the achi plugin is installed and enabled in Claude Code
- **THEN** `/achi:search` is discoverable and accepts a query for achi

#### Scenario: Codex invocation is available

<!-- claim: agent-search-plugins/codex-skill-is-discoverable -->
- **WHEN** the achi plugin is installed and enabled in Codex
- **THEN** `$achi-search` is discoverable and accepts a query for achi

### Requirement: Search delegates to the existing achi CLI

For a nonempty query, each plugin entry point SHALL invoke the `achi` executable
resolved from `PATH` with `search` and the complete query as one positional
argument. It SHALL NOT interpret query text as shell syntax, select a different
database, override configured federation, or implement retrieval itself.

#### Scenario: Multiword query is preserved

<!-- claim: agent-search-plugins/query-is-one-argument -->
- **WHEN** a user searches for `sqlite migration race`
- **THEN** the plugin invokes `achi search` with `sqlite migration race` as one query argument

#### Scenario: Shell-like query is inert

<!-- claim: agent-search-plugins/query-is-not-evaluated -->
- **WHEN** a query contains whitespace, quotes, substitutions, or shell metacharacters
- **THEN** every character is passed as query data and no additional command is executed

#### Scenario: Query is absent

<!-- claim: agent-search-plugins/query-is-required -->
- **WHEN** the host invocation contains no non-whitespace query
- **THEN** the plugin explains the required invocation syntax without running achi

### Requirement: Successful results retain search identity and order

The plugin SHALL parse achi's single JSON search response and present matches in
the order returned. Every presented match SHALL retain its achi message ID,
provider, role, content, and any reported node origins so the user can identify
the source and request follow-up context. Presentation MAY omit internal ranking
scores unless the user asks for them.

#### Scenario: Search returns matches

<!-- claim: agent-search-plugins/results-retain-identity -->
- **WHEN** `achi search` succeeds with one or more ordered results
- **THEN** the plugin presents those results in the same order with each result's ID, provider, role, content, and reported origins

#### Scenario: Search returns no matches

<!-- claim: agent-search-plugins/empty-results-are-explicit -->
- **WHEN** `achi search` succeeds with an empty `results` array
- **THEN** the plugin reports that no indexed history matched and does not fabricate a result

### Requirement: Search failures remain explicit and actionable

The plugin SHALL distinguish an unavailable `achi` executable, malformed
command output, and a typed achi error. For a typed error it SHALL preserve the
reported error kind and message and surface a reported recommended action when
present. It SHALL NOT present partial standard output or model-generated text as
search results after a command failure.

#### Scenario: achi is not installed

<!-- claim: agent-search-plugins/missing-binary-is-reported -->
- **WHEN** the host cannot resolve `achi` from `PATH`
- **THEN** the plugin reports that achi must be installed or added to `PATH` and returns no matches

#### Scenario: Search is not ready

<!-- claim: agent-search-plugins/typed-error-is-preserved -->
- **WHEN** achi returns a typed `search-not-ready` error recommending `achi index`
- **THEN** the plugin reports that error kind, message, and recommendation without running the recommendation

#### Scenario: Output is not valid achi JSON

<!-- claim: agent-search-plugins/malformed-output-is-rejected -->
- **WHEN** the command succeeds but its output is not one valid achi search response
- **THEN** the plugin reports an invalid-output failure and presents no matches

### Requirement: Plugin search is read-only

Invoking either plugin entry point SHALL run at most the `achi search` command.
It SHALL NOT run `index`, `forget`, `models install`, modify configuration, or
write plugin-maintained search state, including when search reports a
recommended action.

#### Scenario: Remediation is recommended

<!-- claim: agent-search-plugins/remediation-is-not-automatic -->
- **WHEN** achi reports that models or index state are not ready
- **THEN** the plugin explains the recommendation and performs no remediation command
