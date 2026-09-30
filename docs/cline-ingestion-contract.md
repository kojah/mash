# Cline ingestion contract

Status: typed native UI and SDK/catalog ingestion implemented; production
completion is still tracked by `mash-c0d.1.8`. Mixed legacy-to-SDK migration
reconstruction and upstream resume/compaction fixtures remain open. This
contract covers both native extension history and current SDK/CLI history.

## Evidence

Sources below are pinned to Cline commit
`3435f72fcf4cb843bee946b8f9e981683564c9e3`, inspected on 2026-09-30.
Links refer to first-party source, not inferred filesystem formats.

| Boundary | Source |
| --- | --- |
| Extension task files and storage root | [disk.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/core/storage/disk.ts) |
| Extension UI record vocabulary and identity | [ExtensionMessage.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/shared/ExtensionMessage.ts) |
| Existing native histories and removed UI status types | [legacy-state-reader.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/legacy-state-reader.ts) |
| Extension history metadata | [HistoryItem.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/shared/HistoryItem.ts) |
| Legacy resume and UI merge semantics | [legacy-task-handling.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/legacy-task-handling.ts) |
| SDK directory configuration | [paths.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/shared/src/storage/paths.ts) |
| SDK session catalog | [sqlite-session-store.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/services/storage/sqlite-session-store.ts) |
| SDK artifact filenames | [session-artifacts.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/services/session-artifacts.ts) |
| SDK manifest schema | [session-manifest.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/session/models/session-manifest.ts) |
| SDK message envelope | [session-data.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/services/session-data.ts) |
| SDK message blocks | [messages.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/shared/src/llms/messages.ts) |
| Artifact writes and compaction separation | [session-manifest-store.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/session/stores/session-manifest-store.ts) |
| Canonical versus display-only history | [conversation-store.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/session/stores/conversation-store.ts) |
| Compaction boundaries and volatile message identities | [session-compaction.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/session/models/session-compaction.ts) |
| Display projection of tool metadata | [display-messages.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/session/display-messages.ts) |

## Native extension histories

The extension resolves task files beneath its host-provided global storage root,
under `tasks/<task-id>/`. Do not equate that root with `~/.cline`: the constant
for the Cline home directory in `disk.ts` also serves unrelated configuration.
The native legacy-state reader accepts a data directory and locates task history
metadata at `state/taskHistory.json`.

`ui_messages.json` is an array of UI records. Each record has numeric `ts`,
`type` (`ask` or `say`), optional `ask`/`say` subtype, `text`, and `partial`.
Optional `seq` indicates a newer copy of the same timestamp identity; `epoch`
is a webview freshness fence. They are not interchangeable with an independent
message ID. Media, files, reasoning and bookkeeping fields exist too.

The task's UI stream distinguishes `say: task` and `say: user_feedback` from
assistant `say: text` and completion responses. An `ask` is an assistant prompt
or approval request, not a user turn. Some ask payloads are structured JSON;
their displayed text must be decoded using that subtype's actual schema.
Unknown or operational subtypes must not become searchable prose merely because
they have a text field. Upstream excludes the old `error_retry` and
`api_req_retried` UI status types when reading legacy history.

`api_conversation_history.json` is model input history, not an equivalent UI
transcript. Native legacy truncation used a persisted deleted range to omit
model context while keeping the full file. That range is not proof that the
corresponding visible conversation was deleted. Prefer the UI history for
visible search; do not merge both files indiscriminately.

`task_metadata.json` records files in context, model usage and environment
history. Its inspected schema does not supply a project root or repository ID.
`HistoryItem.cwdOnTaskInitialization` is an explicit project-directory candidate
in the native task catalog. Do not derive a repository from arbitrary files
read by the model or from the task ID.

## Native SDK/CLI histories

Default SDK configuration resolves `~/.cline/data/db/sessions.db` for the
catalog. `CLINE_DIR`, `CLINE_DATA_DIR` and `CLINE_DB_DATA_DIR` can change it.
The store accepts an explicit directory as well. A session row includes
`session_id`, `cwd`, `workspace_root`, `messages_path`, timestamps, parent and
agent identities, subagent status, and metadata. The transcript is a separate
artifact; searching the database alone would miss the actual messages.

