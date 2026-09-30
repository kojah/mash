# Native Antigravity ingestion evidence

Status: native storage and user-message persistence verified; adapter not yet
implemented. Assistant persistence, resume/rewind, older formats, and captured
real sessions remain validation gates.

## First-party pins

The [official download page](https://antigravity.google/download) distributes
distinct IDE, Hub, and CLI products. Do not assume their storage paths or schema
versions are interchangeable.

- IDE 2.5.5, release `2.5.5-4923483625488384`, Linux x64:
  [official archive](https://edgedl.me.gvt1.com/edgedl/release2/j0qc3/antigravity/stable/2.5.5-4923483625488384/linux-x64/Antigravity%20IDE.tar.gz).
  SHA-256: `0c5233b297d2b3aebb61af49f8944012c2953d361a5ebb16978490636917f831`.
- The official [CLI installer](https://antigravity.google/cli/install.sh) resolves
  platform `linux_amd64` through
  `https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/linux_amd64.json`.
  On September 30, 2026 it returned CLI **1.2.14**, newer than the download page's
  1.2.11 label. The archive
  `https://storage.googleapis.com/antigravity-public/antigravity-cli/1.2.14-4571742832820224/linux-x64/cli_linux_x64.tar.gz`
  matched the manifest's SHA-512:
  `fd771dfc74ddd07b61c8b0a6fd7a238f53a3a098a51052583a01d97ab84ee60db741ce5f041e87a9da3c1a9aabe95b053113374437df1e231773552781edaf09`.
  The installer was read, not executed. CLI storage is not yet runtime-verified.
- The [official SDK source](https://github.com/google-antigravity/antigravity-sdk-python/tree/12f9a4c3becf487302dc799b0f59054f01f3ddb9)
  is pinned for subsequent comparison. It describes a bundled compiled runtime;
  Python-side session state alone does not establish the native disk format.

## Native IDE runtime capture

[The runtime fixture](fixtures/antigravity-native-runtime-2.5.5.json) contains the
exact table definitions and rows written by the unmodified IDE language server.
It also includes selected first-party protobuf descriptors and the native HTTP
response for the two saved user messages. Prompts are synthetic; this is native
writer evidence, not a captured real-user IDE conversation.

The probe ran in user, mount, and network namespaces. A disposable directory was
mounted over the user's home, only loopback networking was available, telemetry
and browser integration were disabled, and backend endpoints pointed at closed
loopback ports. Its own process was stopped and waited on before reading SQLite.
No application was installed, and no model inference or external API was used.

The server's `--help` establishes `--gemini_dir` (default `.gemini`) and
`--app_data_dir` (default `antigravity-ide`). With an explicit disposable Gemini
directory, `StartCascade` wrote:

```text
<gemini_dir>/antigravity-ide/conversations/<cascade_id>.db
```

The request used `cascadeId=50000000-0000-4000-8000-000000000001`, cascade type,
and cascade-client source. The database assigned a **different trajectory ID**.
`SendUserCascadeMessage` with `items: [{text: "Explain cBioPortal mutation
filtering and sample selection."}]` saved a user step before execution failed
because no model was configured. Repeating the same request saved a second
distinct user step. A subsequent `plannerResponse` injection request produced
no assistant step; it does not validate assistant persistence.

## Verified storage boundary

- `trajectory_meta` stores trajectory ID, cascade ID, type, and source. Validate
  filename against cascade ID; do not require equality with trajectory ID.
- `trajectory_metadata_blob`, key `main`, stores serialized
  `CortexTrajectoryMetadata`. Native creation saved its timestamp and cascade ID.
- `steps` uses `idx` as primary key. Native user rows have indexes **0 and 2**,
  with execution-error steps between them. Preserve native step indexes in
  message identities and use separate contiguous normalized ordinals.
- For the captured rows, `step_format=0` and `step_payload` contains the complete
  `gemini_coder.Step` protobuf, including type/status/metadata and its payload
  oneof. Do not decode it as `CortexStepUserInput` directly.
- `metadata` is also stored separately. Step type and status appear in both SQL
  columns and the complete protobuf. The adapter must validate their agreement.
- Auxiliary tables hold generator metadata, executor metadata, parent references,
  and battle-mode information. Their presence does not make them dialogue.
- Native user input uses repeated `TextOrScopeItem` field **3** (`text` field 1
  or scoped context field 2). Legacy `query` field 1 is deprecated. In this
  runtime capture, `user_response` and `items.text` contain the same prompt;
  concatenating both would duplicate searchable text.
- Native user steps have `CortexStepSource=USER_EXPLICIT` (4). Their
  `source_trajectory_step_info` is present and points at their **own trajectory**.
  Presence alone therefore cannot be treated as copied-history provenance.
- Assistant descriptors expose response, modified response, thinking, and raw
  thinking separately. Only verified displayed prose should be searchable;
  reasoning, tool payloads, and generated context must stay out of embeddings.

The binary also contains both `ProtoStore` and `SqliteStore` implementations.
This proves multiple storage paths exist in the package, not which historic
versions used them. Do not alias the older format to Windsurf without evidence.

## Remaining gates

1. Verify native assistant writes and scoped-input display behavior without
   treating synthetic model text as real user provenance.
2. Verify native resume, forks, truncation, copied-step references, archive, and
   subtrajectory behavior, including whether step indexes survive mutation.
3. Implement a bounded read-only SQLite adapter with WAL-aware checkpoints,
   stable conversation/message identities, strict schema/payload validation,
   and rollback preserving prior search results after malformed updates.
4. Wire the provider enum, independent index allowlists, discovery and watch
   dependencies, SQLite/PG migrations, Redis tags, and typed HTTP publication.
5. Test local lexical/semantic/thread retrieval with real installed models and
   publication/search against both shared backends.
6. Capture real IDE/CLI/Hub histories and verify supported version boundaries
   before claiming general Antigravity support or production deployment.
