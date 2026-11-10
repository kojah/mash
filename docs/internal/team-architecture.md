# Team product architecture

Status: proposed, not implemented. This describes how Mash changes if it becomes
a product for a team, where each person has their own machines and agents and
chooses what colleagues can see. The interface this supports is the team
prototype tracked in Beads `mash-kzbm`.

Today Mash is a single-user tool. Several of its rules assume one owner, and
this document names each rule that would change. Nothing here is decided until
those rules are changed in `AGENTS.md`.

## What already fits

- A machine publishes to one authenticated server, with one joint index per
  group and node ownership enforced by the service.
- Each node has its own bearer token. Group membership is the server's `nodes`
  list.
- `list-chats` and search already merge local SQLite with the other nodes in
  the shared index, deduplicate, and degrade to local results when the server
  is unreachable.
- Agent history is stored per OS user, so one Mash install only ever reads one
  person's chats.
- Records (memory) already carry revisions, evidence quotes and confirmations.
- Tombstones already remove a chat from the shared index.

The group is the team. What is missing is the person, the choice of what to
publish, and a check before publishing.

## Concepts

| Concept | Meaning | Unit of |
|---|---|---|
| Workspace | One group on one server | Membership and rules |
| Person | An account in the workspace | Ownership, privacy, sharing |
| Machine | A host | Services, indexing, fleet actions |
| Node | One person's Mash install on one machine | Publishing and authentication |
| Automation | A person-like account for CI or scheduled agents | Attribution without a human |

Person and machine are many-to-many. A person can own several nodes, and a
machine can host nodes for several people. Every chat carries both labels.

Shared OS accounts cannot be separated: if several people log in as one user,
their chats belong to that account.

## Sharing levels

Each chat has one effective level:

- **private:** never leaves the machine.
- **memory only:** its records and their evidence quotes are published. Its
  messages are not.
- **team:** the chat is published and searchable by the workspace.

The level comes from the chat's project, with an optional per-chat override.
New projects are private. A fourth state, **held**, applies to a chat in a
shared project whose scan found something sensitive. It behaves as private
until its owner decides.

Privacy is enforced by where data lives, not by permissions: a private chat is
never sent, so no server role can read it. This is why admins cannot read
private chats.

## What changes

### Identity and membership

- The server gains a person registry. Each node token maps to a person and a
  machine name, in addition to its node ID.
- Roles: admin (manage members, rules and tokens) and member. An automation is
  a member that never signs in to a dashboard.
- Joining issues a node token bound to a person. Leaving revokes that person's
  tokens. Their published chats stay unless the workspace rule says otherwise.
- Membership changes should not need a server restart, which they do today.

### Publication becomes a filtered projection

Today "shared publishes exactly the local index". Under this design a node
publishes a subset, and the published copy can differ from local text because
of redaction. Consequences:

- A publication policy on the node decides, per chat, whether to publish
  messages, records only, or nothing. Inputs are the project rules, per-chat
  overrides and scan results.
- The policy has a revision. Certification and freshness must compare against
  "local index filtered by policy revision N", not the whole local index.
  `status --shared` must report policy-excluded chats separately from stale
  ones, so a private chat is never shown as "behind".
- Lowering a level (team to private) publishes a tombstone. Raising it
  publishes the chat. Both must be idempotent and safe to retry.
- Redaction applies only to the published copy. Local silver stays canonical
  and unredacted. Embeddings for published text must be computed from the
  redacted text, so a redacted secret cannot be recovered from a vector.

This touches publication ordering, tombstones and revision guards, so the
models in `protocol/` need the policy revision and level changes added before
implementation.

### Scan and hold

- A scanner runs on the node before a chat's first publication and again when
  new messages arrive. It uses a maintained secret rule set, plus workspace
  rules.
- A finding moves the chat to held. Held state and findings stay local.
- The owner chooses "publish with redactions" or "keep private". The choice is
  stored as a per-chat override.
- Scanning is best-effort and must be described that way. Evidence quotes for
  memory-only chats are scanned too.

### Storage

- Local silver: new tables for project sharing rules, per-chat overrides and
  scan findings. This is a schema migration for local databases. Each node's
  shared silver on the server gains the owning person.
- Qdrant payloads gain person and machine fields with filter indexes. This is a
  targeted payload update and does not recompute vectors.
- Records gain an owner and a visibility that follows their source chat. A
  record from a private chat is private.

### Reading and search

- The server filters by workspace only in the first version: every member can
  read everything published. Per-project access can follow later by mirroring
  repository access.
- Search results and chat lists carry the owner. New filters: person and
  visibility.
- Agents reach team history through MCP. Other people's transcripts are
  untrusted input to an agent, more so than one's own. MCP tools take a
  `scope` (mine, team memory, team), and the default should not include
  teammates' full transcripts.
- A teammate's chat can be read and exported. It cannot be resumed, so the
  session ID is not offered.

### Dashboard

- `mash dash` stays local. Team pages read through the shared API and show the
  owner's own data when the server is unreachable.
- Sharing rules and per-chat levels are writes from the browser. They need new
  typed, allowlisted mutations for local policy, separate from history routes,
  which stay read-only.
- Services and fleet actions stay scoped to the user's own machines. A
  teammate's machine appears read-only. Workspace-wide fleet control for admins
  is out of scope.

## Rules in AGENTS.md that would change

- "shared publishes exactly the local index" becomes "shared publishes the
  policy-filtered, redacted projection of the local index".
- "one shared index per configured node group" stays, with the group named a
  workspace and nodes attributed to people.
- Dashboard mutation rules gain an allowlisted class for sharing policy.
- The project's single-user stance ("one-user project") is replaced.
- Unchanged: Rusqlite only, common Qdrant engine, silver before views, no
  provider traits, no SSH fan-out, read-only access to upstream agent files.

## Phases

1. **Attribution.** Person registry and node-to-person mapping. Owner on chats,
   records and search results. Read-only team view of what is already
   published. No privacy change yet, so this is only safe for a group that
   already shares everything.
2. **Project sharing.** Publication policy with project rules, private by
   default, per-chat override and unshare by tombstone. Protocol models
   updated first.
3. **Scan and hold.** Local scanner, held queue, redacted publication.
4. **Memory only.** Publish records and quotes without messages.
5. **Workspace management.** Roles, invites, token rotation and member removal
   without a restart.
6. **Finer access.** Per-project read access.

Phases 2 and 3 must ship together before a real team uses it. Phase 1 alone
exposes everything a node publishes.

## Open questions

- Does unsharing remove a chat that a teammate's agent already read or that
  memory already quotes? Tombstones remove the copy on the server, not what was
  learned from it.
- Who owns a chat started by an automation on a shared build machine?
- Should the server keep an access log of who read which chat? It helps trust
  and raises its own privacy question.
- When someone leaves, do their shared chats stay, transfer or expire?
- Is the server self-hosted only, as today, or also offered as a service?
- How are worktree and scratch directories folded into their parent project, so
  one sharing rule covers them?