`SessionArtifacts` uses its supplied directory to construct
`<session-id>/<session-id>.json` manifests and
`<session-id>/<session-id>.messages.json` message artifacts. Child messages can
live in the root session's directory with agent or team-task file stems.
Do not hard-code the separate `resolveSessionDataDir()` path as the store's
artifact root: the default SQLite store supplies its database data directory
to the artifact writer. Honor explicit configured roots and recorded paths.

A version-1 message artifact contains `sessionId`, `updated_at`, `agent`
(`lead`, `subagent`, `teammate`), `origin`, `messages`, optional `taskType`, and
optional `system_prompt`. Origin distinguishes source, user mode and parent or
subagent identity. A version-1 manifest has `session_id`, `workspace_root`,
`cwd`, timestamps, provider/model settings and optional metadata/title and
`messages_path`. Validate agreement of identities across dependent artifacts.

Messages have user/assistant roles and either a string or typed block array.
Block types include text, file, image, media, tool use, tool result, thinking,
and redacted thinking. Search the visible text projection. Preserve necessary
tool context for viewing without embedding tool output, attached-file bodies,
binary data or reasoning as conversational prose. `system_prompt` is model
configuration, not a user request. Display-only messages can represent visible
history and must not be discarded solely for being excluded from model replay.

Canonical messages and compaction state are separate. Compaction sidecars
contain a model-context projection and source-boundary evidence. The native
reader explicitly treats the sidecar as disposable without changing canonical
history. Index canonical history; do not replace it with the sidecar summary.

Native compaction source hashing deliberately excludes message IDs and `ts`
because storage/wire round trips can regenerate them. Consequently, stable
mash identities and refresh tests cannot assume those fields remain unchanged.
Repeated identical real turns still need separate identities; content-only
deduplication would lose history.

## Implementation and verification gates

Use one concrete Cline adapter with typed native models for the two stores,
shared enum-based routing, independent index allowlists, and the existing
blocking ingestion boundary. Keep the CLI and MCP as wrappers around the core.
All reads must preserve upstream files. Catalog reads use Rusqlite read-only
transactions and include WAL changes in source verification.

Discovery and publication must track transcript and metadata dependencies,
including optional metadata appearing or disappearing. Explicit transcript
roots need the same dependency watching as directory roots. Malformed or
changing required snapshots must fail the source without replacing the prior
indexed revision or advancing its checkpoint. Document bounded input and
aggregate parsing limits; byte limits alone do not bound typed allocations.

Required tests before closing `mash-c0d.1.8`:

1. Extension task, feedback, assistant prose, typed ask payloads, partial/final
   updates, unknown subtypes, duplicate timestamp copies and missing metadata.
2. SDK string and block content, root and child identities, display-only turns,
   model/tool noise exclusion, and catalog/manifest/transcript disagreement.
3. Real upstream resume/compaction fixtures proving original visible turns stay
   searchable, regenerated IDs do not duplicate turns, and identical repeated
   user requests remain distinct.
4. Interrupted JSON writes, malformed input, resource bounds, and mutation
   between parse and publication preserving the previous valid revision.
5. Catalog WAL updates, moved/custom artifact directories, metadata changes,
   authoritative removal, and watcher invalidation for dependent paths.
6. Real-model retrieval with message and thread results, project filters,
   canonical viewing, and local/shared per-provider routing on both backends.

Implemented display handling follows native question/plan schemas, removes SDK
user-input envelopes and mode notices, and excludes synthetic resume, hook and
display-role context. Native `ask_question`/`ask_followup_question` tool calls
and paired successful user replies are projected as prose. Completion tools
`submit_and_exit` and `attempt_completion` project their summary/result, following
the upstream [message translator](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/message-translator.ts),
[synthetic-user mapping](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/sdk-user-message-mapping.ts)
and [tool definitions](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/sdk/packages/core/src/extensions/tools/definitions.ts).

Migration remains a material gap: the extension preserves the legacy task ID
when adopting SDK history and merges the legacy UI prefix with SDK messages
after a resume-warning boundary. See
[sdk-task-history.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/sdk-task-history.ts).
Mash rejects discovered legacy UI/catalog collisions explicitly until it can
reconstruct that combined history, including when SDK artifacts are configured
without a catalog. Existing tests use native
schema fixtures; actual upstream resume/compaction fixtures are still needed
to prove the full production gate. Do not close the provider task on the
strength of parser fixtures alone.
