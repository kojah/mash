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

The application now supports server-owned silver revisions/completeness,
independent lexical/dense manifests, one active gold configuration and explicit
source-free shared rebuilds. See [the current layer contract](silver-gold-configuration.md).
These application capabilities do not prove live fleet migration or complete the
remaining topic-quality and retrieval evaluation work in `mash-q3e`.
