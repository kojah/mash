# Codex native history evidence

The fixture `fixtures/codex-native-migration-0.159.2.jsonl` was written by the
unmodified installed `codex-cli 0.159.2`, using `codex migrate-rollouts --apply
--json` on synthetic history in an isolated `CODEX_HOME`. No model call or private
history was involved. Migration reported `migrated` for thread
`11111111-1111-4111-8111-111111111111`.

The synthetic input contained two identical user requests, each followed by a
different answer. Every request and answer had a raw `response_item` and its
legacy `event_msg` mirror. Native migration retained the raw records and replaced
the legacy mirrors with completed `UserMessage` and `AgentMessage` turn items.
The session declares `history_mode = "paginated"` and records carry ordinals.

Raw response IDs and completed item IDs differ. User response records have no ID
in this fixture. Completion timestamps also differ from response timestamps.
Repeated text therefore cannot establish message identity. The parser regression
checks four messages, distinct identities for the repeated requests, original
response timestamps, session/project identity, and stable rereads.

## Remaining contract gap

The current adapter reads raw `response_item` records and ignores event mirrors.
It passes this mixed migration fixture, but does not yet ingest dialogue present
only in completed turn items. This fixture does not prove lifecycle-only support.
Beads `mash-c0d.7.3` tracks that gap and reconciliation of partially mirrored
histories. Reconciliation must preserve separate repeated turns, avoid treating
streaming or started items as final answers, retain unmatched dialogue, and keep
canonical context available. Do not use global content deduplication or assume
raw and completed IDs coincide.

`fixtures/codex-native-lifecycle-only-0.159.2.jsonl` is a second successful
native migration, in a fresh isolated home, of the same synthetic legacy events
without their raw response records. It contains four completed dialogue items
and no response records. This is an actual emitted format, not a guessed shape;
the current adapter cannot yet recover its messages. Keep this fixture for the
lifecycle implementation's conformance checks; do not count it as passing
retrieval coverage.

The existing bounded reader permits streaming large completed `FileChange` and
`CommandExecution` envelopes without retaining their tool payloads. Dialogue and
canonical records retain the 8 MiB input limit. Ordinary malformed bounded lines
are counted and skipped; malformed or oversized streamed envelopes fail indexing.
