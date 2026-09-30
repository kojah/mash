# Claude source reconciliation validation

The predecessor (`a2c20e0`) could lose a checkpointed surviving transcript when
another physical copy of the same Claude thread was deleted. Divergent copies
could overwrite one another. Routed publication could retain a deleted copy's
tail in either Redis or PostgreSQL.

The implementation retains source contributions separately from canonical
threads. Native session/agent-scoped UUIDs deduplicate matching messages, and
native ancestry determines branch context before deterministic ordering.
Conflicting payloads or ancestry roll back the contribution transaction.
Unchanged searchable text retains its local vector when ordinals change.

## Automated validation on Xenia

- Cranelift full suite: 422 passed, 8 skipped; execution 98.315 seconds. This is
  an observed validation run, not an isolated test-speed benchmark.
- Clippy with warnings denied passed; one doctest passed.
- Four real-model CLI/authenticated HTTP cases cover PostgreSQL and Redis with
  normal and independently routed publication. Each checks local/shared counts
  and message payloads across matching copies, divergence, rewind, deleting
  either copy, retries, empty files, restoration, and forgetting.
- Unit/integration coverage includes both contribution orders, causal branches,
  late-arriving ancestors, intermediate non-message events, cycles, conflicting
  UUID rollback, checkpoint retries, vector reuse, source identity changes,
  tombstones, routed horizons, revision/stamp checks, and explicit upgrades.

Disposable services used Redis port 57379 and PostgreSQL port 57432. These tests
do not establish production deployment or native Mac validation.

## Frozen real transcript sample

Private artifacts are under
`/home/james/scratch/mash-artifacts/claude-copies-a2c20e0/real-sample`.
The sample contains 30 Claude files totaling 15,884,317 bytes. Its schema-34
index has 30 threads, 2,378 canonical messages, 2,378 source message
contributions, and 152 searchable vectors. First indexing took 6,121 ms wall
time. An unchanged repeat took 16.4 ms wall time (6 ms reported core time),
skipped all 30 sources, and performed no inference or model loading.
Of 2,409 raw user/assistant records, 31 have no projectable content under the
existing content flattening rules; the other 2,378 match the canonical count.

A comparison with the older installed schema-32 release yielded 2,195 messages
and 28 threads. Comparing the multiset of role, content, search projection, and
timestamp found **zero old payloads missing** and **183 additional payloads** in
the new index. The inputs contain two native session groups with two distinct
agent identity sets each. This older release predates the separate parent/agent
identity fix; the comparison cannot isolate this change's contribution to the
recovery. It establishes payload preservation against that baseline, not equal
output or a human relevance assessment.

SQLite file size was 4,280,320 bytes for the older baseline and 9,166,848 bytes
for the new index. Source snapshots store canonical text per physical source;
this sample has no duplicated native message contributions. The storage cost
and changed identities are intentional, and must be included in capacity
planning. The comparison includes other intervening changes.

## Remaining gates

The broader Beads preservation gate stays open pending the remaining migration,
move, failure-injection, native Mac, and real captured-copy validation. Giant
conversation memory bounds, human relevance, exact-commit fleet deployment,
and performance comparisons remain separate production gates. Changed routed
threads currently use the existing full-thread embedding path; incremental
shared vector reuse still needs measurement. Search never initiates this
migration or indexing.
