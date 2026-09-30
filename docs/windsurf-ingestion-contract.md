# Windsurf / Devin Desktop native Cascade evidence

Status: bounded typed native ingestion and provider routing are implemented,
using verified first-party reader/writer fixtures. This evidence does not
establish actual UI-session ingestion, native resume/rewind behavior, complete
tool/subtrajectory reconstruction, or current production deployment.

## Pinned first-party package

The [official Linux updater](https://windsurf.com/editor/update-linux) links to
the download redirect at
`https://windsurf.com/api/windsurf/download-redirect?build=linux-x64&isNext=false`.
On 2026-09-30 that redirect selected
[Devin Linux x64 3.10.31](https://windsurf-stable.codeiumdata.com/linux-x64/stable/b98cc43128712ba73c60cca73876f58a710aaa27/Devin-linux-x64-3.10.31.tar.gz).
Downloaded archive SHA-256:
`b3ddf1098c11255354d60a0778d7033989eaed85a37bbd55df90167f42eb5346`.
This is a locally measured artifact checksum, not a publisher signature.

The archive still bundles `resources/app/extensions/windsurf/dist/extension.js`
and `extensions/windsurf/bin/language_server_linux_x64`. Its `product.json`
names the application `devin-desktop`, with `.devin` as the data folder and
`.windsurf` as the old data folder. Product user-data names alone do not prove
the language server's Cascade persistence root.

The bundled extension's `WindsurfExtensionMetadata.codeiumDirPathSegments`
returns `.codeium/windsurf` for stable builds, with distinct insiders, next and
airgap variants. Its language-server launcher passes those joined segments as
`--codeium_dir`. The native CLI documents that flag separately from
`--database_dir`, which the extension uses for its code-search database.
An absolute `--codeium_dir` override was exercised by the native runtime probe;
the server loaded its `cascade/<trajectory_id>.pb` file successfully.

## Native persistence boundary

The stripped Go language-server executable retains its Go function-name table.
Inspecting that table and targeted disassembly establishes this call path:

- `cortex.(*CascadeManager).asyncLoadCascadeTrajectoriesFromDisk.func1`
  calls `cortex/proto_saver.ProtoSaver[...].LoadAll`.
- The saver specializes for `cortex_pb.CortexTrajectory`, as well as separate
  implicit trajectories and memories. Those other collections are not Cascade
  conversation histories.
- `language_server.initServers` joins a `cascade` directory and configures a
  `WithEncryptionKey` option with a static 32-byte value embedded in this public
  package. The `implicit` saver is configured separately.
- `ProtoSaver.Save` marshals protobuf, checks for a 32-byte encryption key,
  constructs AES and GCM, generates a nonce using `crypto/rand.Read`, seals with
  empty associated data, and prefixes the nonce to the sealed bytes.
- Saving uses a temporary identity, writes a file, and renames it to the final
  path. The reader must not discover temporary files as independent sessions.
- `ProtoSaver.Load` constructs AES/GCM before protobuf unmarshal when encryption
  is configured. It requests a `.pb` path; the path helper combines identity
  and extension using `%s%s`.
- `getSavedPaths` reads the directory, skips directories, and selects names
  ending in `.pb`. `SetArchived` writes or removes a `.pb.archived` companion
  marker. An archived history remains in its original `.pb` file; the marker is
  not a second conversation. Both behaviors were checked against the native
  runtime, which reports `isArchived: true` and keeps the conversation available.

This is an internal format rather than a documented storage API. A reader needs
authenticated decryption, bounded protobuf decoding, exact identity checks, and
revision handling. Reading these files as JSON or unencrypted protobuf would
lose the native boundary.

## Typed schema evidence

Webpack factory `99124` in the first-party extension exports native protobuf
constructors. Executing that factory and its protobuf dependencies in isolation
does not start the extension or its language server. The fixture below was
serialized and decoded through these constructors.

| Message | Relevant protobuf fields |
| --- | --- |
| `CortexTrajectory` | `trajectory_id` 1, `steps` 2, `trajectory_type` 4, `parent_references` 5, `cascade_id` 6, `metadata` 7, `source` 8, `renamed_title` 10. |
| `CortexTrajectoryStep` | `type` 1, `status` 4, `metadata` 5, `subtrajectory` 6, `user_input` 19, `planner_response` 20, `subtrajectories` 51. |
| `CortexStepUserInput` | `query` 1, `user_response` 2, `items` 3, `active_user_state` 4, `images` 5. |
| `CortexStepPlannerResponse` | `response` 1, `thinking` 3, `message_id` 6, `tool_calls` 7, `modified_response` 8. |
| `CortexTrajectoryMetadata` | `workspaces` 1, `created_at` 2. |
| `CortexStepMetadata` | `created_at` 1, `source` 3, `cortex_request_source` 19, `source_trajectory_step_info` 20. |

Step types distinguish `USER_INPUT = 14` from `PLANNER_RESPONSE = 15`.
Trajectory types distinguish `CASCADE = 4` from implicit, checkpoint,
background-research, auto-Cascade and other internal runs. Statuses include
`DONE = 3`, `INVALID = 4`, `CLEARED = 5`, `CANCELED = 6`, and `ERROR = 7`.
Their UI visibility semantics must be traced before assigning searchable roles.
Neither a user-input payload nor a familiar text string alone proves human
provenance. Parent/subtrajectory and copied-step metadata also need inspection.

## Serialization fixture and limits of evidence

[The fixture](fixtures/windsurf-native-cascade-3.10.31.json) contains a synthetic
cBioPortal trajectory, two identical requests at distinct step positions,
visible responses, thinking-only content, and a cleared step. It records the
first-party schema's relevant enum values, serialized protobuf bytes and hash,
and first-party JSON roundtrip output. Constructor encode/decode/re-encode
produced byte-identical protobuf.

Its encrypted envelope was reconstructed with Node AES-256-GCM from the observed
native saver layout and roundtripped independently. It was **not** written by
the native Go saver, and the fixture is **not** a recorded UI conversation.
It supplies a wire-schema probe; it does not yet define expected search results.
Step identities across rewinds, UI visibility, workspace metadata, and all
local/shared ingestion paths remain validation gates tracked in Beads.

The adapter reads saved user-query and planner-response fields, retaining
thinking/modified-response/user-response context separately from search prose.
Root trajectory type/source, copied-step metadata and step source determine
provenance. Unknown values remain in the typed protobuf boundary and receive no
search projection. Native step position establishes message identity; normalized
ordinals remain contiguous after omitting non-dialogue tool steps. Workspace
and Git-root file URIs are accepted only when they resolve to one unique path.
The reader authenticates before decoding and preflights known nested messages
before prost allocates repeated containers. No provider process or network call
is part of ingestion.

## First-party runtime interoperability

[The runtime fixture](fixtures/windsurf-native-runtime-3.10.31.json) records
results from the pinned language-server executable, running against only
synthetic histories. Filesystem and network namespaces isolated its home
directory and network. No credentials, inference, real user histories, or
installed background services were involved. The temporary server was stopped
after the probe.

`GetAllCascadeTrajectories` decrypted and loaded the constructor fixture,
reporting its six steps and native trajectory identity. `ArchiveCascadeTrajectory`
created a zero-byte marker and preserved the history. `RenameCascadeTrajectory`
then exercised the native saver: its new encrypted `.pb` was captured,
authenticated/decrypted independently, and decoded with the first-party
protobuf constructor. The title changed to `Native writer probe`; all six
step payloads remained semantically identical. The runtime fixture includes
that native-written envelope, plaintext protobuf, hashes, and decoded record.
The runtime capture uses a random nonce and is not byte-deterministic.

The misleadingly named `GetCascadeTranscriptForTrajectoryId` request uses
`cascade_id`, not `trajectory_id`; the exercised lookup accepted the stored
trajectory identity. Its transcript included both identical user requests and
visible assistant responses, excluded thinking-only prose, and **included**
the `CLEARED` user-input step. The fixture's literal sentence "Cleared request
must not be searchable" is synthetic test content, not an established provider
visibility rule. Filtering all cleared statuses would contradict this native
transcript evidence. UI rendering and provenance need their own verification;
the transcript helper alone does not prove human origin for every step payload.

The [official Cascade hook documentation](https://docs.devin.ai/desktop/cascade/hooks)
also describes JSONL transcripts. That hook-generated representation is a
separate boundary and must not be presented as support for existing native
Cascade files. Current Devin ACP agent histories likewise require separate
format evidence rather than assuming every session is a Cascade trajectory.
