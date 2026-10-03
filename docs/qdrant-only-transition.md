# Qdrant-only shared storage

Qdrant is the sole supported shared backend. Nodes retain required local SQLite
indexes for offline use and communicate with the authenticated Axum service.
Only that service accesses Qdrant. One node group has its own shared collections.

The PostgreSQL and Redis adapters, driver dependencies, schema migrations,
Cornucopia output and generation, and backend-selection dispatch are removed.
SQLite sqlc generation remains available through the Makefile build workflow.
CI provisions only Qdrant for shared storage. The lockfile shrinks from 434 to
375 packages: 59 removed, none added. This reduces the maintenance/build surface;
it is not a measured search-latency improvement.

Server TOML requires `qdrant_url_env`, with optional `qdrant_api_key_env` and the
existing bounded Qdrant vector/memory/publication settings. There is no `backend`
selector, including `backend = "qdrant"`. PostgreSQL/Redis configuration fields
are rejected. Node HTTP configuration and CLI/MCP tools retain their interfaces.

Unique provider and CLI publication contracts were ported to Qdrant. Shared
read/index tracing remains on the concrete Qdrant methods. Historical benchmark
reports remain evidence of their recorded versions; the Redis vector pilot is
archived under `research/storage/`, outside supported development tooling.

## Validation on Xenia, 2026-10-03

- Cranelift standard suite: 618 tests passed.
- All 31 selected Qdrant contracts passed across the initial run and focused
  corrections. Coverage includes real-model relevance, scoped messages, thread
  metadata, every provider, rich filters, revisions, rollback, source-silver
  evidence, schema compatibility, native copy reconciliation and offline forgets.
- Three additional installed-model and parallel-retrieval tests passed.
- The owned Qdrant/API restart contract passed graceful restart and SIGKILL
  recovery with persisted storage and stale-revision rejection.
- All-target Clippy with warnings denied, formatting, doctests, SQLite generation
  freshness, workflow YAML parsing and deployment-script tests passed.

Two ported fixture assumptions needed correction: provider metadata comes from
thread listing rather than message viewing; and migrated, unverified silver can
publish successfully while explicit indexing still reports incomplete status.
The latter fixture now checks that publication does not certify legacy silver.
Logs remain outside Git under `/home/james/scratch/mash-artifacts/` with prefix
`qdrant-only-` and date `20261003`. Tests used a disposable Qdrant 1.19.1 service;
restart tests owned a separate container. These checks do not establish hosted CI,
production-size performance, power-loss durability or deployment completion.

## Live transition and remaining layer work

The existing Xenia service still uses Redis. No existing shared history has been
deleted or migrated as part of this code cleanup. Bead `mash-847` tracks persistent
Qdrant provisioning, configuration replacement, explicit node re-publication and
coverage/freshness/search verification before retiring the old data. The old
Redis server configuration cannot start the new binary.

### Live preparation verified, 2026-10-03

Persistent Qdrant 1.19.1 now runs on Xenia as `mash-qdrant`, with Docker volume
`mash-qdrant-data`, restart policy `unless-stopped`, and HTTP bound only to
`127.0.0.1:56333`. Its presence does not mean that chat data has been migrated.

The staged server configuration is
`~/.config/mash/server-qdrant.toml`, with a private environment file alongside it.
It retains the three `personal` group identities, existing API token variables,
4096 MiB node snapshot limit and one concurrent upload. A temporary current-code
API on `127.0.0.1:8788` successfully initialized the group, accepted each node's
authenticated capabilities request, and rejected unauthenticated status with 401.
The temporary API was then stopped; no preview service was installed.

Live traffic remains on the existing Redis API at port 8787. Its freshness
baseline contains Xenia and personal-MacBook publications; dev-MacBook publication
is absent. The dev MacBook is reachable using its existing dedicated SSH identity
and has TOML configuration under macOS's
`~/Library/Application Support/dev.jamesqo.mash/`. The personal MacBook's SSH
connection timed out. No node configuration, installed binary or existing index
was changed by this preparation.

Continue `mash-847` by explicitly preparing current certified silver and publishing
each node into Qdrant, then verifying source coverage, freshness, searches and
local offline access before changing the live endpoint. Keep Redis data until
that verification passes. Private baseline and authentication-check captures are
under `/home/james/scratch/mash-artifacts/qdrant-live-transition-20261003/`.

### Provider replay and native build preparation

The installed Xenia SQLite index is schema 16 (568 threads, 169,425 messages,
101,808 vectors); the dev MacBook index is schema 15 (2,252 threads, 301,838
messages). Consistent SQLite backup snapshots of both were saved in their private
migration directories. Neither installed index was migrated in place.

An explicit Xenia replay into a separate schema-39 database used the configured
Claude Code/Codex roots and 90-day source horizon, with local-only lexical gold.
It discovered 818 files totaling about 4.08 GB. The run reported one malformed
record, then exhausted source-change retries on this actively appended Codex
conversation and exited with code 9. The migration database has zero committed
threads/messages. No data was published or certified by this attempt.

Bead `mash-847.1` tracks the active-source ingestion blocker. Per-parse and
post-queue stamp checks already have bounded retries; increasing retry counts
does not establish a safe snapshot. Investigate a verified committed-record
prefix while retaining rejection of unsafe edits, truncations and replacement,
correct checkpoints, and explicit reporting of malformed records.

The dev MacBook built archived source `faf7faf` using its nightly toolchain and
`cargo build-clif --locked -j 2` in a private migration directory. The native dev
build completed in 1m 46s. A read-only `status` smoke check exited successfully,
recognized installed models, reported the deliberately missing database unready,
and did not create it. Its SHA-256 is
`819b84cfa4cedb73c7dc4a8f5572ab567c0868e5b193330da9f2bb81b1fb8a16`.
The installed binary remains unchanged. The personal MacBook still timed out.

The application now supports server-owned silver revisions/completeness,
independent lexical/dense manifests, one active gold configuration and explicit
source-free shared rebuilds. See [the current layer contract](silver-gold-configuration.md).
These application capabilities do not prove live fleet migration or complete the
remaining topic-quality and retrieval evaluation work in `mash-q3e`.

### Active JSONL replay fix

Claude Code and Codex JSONL readers now parse a private, length-bounded file
capture while preserving original source and message identities. File identity
and a hash of the captured prefix are checked after parsing and before applying
queued results. Later appends remain pending through the original checkpoint;
edits, truncation and replacement invalidate the capture. Other provider readers
retain their existing stability checks. Captures add temporary disk usage and
file I/O, without retaining whole source files in memory.

The separate Xenia migration replay subsequently scanned 818 files totaling
4,087,707,520 bytes and retained 812 threads and 315,979 messages, including
38,660 searchable messages. It completed in 141.402 seconds with no model
inference and no source-change rollback. This is a functional replay under host
load, not a controlled performance comparison; the corpus grew between runs.

The replay still exited with code 9 because one Claude source was rejected.
Its diagnostic points to line 8712, column 184; independent JSON decoding
confirmed an invalid complete line rather than two concatenated valid objects.
Silver remains incomplete and gold unavailable. The fix does not ignore
malformed records, change provider files, publish to Qdrant, or switch the live
Redis service. Private captures are `xenia-silver-prefix.json` and
`xenia-silver-prefix.log` in the migration artifact directory above.
