# Redis real-history publication and persistence at a larger budget

The earlier 2 GiB Redis publication was OOM-killed and supplied no usable
ingestion timing. At `1b8720f`, the same frozen canonical histories successfully
published and replaced their active revision with Redis limited to 8 GiB and
`maxmemory=6gb`, with `noeviction` and append-only persistence enabled.
This calibrates a specific corpus and configuration; it does not establish a
general backend winner or complete the shared-index production evaluation.

## Corpus and configuration

- Frozen accepted contributions: 789 conversations, 225,267 canonical messages,
  and 29,453 searchable messages with compatible 384-dimensional vectors.
  These include 515 Claude Code and 274 Codex conversations.
- Original NDJSON upload: 550,139,311 bytes, SHA-256
  `a6a1909d299d8623f26e12d639734cf13e73c7d09654280658ab95bfb4351eb4`.
- Replacement: 550,139,333 bytes, SHA-256
  `a7aca0943ffc28dcf01b6d4b26b08ab5bdadc0838756c9679d92fc6f387ef869`.
  Revision changes from 1 to 2 and one conversation title changes; messages,
  projections, vectors and context edges are unchanged.
- Xenia Linux host; load and filesystem cache were uncontrolled. Each trial
  used a fresh, isolated Redis container, 4 CPUs, an 8 GiB container memory
  limit, and a host API process without a separate RAM limit.
- Cached `redis:8.2` image reports Redis **8.2.10**. Image ID:
  `sha256:164c759a0c342ee69d08fc99219382b0fd682181465c0df2e0e6911f4c85d73c`.
- Redis FLAT/exact search; `maxmemory=6gb`, `maxmemory-policy=noeviction`,
  `appendonly=yes`, `appendfsync=everysec`. Scheduled RDB saves and automatic
  AOF rewrites were disabled to isolate the measured phases. This is **not**
  a recommendation to leave AOF compaction disabled in production.
- Strict server TOML: `redis_max_node_mib=2048`,
  `max_mutation_snapshot_mib=2048`, `max_concurrent_uploads=1`.
  One authenticated publishing node in a fresh group.
- Optimized binary SHA-256:
  `349d9ca8d1d5128d0ddeaef381e4ec27f7d4aeac0374b4ada4a7f7f87d711c74`.
  Runtime source is unchanged from the previously validated 567-test commit.

The source refresh remains incomplete because a native Claude record is malformed.
These are complete frozen **accepted contributions**, not proof that all current
native histories are indexed or ready. SQLite and original histories were read
only; no source record or readiness marker was changed.

## Authenticated HTTP publication

| Trial | Initial publication | Replacement | API peak RSS |
| --- | ---: | ---: | ---: |
| First | 28.849 s | 36.237 s | 932,784 KiB |
| Diagnostic repeat | 30.793 s | 43.946 s | 959,776 KiB |
| Corrected restart trial | 30.025 s | 38.560 s | 1,024,684 KiB |

Every publication returned HTTP 200, the expected revision, all four expected
counts, and zero removed messages/purged conversations. Times include HTTP
transfer, validation, snapshot decoding/planning, staging, and active-head
publication. They are observations, not latency percentiles. No controlled
head-to-head Qdrant/PostgreSQL comparison was run here.

Before each restart attempt, the API verified counts, revision 2, the replacement
title, and an exact canonical message body against read-only SQLite. The corrected
trial also verifies provider/node scope and nonempty semantic candidates for
both message and thread searches before and after restart. Candidate probes use
a stored document vector plus a diagnostic lexical query, including self matches;
they establish retrieval availability and scope, not natural-language relevance
or end-to-end CLI/MCP search latency.

## Restart and resource verification

The corrected trial acknowledged local persistence with `WAITAOF 1 0 10000`,
stopped the API, gracefully restarted Redis, waited for `loading=0`, and reopened
the API against the rediscovered endpoint. Redis restart through completed AOF
loading took **26.944 s**. This excludes subsequent API initialization and Redis
Search readiness checks; it is not an end-to-end recovery-time measurement.

After restart, API checks again confirmed revision 2, all four counts, the changed
thread title, the exact canonical anchor body, and successful provider/node-scoped
message and thread candidate retrieval. Before/after verification summaries match.
No database OOM occurred; the owned container and anonymous volumes were removed
after the process handle completed.

| Corrected trial resource measurement | Value |
| --- | ---: |
| API peak RSS across the complete trial | 1,024,684 KiB (about 1,001 MiB) |
| Highest periodic Docker working-set sample | 3.994 GiB |
| Redis allocation peak through initial publication | 2.198 GiB |
| Redis allocation peak through replacement | 4.271 GiB |
| Redis allocation peak during AOF reload | 4.160 GiB |
| Redis allocation after recovery checks | 2.079 GiB |
| AOF size before restart | 3,390,378,444 bytes |

Redis allocation peaks come from its `INFO memory` counters, not Docker RSS.
Periodic Docker sampling produced 57 observations and can miss transient peaks.
The restart resets Redis's peak counter, so publication and reload are recorded
separately. The API high-water mark includes publication and verification; the
third trial additionally ran candidate probes. Samples and timings should not be
treated as precise cross-trial performance comparisons.

The first two API restart checks failed because the benchmark retained the old
Docker-assigned host port. A small independent container reproduced the change
from `127.0.0.1:33825` to `127.0.0.1:33826` after restart. Redis itself finished
loading, retained its index/schema and embedding dimensions, and had no OOM.
The corrected harness rediscovers the published endpoint after restart. These
failures are retained in the evidence and do not count as successful API recovery.

## Limits and evidence

`maxmemory` controls Redis allocation admission, not total RSS, container memory,
or the API process. The serialized mutation-work quota is also not a hard RAM
bound. Allow headroom for staging, lazy freeing, index construction, readers,
allocator retention, AOF buffering and persistence work. A full replacement
temporarily holds old and new data; payload caps alone do not size the database.
The successful 8 GiB configuration does not prove a minimum safe budget, several
simultaneous nodes/publishers, a different corpus, or sustained growth.

This test covers a graceful restart after local AOF fsync acknowledgement. It
does not simulate an unacknowledged crash, disk loss, replication/failover,
automatic AOF rewrite or RDB snapshot copy-on-write pressure. Larger Redis
resources resolve the earlier publication OOM for this workload, not every
production resource/recovery requirement.

Private artifacts are under
`/home/james/scratch/mash-artifacts/redis-budget-20261001`, including the harness,
configuration, binary/input hashes, Docker/Redis resource samples, persistence
and schema diagnostics, responses, and terminal cleanup evidence. Failed attempts
are retained in sibling `-first-run` and `-second-run` directories. Private
histories, credentials and payloads stay outside Git. Only owned containers and
volumes are removed after terminal process handles; unrelated services remain.

Bead `mash-c0d.21.3` tracks this calibration. Parent `mash-c0d.21` remains open
for broader resource bounds. `mash-c0d.2.2.2` tracks small persisted-backend
restart contracts in CI. `mash-dna` remains open for the complete backend
evaluation, including representative end-to-end searches and reviewed relevance.
