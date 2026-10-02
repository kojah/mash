# Cline ingestion contract

Status: typed native UI and SDK/catalog ingestion implemented; production
completion is still tracked by `mash-c0d.1.8`. Mixed legacy-to-SDK reconstruction
is implemented; recorded upstream resume/compaction validation remains open. This
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

The extension preserves the legacy task ID
when adopting SDK history and merges the legacy UI prefix with SDK messages
after a resume-warning boundary. See
[sdk-task-history.ts](https://github.com/cline/cline/blob/3435f72fcf4cb843bee946b8f9e981683564c9e3/apps/vscode/src/sdk/sdk-task-history.ts).
Mash reconstructs that combined history when both roots are configured, for
catalog-backed and direct SDK artifacts. The legacy UI is a required watched
dependency; both local ingestion and shared publication receive the complete
source descriptor. A visible native user warning selects the boundary; absent
that marker, SDK history alone is authoritative. Multiple same-kind histories
remain an explicit ambiguity error. Discovery verification detects dependency
changes even when the SDK source identity stays unchanged.

Source-based fixtures cover repeated turns, regenerated SDK IDs/timestamps,
compaction sidecars, interrupted writes and rewinds. Real-model searches exercise
legacy user requests and feedback after migration. These tests use native
schema fixtures; actual upstream resume/compaction fixtures are still needed
to prove the full production gate. Do not close the provider task on the
strength of parser fixtures alone.

## First-party helper capture

`tools/provider-controls/cline-capture.ts` executes unmodified helpers at the
pinned commit above with Bun 1.4.2, zod 4.3.6 and nanoid 5.1.7. It calls native
legacy API migration and UI-prefix merge, SDK manifest/message persistence,
compaction state creation/persistence/readback and context projection. Synthetic
offline inputs include a legacy turn removed from model replay, two identical
new user requests and a fixed compaction-only canary. It does not call inference.

The v2 capture also executes native `sdkMessagesToClineMessages` and the native
legacy-prefix merge. Its 17 captured artifacts include SDK-only and merged
display histories at each phase. Replaying the merged display rows exposed
native `<user_input>` wrappers in visible user text; UI and SDK ingestion now
share the same visible-prose projection while preserving canonical envelopes.

The capture records hashes of the Bun executable and each installed dependency
tree (a lexically sorted JSON list of relative filenames and file SHA-256 values),
alongside hashes of primary upstream source modules. The clean checkout commit
pins the remaining first-party imports. Dependency installation runs no lifecycle
scripts and does not modify the upstream checkout.

Frozen native writer outputs and their SHA-256 inventory are under
`app/ingestion/cline/tests/fixtures/native-helper`. The initial, compacted and
regenerated snapshots exercise different upstream-generated SDK IDs and explicitly
regenerated IDs/timestamps. Capture timestamps and random IDs are volatile on
recapture; artifact hashes identify the checked-in capture, not a promise of
byte-identical reruns. Canonical message preservation, visible search projections,
distinct repeated turns and stable mash identities are the replay invariants.
Native user-input envelopes remain in canonical viewing while search uses the
unwrapped projection. Compaction sidecars do not replace canonical history.

To recapture, supply a clean checkout at the pinned commit and an isolated Bun
dependency directory with the exact zod/nanoid versions above:

```bash
bun --no-install tools/provider-controls/cline-capture.ts \
  /path/to/pinned/cline /path/to/isolated/dependencies /path/to/fresh/output
```

The harness builds a sealed Bun bundle, resolving required shared exports
directly to original upstream modules and zod/nanoid to the isolated dependency
directory. Runtime package auto-installation is disabled. Bundle and harness
hashes accompany the source/runtime/dependency inventory. Host and workspace
ports and model-error classification are guarded substitutions that fail if
used; the matching promotion-error constant is supplied by the harness.
Those branches are not evidence of native host behavior. Its catalog adapter
remains an in-memory test double. The actual extension renderer and native
session/catalog lifecycle are not executed. The compaction projection is supplied
offline rather than generated by a model. This is first-party **helper execution**,
not an end-to-end extension/session capture. It strengthens schema, preservation
and refresh evidence without satisfying the outstanding full resume/compaction,
catalog and provider completion gates in `mash-c0d.1.8`.

Schema 35 repairs existing nonempty Cline user search projections transactionally.
It preserves canonical text and identities, invalidates only changed message
vectors and Cline checkpoints, and retains already excluded empty projections.
Changed projections require explicit `mash index` before semantic search becomes
ready again. Older schemas retain their broader existing reindex requirement.
Rollback/retry and preservation of unrelated vectors are covered by storage
migration tests; search itself never upgrades the database.

The 2026-10-02 capture passed mash's actual CLI indexing with the existing local
model installation: 10 messages/10 stored embeddings initially, then the same
message identities and canonical text with zero new embedding writes after
compaction and ID/timestamp regeneration. The Rust replay fixture checks each
captured file's hash, reopens SQLite between refreshes, verifies the complete
visible projection and preserves the legacy subject omitted from model replay.
Logs and CLI comparison records remain outside Git under
`/home/james/scratch/mash-artifacts/cline-native-helper-final-20261002` and the
`cline-native-helper-*-20261002.log` captures. An early comparison incorrectly
compared visible text with canonical envelopes; the corrected comparison checks
search projections and canonical preservation separately.

The v2 native-translator CLI control additionally indexed the merged display
history with 10 messages/vectors. A simulated schema-34 index retained canonical
envelopes but old wrapped user projections: search refused it without changing
the schema, explicit indexing repaired two user projections/vectors, and the
other eight vectors remained byte-identical. Search succeeded afterward.
SDK compaction/regeneration refreshes retained all identities, canonical text,
projections and vectors with zero new embedding writes. Records are under
`/home/james/scratch/mash-artifacts/cline-translator-cli-final-20261002/results.json`.
The first audit attempt incorrectly expected a failed CLI's error JSON on
stdout; its retained log records that harness failure. The corrected audit
reads errors from stderr. These are offline synthetic first-party controls,
not a real extension-session or fleet capture.
