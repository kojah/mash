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
the migration or ingestion. PostgreSQL/Redis wire and storage layouts are unchanged.

Unmarked summaries and complete JSON snapshots lack reliable event provenance.
They continue to follow resumed history; live UI/model compaction and alternate
summary formats remain validation work in Beads `mash-c0d.2.1`.
