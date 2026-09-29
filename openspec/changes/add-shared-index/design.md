# Local and shared index design proposal

## Access boundary

Recommended: nodes publish to and query a small HTTP service backed by
PostgreSQL. The service owns shared mutation rules, query ranking, authorization,
and message context. The CLI retains its existing SQLite and FastEmbed pipeline.

This recommendation is pending user selection. Direct PostgreSQL access would
require a different client boundary and should not be implemented alongside it.

## Configuration

Use a strict local configuration document containing provider roots and the
indexing horizon. Shared configuration is optional. When absent, the CLI is
fully local and performs no shared requests.

For the recommended HTTP boundary, the proposed shared fields are:

- `endpoint`: deployment-supplied service URL; no hardcoded machine or host.
- `node_id`: stable publisher identity, independent of hostname or SSH aliases.
- `token_env`: name of an environment variable holding an access token; status
  reports configuration and pending work without printing token values.

An existing node-inventory configuration needs an explicit migration to the
new format. Reject the old shape with a useful configuration error rather than
silently choosing roots or continuing SSH behavior.

## Publishing and failures

Record pending publication in SQLite in the same transaction as each canonical
change. Publish bounded batches after local commits. Advance acknowledgements
only after the shared store confirms acceptance. A dropped response may cause
the same batch to be retried; retries must be idempotent.

Each node uses a persistent publication sequence. The shared store tracks the
last accepted revision for each owned record and rejects stale mutations.
Message removal, conversation removal, and forget must all emit deletions.
A later local no-change index run still drains outstanding work.

Local success and shared failure are separate outcomes. Shared failures never
roll back local indexing. Status exposes pending publication and the last
acknowledged state. Explicit shared search reports an actionable shared error
when the service is unavailable; it never silently substitutes local results.

## Identity and deletion

Authenticate publisher identity rather than trusting a `node_id` in a request.
Initially isolate ownership by authenticated node, provider, conversation ID,
and message ID. Repeated uploads from the same owner update existing records.
Copies of a conversation on different nodes remain separate until a deliberate
cross-node identity and conflict policy is chosen.

Forget creates a durable tombstone. An acknowledged tombstone blocks stale
uploads from resurrecting the owned conversation. A global forget across all
owners requires a separate explicit policy; do not imply that a local forget
has removed another node's copy.

## Search and view

Initially propose explicit local/shared scope for search and view. A shared hit
includes its owner so a subsequent view can address the same stored record.
Shared view includes ordinary context and raw tool-result messages without
requiring the source machine to be reachable.

Store embedding generation and dimensionality with uploaded vectors. Search
must never compare incompatible vectors. Determine shared lexical retrieval,
exact cosine retrieval, RRF, and bounded reranking before selecting database
extensions. No ANN index is implied by choosing PostgreSQL.

If merged local/shared search is subsequently selected, define deduplication,
ranking, and explicit partial-failure behavior before making it the default.
Pending local uploads and already-published local records otherwise overlap.

## Hosting

Provide a development instance and integration fixtures after the access
boundary is selected. Keep deployment-specific URLs and credentials out of the
repository. Choose production hosting independently. Do not deploy or migrate
existing histories as part of implementation verification.
