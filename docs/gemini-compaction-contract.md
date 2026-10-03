# Gemini compaction recovery

The recording fixture in `fixtures/gemini-compaction-38700b4.jsonl` was emitted
by the first-party `ChatRecordingService` at commit
`38700b4b38bf387dafded6c97c3f190d084b49e9`. Its source SHA-256 is
`e8298c7e2ecee1282d2470459f3e4988ad6a3381c1bdbbcc119a61706a1fe282`.

This is an isolated recorder execution with controlled history turns, not a UI
capture or a live model-generated summary. A Node harness transformed the pinned
TypeScript, supplied filesystem/path/readline imports, and stubbed project hash,
clock, UUID, debug and content helper functions. It instantiated the real
recorder against a disposable file and invoked `recordMessage`,
`updateMessagesFromHistory`, and `rewindTo`. Native writer/replay methods were
unchanged. The probe and pinned sources are retained outside Git in
`/home/james/scratch/mash-artifacts/gemini-compaction-38700b4`.

The controlled checkpoint uses the shape produced by the pinned
[compressor](https://github.com/google-gemini/gemini-cli/blob/38700b4b38bf387dafded6c97c3f190d084b49e9/packages/core/src/context/chatCompressionService.ts):
a user summary containing a complete `<state_snapshot>`, followed by the exact
model acknowledgement `Got it. Thanks for the additional context!`, and any
retained context. The pinned
[chat implementation](https://github.com/google-gemini/gemini-cli/blob/38700b4b38bf387dafded6c97c3f190d084b49e9/packages/core/src/core/geminiChat.ts)
syncs initial context through the recorder; aborted calls roll back history
before calling the same checkpoint method. The
[recorder](https://github.com/google-gemini/gemini-cli/blob/38700b4b38bf387dafded6c97c3f190d084b49e9/packages/core/src/services/chatRecordingService.ts)
keeps the earlier append-log entries even though its loader replaces the active
messages when it sees a checkpoint.

The native loader returns six message IDs: `summary-1`, `ack-1`, `copied-answer`,
`later-user`, `later-answer`, and `final-user`. mash additionally recovers the
two genuine earlier turns, retaining their original message IDs. Search has
five evidence messages; the copied answer, summary and acknowledgement remain
context only. Cancelled and explicitly rewound turns remain absent.

Recovery requires a new summary/acknowledgement pair. Other checkpoints replace
active history normally. Removing the compaction anchor by rewind or checkpoint
also removes the archive; archived IDs cannot be used as active rewind targets.
Repeated compaction archives only directly recorded dialogue. A checkpoint that
reintroduces a recorded ID owns that message once, preserving its provenance.
Archived text shares the existing 32 MiB replay and 65,536-message budgets.

SQLite schema 31 and shared source fingerprint `search-text-v5` require an
explicit `mash index` to refresh unchanged histories. Search never performs
the migration or ingestion. Compaction uses the existing typed HTTP publication contract.

Unmarked summaries and complete JSON snapshots lack reliable event provenance.
They continue to follow resumed history; live UI/model compaction and alternate
summary formats remain validation work in Beads `mash-c0d.2.1`.

## Compressor and hot-restart execution

`fixtures/gemini-pipeline-38700b4.jsonl` strengthens the recorder-only fixture
above. At the same pinned commit, an isolated Node 24 harness executes the
first-party `ChatCompressionService.compress`, `GeminiChat` hot-restart
constructor and initialization, `AgentChatHistory`, recorder resume loader,
message/checkpoint writer and inclusive rewind. The compressor itself chooses
the split, emits the summary/acknowledgement turns and accepts or rejects the
compression; the harness does not construct those checkpoint turns by hand.

| Pinned source | SHA-256 |
| --- | --- |
| `chatCompressionService.ts` | `a278dae3a680fdb0d6072dbf821b496933a6f18d7899d43d6c5794690ca1c361` |
| `geminiChat.ts` | `446b5cd55e91b4b79273d0c4a20bdac923ba6a6bf2cebc5016c74a49519e6717` |
| `agentChatHistory.ts` | `ac2303b4ea280a0509893a9c6553728b51b1c04c37c2f0af6877538b6907bf0e` |
| `chatRecordingService.ts` | `e8298c7e2ecee1282d2470459f3e4988ad6a3381c1bdbbcc119a61706a1fe282` |

Two controlled model responses per compression replace the remote inference
boundary. Token counting, initial environment-context construction, prompts,
telemetry, hook infrastructure, tool-ID normalization, UUIDs and clock are
controlled dependencies. The fixture has prose-only content, so it does not
exercise tools, model-role scrubbing or environment injection. No live model
request, UI interaction or fleet data was used.

The harness asserts rejection of empty and inflated summaries, then performs
two successful compressions through actual hot restarts. It also records and
rolls back aborted dialogue and explicitly rewinds a later turn. The native
loader retains only `pipeline-3`, `pipeline-4` and `final-user`. mash recovers
four earlier genuine messages with their original identities; the resulting
seven canonical messages contain exactly five searchable dialogue messages.
Summary, acknowledgement, cancellation and rewind canaries remain unsearchable.

Parser and SQLite indexing contracts cover the captured bytes directly, stable
identities before/after compression, all five searchable turns and excluded
payloads. The executed harness (`pipeline-probe.mjs`), pinned sources,
native-loaded output and source hashes (`pipeline-evidence.json`) are retained
under `/home/james/scratch/mash-artifacts/gemini-compaction-38700b4`.
This closes the component-execution follow-up `mash-c0d.2.1.1`; the parent still
requires live UI/model and alternate-summary validation.

## Tool masking and content-only history replacement

`fixtures/gemini-tool-history-masked-38700b4.jsonl` and
`fixtures/gemini-tool-history-38700b4.jsonl` were emitted by the same pinned
recorder, `GeminiChat` and `AgentChatHistory`, under Node 24.5.0. The source hashes
above are checked before execution. The isolated harness invokes native thought,
message and tool-call recording, hot-restart initialization, `setHistory`,
checkpoint writing, inclusive rewind and the resume loader. It uses a controlled
header/path, project hash, clock, UUIDs, tool registry, text/token helpers and
tool-ID normalization. The content-generator boundary throws if inference is
requested. No remote model call occurs.

The first capture passes history-turn objects with retained IDs to `setHistory`.
It replaces a tool-result part and verifies the recorder also updates the
matching tool call on the preceding assistant message. Native IDs remain
`request`, `answer`, `tool-result` and `followup`. The second passes content-only
objects, as the pinned
[client's `CONTENT_TRUNCATED` recording branch](https://github.com/google-gemini/gemini-cli/blob/38700b4b38bf387dafded6c97c3f190d084b49e9/packages/core/src/core/client.ts)
does. That inspected source has SHA-256
`16fd5149220cc0c7aa2215b38253b331d0e98d863a13b0d01927a47b734a7dad`.
Native `setHistory` records four new synthetic turns and replaces the active
history with `tool-history-1` through `tool-history-4`. The native loader retains
exactly those four IDs after a cancellation checkpoint and inclusive rewind.
This executes the recording branch; the tool-output budget/truncation algorithm,
actual model compression, network cancellation and live UI are not exercised.

Both snapshots contain the same three searchable prose messages. The tool-only
user turn remains canonical context with an empty search projection. Thoughts,
tool arguments/results, cancellation and rewind canaries remain unsearchable.
The masked capture removes the superseded original tool-result payload; the
content-only replacement follows the native snapshot's loss of old assistant
thought/tool-call metadata. Mash does not restore that removed metadata or
invent aliases for the provider's regenerated IDs. Neither checkpoint proves
summary compaction, so it must not archive earlier removed turns. A SQLite
refresh removes the old canonical IDs, preserves searchable prose and leaves a
subsequent unchanged-source restart checkpointed.

| Capture | SHA-256 |
| --- | --- |
| ID-preserving masked prefix | `cc00ff7a44d88669a2fc19ab04970bc4b9b7b1746585e96112bb25d7b571f772` |
| Content-only replacement, cancellation and rewind | `ca2dbea56cf7bf62bddffe9c463aed2e2116b2473e8c33b71f0fb9d19a4b693e` |
| Native loaded JSON | `c7c7bc949e0b2fe04541f1ce1a0110835ca9b07fd0757bd317fdccb8098c6409` |

The exact deterministic capture repeated with identical evidence and file hashes.
Rust controls verify fixture hashes, native/raw and canonical identities, four
retained messages, three prose projections, no false compaction archive, local
query inclusion/exclusion, refresh deletion and reopened checkpoint reuse.
These are component/SQLite lexical checks, not full semantic inference or live
session validation. The harness, native loaded output, evidence and repeat log
are retained outside Git under
`/home/james/scratch/mash-artifacts/gemini-tool-history-38700b4-20261003`;
`capture.mjs` SHA-256 is
`7613629da95fbd8823f844d59965e7ce1e5028ecf672063aa7b5008f62a5d138`.
The initial harness assertion failed because Node's strict array equality
distinguishes VM-realm prototypes; comparing host arrays fixed the harness
without changing native writer behavior. Its partial capture is retained
separately and is not an accepted fixture.
The three executed sources and inspected client were additionally fetched from
the pinned first-party URLs and matched the cached bytes exactly; the private
`remote-source-verification.json` retains URL, byte-count and hash evidence.

Tracking: `mash-c0d.2.1.2`. The parent live UI/model, token-budget path and ambiguous
alternate-summary recovery gates remain open.

Validation on 2026-10-03: 592 standard Cranelift tests passed (114 opt-in cases
excluded), strict all-target Clippy and formatting/diff checks passed, and one
doctest passed. Final logs use the `gemini-tool-history-` prefix and
`-final-20261003.log` suffix in the private artifact directory. The first refresh test rewrote
identical fixture bytes before expecting a checkpoint skip; changing file
metadata correctly caused a re-read with zero changed messages. The corrected
restart leaves the source untouched and verifies both checkpoint reuse and zero
changes. No production parser behavior changed in this follow-up.
