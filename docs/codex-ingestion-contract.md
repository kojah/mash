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

## Dialogue reconciliation

The adapter reads raw response records and completed user/assistant turn items.
It pairs the nearest unconsumed dialogue candidate only when the representations,
role, exact canonical content hash and turn affinity agree. Each pair is consumed
once. Same-representation repeated requests remain separate; marked turn endings
retire unmatched candidates from that turn. A queued raw user request emitted
before the previous turn ends follows the next start marker, as in the native
fixture. Tools remain canonical context and do not replace a dialogue candidate.
The first representation supplies the retained ID and timestamp.

This is deliberately conservative: changed content and ambiguous interleavings
can retain both records. It does not use content-wide deduplication, infer missing
text, merge similar requests, or treat started items/deltas as completed answers.
Legacy event mirrors remain omitted. Completed-only IDs include turn and item
identity; thread metadata remains authoritative for conversation identity.

`fixtures/codex-native-lifecycle-only-0.159.2.jsonl` is a second successful
native migration, in a fresh isolated home, of the same synthetic legacy events
without their raw response records. It contains four completed dialogue items
and no response records. This is an actual emitted format, not a guessed shape;
parser conformance checks recover all four messages, preserving the two repeated
requests. Model-backed core checks explicitly index both native fixtures and
exercise message/thread searches with provider, role and project filters and
canonical context views. These fixtures do not establish every native UI flow or
aggregate ingestion memory use.

SQLite schema 32 and source fingerprint `search-text-v6` require an explicit
`mash index` refresh. Existing canonical messages and embeddings survive the
migration, but readiness/checkpoints are invalidated so lifecycle-only sources
are reingested. Shared source fingerprints also change. Search never performs
that refresh automatically.

The existing bounded reader permits streaming large completed `FileChange` and
`CommandExecution` envelopes without retaining their tool payloads. Dialogue and
canonical records retain the 8 MiB input limit. Ordinary malformed bounded lines
are counted and skipped; malformed or oversized streamed envelopes fail indexing.
