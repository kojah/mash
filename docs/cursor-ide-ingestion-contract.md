# Cursor IDE native ingestion contract

Status: first-party desktop storage and serializers inspected; a deterministic
native serialization fixture is captured. The IDE reader remains unimplemented.
This format is separate from the implemented [Agent CLI reader](cursor-ingestion-contract.md).

## Pinned evidence

The official [Linux package index](https://downloads.cursor.com/aptrepo/dists/stable/main/binary-amd64/Packages)
selected desktop `3.22.12-1790400601`. Its
[Debian package](https://downloads.cursor.com/aptrepo/pool/stable/c/cu/cursor_3.22.12_amd64.deb)
has SHA-256 `8b9198f7dde79f7e27cc83dabf3c66a93ca5303b6919719100c82f29d9498a65`,
matching the index. `resources/app/product.json` identifies commit
`3a92974361033b2051526321308c2740fe5912c0` and build date
`2026-09-26T04:55:12.125Z`. These are internal formats, not a published storage API.

Relevant first-party code is in `resources/app/out/main.js` and
`resources/app/out/vs/workbench/workbench.desktop.main.js`. The inspected bundled
modules include:

- `storage.js`: application/profile storage and SQLite KV access.
- `composerData.js`: native header identity and display/provenance hints.
- `composerDataHandle.js`: root persistence and message ownership.
- `composerMessageStorageService.js`: bubble persistence and checkpoint hydration.
- `composerBlobStore.js`: native binary blobs and checkpoint pointers.
- `composerDraftRecord.js`: separate unsent draft persistence.
- Native composer serialization helpers `A_u`, `R_u`, `eUs`, `dvt` and `zFi`.

Names refer to this pinned bundle; minified symbol names may change next release.

## Physical storage

The main process names its SQLite storage `state.vscdb`. Application storage uses
the default profile; renderer `cursorDiskKV` operations route to application
storage rather than the workspace index. A workspace's visible composer list
therefore does not establish ownership of all stored conversations. Configured
roots must accommodate user-data/profile overrides; do not infer them from the
Agent CLI's `CURSOR_CONFIG_DIR`.

The native SQLite opener sets `user_version = 1`, WAL, synchronous NORMAL and
automatic checkpoints. It creates these relevant tables, in addition to unrelated
collection tables:

| Table | Columns | Purpose |
| --- | --- | --- |
| `ItemTable` | `key TEXT UNIQUE ON CONFLICT REPLACE`, `value BLOB` | UI, settings and composer lists. |
| `cursorDiskKV` | `key TEXT UNIQUE ON CONFLICT REPLACE`, `value BLOB` | Canonical composer roots, bubbles and agent blobs. |

The declared BLOB affinity does not imply every value is binary. Native
`cursorDiskKVSet` binds strings directly and `cursorDiskKVGet` accepts only string
values. Binary access binds/returns buffers. A reader must distinguish SQLite
TEXT and BLOB without silently coercing malformed values.

Agent CLI `meta`/`blobs` tables are a different physical layout despite the same
SQLite version. Detect the supported schema, rather than relying on that version
alone or treating every `.db` file as one format. Read through the existing
read-only snapshot boundary and track WAL changes.

## Conversation ownership and order

`composerData:<composerId>` stores JSON. Its native `composerId` must agree with
the key. The pinned serializer writes `_v = 18`; older migration variants need
independent fixtures before claiming support.

`fullConversationHeadersOnly` contains ordered bubble identities and message
types. `bubbleId:<composerId>:<bubbleId>` stores each serialized message. The
writer persists eligible bubbles before the root; it can retain bubbles that are
no longer present in the current headers. Follow the current root's ordered
headers rather than scanning the bubble prefix into search results. Missing
referenced payloads must fail indexing and preserve the previous revision.

The serializer and message writer both omit `isDisplayOnly` rows. Cloud streaming
roots additionally filter headers using server bubble IDs and the saved kickoff
boundary. The reader must honor the saved headers without reconstructing excluded
rows from leftover payloads or a preview list.

Native message types are `HUMAN = 1`, `AI = 2`, and `UNSPECIFIED = 0`.
Bubble IDs distinguish repeated identical requests; text deduplication would lose
real turns. Native timestamps have separate forms: bubble `createdAt` is an ISO
string in the fixture; root creation/update values are millisecond numbers.
Absent timestamps must remain absent rather than being fabricated from root time.

## Visibility and metadata boundaries

Bubble `text` contains visible prose, but message type alone is insufficient to
establish human provenance. Native fields include `isSimulatedMsg`,
`simulatedMsgReason`, `simulatedMessageMetadata.suppressUIDisplay`,
`isPlaceholder`, `serviceStatusUpdate`, `eventNotification`, `thinking`,
`capabilityType`, and `toolFormerData`. These need typed models and explicit
normalization. Tools/reasoning and synthetic operational messages are context,
not human search evidence. Unknown variants must not be guessed into prose.

Root `text`/`richText` and `composerDraft:<composerId>` can hold unsent input.
`latestConversationSummary` is model context. Neither belongs in message search.
Native subagent metadata also needs conservative visibility handling.

Root `workspaceIdentifier`, native state workspace URIs, `trackedGitRepos` and
worktree metadata can supply project/repository evidence. Resolve only unambiguous
native paths; do not derive repositories from files read by the model. Encryption
keys and other configuration fields must never enter canonical content, titles,
errors or search projections.

## Native agent state

Serialized `conversationState` is the protobuf
`agent.v1.ConversationStateStructure`. Modern strings use `~` followed by base64;
the native hydration code also accepts bare hex. It has the same selected root
tags as the Agent CLI contract, including prompt messages, turns, archives,
workspace URIs, tracked repositories and start time.

`ComposerBlobStore` addresses binary values with `agentKv:blob:<hex-id>` and
supports a legacy hex-string fallback. `agentKv:checkpoint:<composerId>` and
`agentKv:bubbleCheckpoint:<composerId>:<bubbleId>` hold hex pointers. A one-byte
zero blob is the native empty-byte sentinel. A bubble without inline state may
hydrate through its bubble checkpoint; absent state/pointer yields an empty
native state. This model-input/checkpoint graph must not automatically replace
the visible bubble history.

## Fixture and verification limits

[The fixture](fixtures/cursor-ide-native-v18.json) contains eight KV rows generated
by isolated desktop serializer and `storeMessage` code with synthetic cBioPortal
input. Six bubbles are reachable in saved header order, including two identical
requests with distinct native IDs. It includes simulated input, a tool row,
reasoning, unsent draft/summary metadata and an abandoned branch bubble. A
display-only input is omitted by the native writer; image data is stripped by
native serialization.

The exact desktop root descriptor was executed using the pinned first-party
Agent package's protobuf runtime and referenced child types. Field numbers/names
were checked against that runtime; inline root bytes were decoded back to verify
the workspace URI. Clock, UUID and random-key generation were deterministic;
logging, metrics and KV persistence were isolated test boundaries. No desktop
startup, login, model call or real user session was executed.

The emitted KV writes were inserted into a disposable SQLite database using the
inspected native table definitions. Integrity, TEXT storage, header order,
distinct repeated IDs and the unreachable branch row were checked. This validates
serialization and the relevant SQL layout, not the full desktop database opener,
real editing/compaction, cloud hydration or installed-version compatibility.

Remaining work: implement bounded typed IDE ingestion; test local/shared routing,
project filters and real-model retrieval; capture actual IDE edit/resume/compaction
and subagent histories; validate deployment. The parent Cursor task stays open.
