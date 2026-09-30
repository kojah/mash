# Cursor native ingestion contract

Status: native Agent storage and serializer evidence captured; the Rust adapter
and Cursor IDE format remain unfinished in `mash-c0d.1.9`. This contract is a
prerequisite for provider implementation, not a claim of working Cursor support.

## Evidence and fixture provenance

The first-party [installer](https://cursor.com/install) selected Agent package
`2026.09.28-64d2043`. The inspected Linux x64
[package](https://downloads.cursor.com/lab/2026.09.28-64d2043/linux/x64/agent-cli-package.tar.gz)
has SHA-256 `6e4cd936a4866b8a77c50ff51a564460d715772fabc477a01aa0f0455d9559f0`.
This pin matters: native storage is an internal format, not a published stable API.

[The fixture](fixtures/cursor-agent-native-v1.json) contains deterministic,
synthetic cBioPortal conversations serialized by that package's own protobuf
classes and persisted by its native SQLite blob store. The package's native
`AgentStore` checkpoint/reset/full-conversation methods verified the current
turn. Its native transcript hydration verified two archived messages followed
by two current messages, excluding the system prompt and marked model summary.
The source SQLite artifact had `user_version = 1` and twelve blobs.

The fixture records metadata as stored hex, blob IDs and original binary payloads,
and the hydrated messages. It includes reasoning as context, a native turn/step,
an external user-text blob, and a summary archive. No login, remote inference or
real user conversation was involved. This proves serializer/storage compatibility
for these cases; it does not prove real CLI compaction or IDE behavior.

The exercised factories were loaded in isolation without running the CLI startup
entrypoint. Relevant bundled module names are:

- `./src/state/index.ts` and `../cursor-config/dist/paths.js` for native roots.
- `../cursor-sdk-local-runtime/dist/run-store/sqlite-blob-store.js` for storage.
- `../agent-kv/dist/index.js` for metadata, hashing and checkpoints.
- `../proto/dist/generated/agent/v1/agent_pb.js` for binary schemas.
- `../agent-transcript/dist/index.js` for archive/current-message hydration.

## Agent physical store

The config root is `CURSOR_CONFIG_DIR` when nonblank, otherwise
`$XDG_CONFIG_HOME/cursor` when configured, otherwise `~/.cursor`. Native chats
live below `chats/<md5-of-resolved-workspace-path>/<session-id>/store.db`.
The hash identifies the workspace directory; it is not a reversible project path.
`CURSOR_DATA_DIR` controls the separate projects/transcript location and must not
be substituted for the config/chat root.

The native writer uses SQLite WAL and these tables:

| Table | Columns | Meaning |
| --- | --- | --- |
| `meta` | `key TEXT PRIMARY KEY`, `value TEXT` | Key `0` stores hex-encoded UTF-8 JSON metadata. |
| `blobs` | `id TEXT PRIMARY KEY`, `data BLOB` | Content-addressed binary, UTF-8 or JSON payloads. |

Metadata includes `agentId`, `latestRootBlobId`, `name`, `createdAt`, mode/approval
settings and optional subagent information. `latestRootBlobId` is a hex-encoded
binary identifier inside the metadata JSON. The metadata also contains a blob
encryption key; an adapter must never expose it in titles, errors or search data.
The exercised native SQLite store writes the supplied blob bytes directly.

Read the upstream store through the existing read-only SQLite snapshot boundary.
Track the WAL and logical source identity, reject missing referenced blobs, and
bound byte lengths before fetching/decode. Enumerating every stored blob would
revive obsolete checkpoints and abandoned branches: reachability starts at the
metadata's latest root.

## Reachable history

The root is a protobuf `ConversationStateStructure`. Relevant fields are:

| Field | Tag | Encoding |
| --- | --- | --- |
| `root_prompt_messages_json` | 1 | Repeated blob identifiers |
| `turns` | 8 | Repeated native turn identifiers |
| `previous_workspace_uris` | 9 | Repeated URI strings |
| `summary_archives` | 13 | Repeated archive identifiers |
| `conversation_started_timestamp_ms` | 26 | Optional uint64 |
| `message_count_at_last_compaction` | 37 | Optional uint32 |

An archive is a protobuf `ConversationSummaryArchive`; its repeated
`summarized_messages` field (tag 1) references JSON messages. The native transcript
loader hydrates archives in order, then current root prompt messages. It omits
current system messages and messages marked `providerOptions.cursor.isSummary`.
It tolerates missing/malformed archive data and skips oversized transcript blobs;
mash must report incomplete input and preserve its previous indexed revision.

Native turn data is a separate protobuf path: a `ConversationTurnStructure`
selects an agent or shell turn. Agent turns reference `UserMessage` and ordered
`ConversationStep` blobs; steps select assistant prose, tools or thinking.
`UserMessage` has native message identity, optional simulated-message provenance,
optional external text references and optional timestamps. Neither summaries nor
simulated user prompts should automatically become human search evidence.

A Rust reader needs typed protobuf and JSON models at these distinct boundaries.
Tool-owned arguments/results may remain opaque. Search projects visible prose;
canonical viewing retains useful tool/reasoning context. Message identity must
survive repeated identical genuine requests, compaction and resumed checkpoints.
Metadata timestamps must not be invented for archived messages lacking them.

## Remaining production gates

1. Implement the bounded native Agent reader and provenance-aware normalization;
   verify root JSON/native-turn relationships using captured CLI sessions.
2. Exercise real resume, compaction, repeated requests, rewinds, simulated turns,
   missing blobs, external text, subagents, and WAL-only changes. The current
   fixture exercises the native writer and hydration functions with synthetic
   inputs rather than executing a real compaction operation.
3. Verify project/repository metadata, native text visibility and stable message
   identities without deriving a repository from arbitrary files the model read.
4. Inspect first-party Cursor IDE storage and add native IDE fixtures. Do not
   declare IDE coverage based on Agent or transcript formats.
5. Wire the provider through config, local indexing, watching, shared publication,
   both shared backends, CLI/MCP filters and explicit readiness checks. Validate
   real-model retrieval and production deployment before closing the parent task.
