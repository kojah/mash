# Team product architecture

Status: proposed, not implemented. This describes how Mash changes if it becomes
a product for a team, where each person has their own machines and agents and
their chats are shared with colleagues by default. The interface this supports
is the team prototype tracked in Beads `mash-kzbm`.

Today Mash is a single-user tool. Several of its rules assume one owner, and
this document names each rule that would change. Nothing here is decided until
those rules are changed in `AGENTS.md`.

## Chosen approach

A node keeps publishing exactly its local index, as it does today. Privacy is a
flag on the server: a private chat is stored there and hidden from everyone but
its owner.

The alternative was to keep private chats on the owner's machine and publish a
filtered subset. That gives a stronger promise, because the server never holds
private chats. It was set aside because it changes what certification and
freshness compare against, needs a policy revision in the publication protocol,
and makes sharing opt-in, so a team sees little on day one.

The cost of the chosen approach is stated to users plainly: whoever runs the
shared server can read private chats.

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
- Recognizable credentials are already masked in outgoing publications and in
  shared read responses. See [privacy](privacy.md).
- `forget` already removes a chat from the shared index with a tombstone.

The group is the team. What is missing is the person, the private flag, and a
choice about what a new member brings.

## Concepts

| Concept | Meaning | Unit of |
|---|---|---|
| Workspace | One group on one server | Membership and rules |
| Person | An account in the workspace | Ownership and visibility |
| Machine | A host | Services, indexing, fleet actions |
| Node | One person's Mash install on one machine | Publishing and authentication |
| Automation | A person-like account for CI or scheduled agents | Attribution without a human |

Person and machine are many-to-many. A person can own several nodes, and a
machine can host nodes for several people. Every chat carries both labels.

Shared OS accounts cannot be separated: if several people log in as one user,
their chats belong to that account.

## Visibility

Each published chat is either:

- **team:** every member of the workspace can search and read it. This is the
  default.
- **private:** only its owner can search and read it.

The level comes from the chat's project, with an optional per-chat override.
An owner can mark a project private, and every chat in it follows.

A third state, **held**, applies when the scan finds something that looks
sensitive beyond a maskable credential. A held chat is treated as private until
its owner chooses "share with team" or "keep private".

Records follow their source chat. A record from a private or held chat is
private.

## What changes

### Identity and membership

- The server gains a person registry. Each node token maps to a person and a
  machine name, in addition to its node ID.
- Roles: admin (manage members, rules and tokens) and member. An automation is
  a member that never signs in to a dashboard.
- Joining issues a node token bound to a person. Leaving revokes that person's
  tokens. Their shared chats stay unless the workspace rule says otherwise.
- Membership changes should not need a server restart, which they do today.

### Publication

Publication itself does not change: shared still publishes exactly the local
index, so certification, freshness checks and the protocol models keep their
current meaning.

Visibility is separate server-side state, not part of the published history:

- The server stores project rules and per-chat overrides per person, and the
  effective visibility of each chat.
- A change is a small authenticated write by the owner. It does not republish
  the chat or publish a tombstone.
- Visibility must survive a node republishing or replaying its history, so it
  is keyed by stable chat identity and not carried in the node's payload.
- A person's rules apply to all of their nodes.

### What a new member brings

Joining with an existing index would publish years of history at once, across
work and personal projects. Before a node's first publication the owner
chooses:

- which projects to leave out, and
- whether to share everything or only chats from the join date onward.

Left-out history is published as private, not withheld, so publication stays
exact. The choice is stored as the person's project rules and a start date.

### Scan and hold

- The server scans a chat when it arrives and when it gains messages, using
  the existing credential detector plus workspace rules.
- Credentials keep being masked as they are today.
- Other findings mark the chat held and notify its owner.
- Scanning is best-effort and must be described that way. It limits who can
  see a chat. It does not stop the chat from reaching the server.

### Reading and search

- Every shared read path filters by visibility: search, chat lists, `view`,
  records and attachments. A missed path leaks private chats, so this filter
  belongs in one place that all read routes go through, with a test per route.
- Qdrant payloads gain person, machine and visibility fields with filter
  indexes. This is a targeted payload update and does not recompute vectors.
- Search results and chat lists carry the owner. New filters: person and
  visibility.
- Agents reach team history through MCP. Other people's transcripts are
  untrusted input to an agent, more so than one's own. MCP tools take a
  `scope` (mine, team memory, team), and the default should not include
  teammates' full transcripts.
- A teammate's chat can be read and exported. It cannot be resumed, so the
  session ID is not offered.

### Storage

- Each node's shared silver on the server gains the owning person.
- The server gains tables for people, project rules, per-chat overrides and
  held findings. These are not part of the silver schema that local and shared
  databases have in common.
- Local silver does not change.

### Dashboard

- `mash dash` stays local. Team pages read through the shared API and show the
  owner's own data when the server is unreachable.
- Setting visibility is a write from the browser to the shared API. It needs
  new typed, allowlisted mutations, separate from history routes, which stay
  read-only.
- Visibility cannot be changed while the server is unreachable.
- Services and fleet actions stay scoped to the user's own machines. A
  teammate's machine appears read-only. Workspace-wide fleet control for admins
  is out of scope.

## Rules in AGENTS.md that would change

- "one shared index per configured node group" stays, with the group named a
  workspace and nodes attributed to people.
- "shared publishes exactly the local index" stays.
- Dashboard mutation rules gain an allowlisted class for visibility.
- The project's single-user stance ("one-user project") is replaced.
- Unchanged: Rusqlite only, common Qdrant engine, silver before views, no
  provider traits, no SSH fan-out, read-only access to upstream agent files.

## Phases

1. **Attribution.** Person registry and node-to-person mapping. Owner on chats,
   records and search results. Read-only team view of what is already
   published.
2. **Private flag.** Project rules, per-chat override and the visibility filter
   on every shared read path. The join-time choice.
3. **Scan and hold.** Server-side findings, held state and owner review.
4. **Workspace management.** Roles, invites, token rotation and member removal
   without a restart.
5. **Finer access.** Per-project read access, for example by mirroring
   repository access.

Phases 1 and 2 must ship together before a real team uses it. Phase 1 alone
exposes everything a node publishes.

## Set aside

- **Private chats that never leave the machine.** See "Chosen approach".
- **Memory only**, where a chat's records are shared but its messages are not.
  It can return later as a third visibility value.
- **Redacted copies** beyond credential masking. A held chat is shared or kept
  private as a whole.

## Open questions

- Should there be an escape hatch for chats that must not reach the server at
  all? Today the only one is not indexing that project, or `forget`.
- Does making a chat private hide memory that already quotes it, and what
  about what a teammate's agent already read?
- Who owns a chat started by an automation on a shared build machine?
- Should the server keep an access log of who read which chat? It helps trust
  and raises its own privacy question.
- When someone leaves, do their shared chats stay, transfer or expire?
- Is the server self-hosted only, as today, or also offered as a service?
- How are worktree and scratch directories folded into their parent project, so
  one visibility rule covers them?
