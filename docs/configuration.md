# Configuration

[Documentation](README.md) · [Project home](../README.md)

Supported providers are Claude Code, Codex, GitHub Copilot (Chat and CLI),
Cursor (IDE and Agent), OpenCode, and Antigravity. Original provider history
files are never modified.

mash reads `config.toml` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned TOML. Provider presence enables that provider;
roots must be absolute. `index.since_days` defaults to 90. Set
`index.all_history = true` instead for all history; these settings cannot be
combined. CLI indexing flags override these settings.

Optional `[views.query]` settings select the default `unit` (`"chats"` or
`"messages"`) and `limit` (1–200). Omitting them preserves chats and 20 results.
CLI/MCP search parameters override either setting without rebuilding the index.
`[views.query.fusion]` controls hybrid candidate fusion independently of views
builds: `rank_constant` defaults to 60 (1–1000), and `lexical_weight` and
`semantic_weight` each default to 1 (1–100). Higher weights favor that candidate
stream before contextual reranking. `recency_weight` (default 50, 0–100) favours
newer messages in the final order: recency is ranked like another candidate
stream and combined with the relevance order, at that percentage of
relevance's weight, after reranking and before paging. A newer
message can overtake comparably relevant older ones, but a strong old match
still outranks a weak new one; undated messages get no recency credit and 0
ranks by relevance alone. These query-only settings apply to local,
combined and fallback searches, take effect on the next request, and require no
index rebuild. Exact-mention search is independent of fusion settings.
Set `[views.query] ranking = "fusion"` to keep fused hybrid ordering and skip
contextual reranking and its context fetches. The default is
`"contextual-rerank"`. This query-only policy applies across local, shared and
combined searches through the same CLI/MCP core, using the same typed requests.
It changes no stored vectors or silver. Dense query embeddings still require
the local embedding model or configured remote service. Fusion skips loading
the reranker. Local installation inventory validation remains required; remote
fusion needs no local model installation. Status reports `query_models` as
`"available"` or `"unavailable"` for current TOML query defaults,
separately from physical `models_installed`. Switching ranking reloads the
matching cached backend.
Use diagnostics to compare execution and results before choosing a default.
For one search, use `--ranking fusion` or MCP `"ranking":"fusion"`; the other
choice is `contextual-rerank`. Omission inherits current TOML. Explicit ranking
overrides require related-discussion intent, do not persist, and never rebuild
the index.
For one hybrid request, CLI `search` accepts `--fusion-rank-constant`,
`--lexical-weight`, `--semantic-weight` and `--recency-weight`; MCP `search` accepts a partial
`fusion` object with the corresponding underscore-named fields. Omitted fields
inherit current TOML defaults. Overrides are validated before model work,
apply to local/shared/fallback ranking, and are rejected for exact-mention.
For resolved policy and retrieval outcomes, use CLI `search --diagnostics` or
MCP `search` with `"diagnostics":true`. The optional report includes selected
artifacts, completed retrieval sources, fallback, result count and core time.
It does not run indexing or change search results.
See [silver enrichment and views configuration](https://github.com/kojah/mash/blob/a49210157b990b23c5ef9f90d5d1d13fd3ce9f11/docs/silver-gold-configuration.md)
for the layer contract and current configuration scope.

```toml
version = 1

[providers.claude-code]
roots = ["/home/james/.claude/projects"]

[providers.codex]
roots = ["/home/james/.codex/sessions"]

[index]
since_days = 90
destination = "both"

[shared]
group_id = "personal"
node_id = "xenia"
api_url = "https://mash.example.net"
token_env = "MASH_NODE_TOKEN"
```

This example indexes both providers in SQLite and publishes that index to the
shared service. SQLite is required on every node. `index.destination` accepts `local` or `both`;
`both` requires the top-level `shared` connection settings. Without an explicit
destination, mash selects both when shared settings exist, and local otherwise.
Shared-only configurations are rejected. Both mode publishes exactly the local
index; the former per-destination `[index.local]` and `[index.shared]` provider
filters are rejected. Disable a provider entirely to keep it off the node.

Omit the top-level `shared` for a fully local installation. Search uses local Qdrant plus other nodes in the shared group when shared settings exist; otherwise it uses local Qdrant. Configure the same
group ID and API URL on every member, with a distinct node ID and token for each
publisher. The service authorizes each token for a fixed group and node. Group IDs select
separate Qdrant collections; every node within a group contributes to one joint
index. The database can live wherever you choose.

## Remote embeddings

Local FastEmbed inference remains the default. Add `embeddings` to the node
configuration to use Voyage-4 for both document and query embeddings:

```toml
[embeddings]
api = "voyage"
url = "https://api.voyageai.com/v1/embeddings"
model = "voyage-4"
dimensions = 1024
token_env = "VOYAGE_API_KEY"
```

Set `VOYAGE_API_KEY` in the node's environment. It is separate from the shared
index token; the index service never receives it. History search projections
and search queries are sent to the configured inference endpoint. There is no
automatic fallback to another model. `model` is configurable, including
`voyage-code-4`; Voyage dimensions can be 256, 512, 1024, or 2048. Requests use
`input_type: document` for indexing and `input_type: query` for searching.
Overlong inputs fail explicitly rather than being silently truncated.

For an OpenAI-compatible endpoint, set `api` to `openai-compatible` and provide
its complete embeddings URL, model, dimensions, and optional `token_env`.
Set `request_dimensions = true` if the endpoint should receive the `dimensions`
parameter. Fixed-dimension self-hosted endpoints can omit it. Mash supports
1–2048 dimensions and validates vector count, ordering, dimensions, and values.
HTTP requests are asynchronous, bounded, and time out after 60 seconds. Local
SQLite pipelines remain on blocking workers while remote requests run on Tokio.

Remote indexing needs no local model assets. Reranking remains local; run
`mash models install` with remote embeddings configured to install only the
reranker before searching. No models are downloaded by indexing or searching.
If a valid local embedding installation already exists, installing for remote
embeddings preserves it. Switching back to local inference does not require
reinstalling those assets; the changed embedding space still requires indexing.

For shared indexing, configure the service's embedding space as well:

```toml
[embeddings]
model = "voyage-4"
dimensions = 1024
```

The service configuration applies to every group it serves. Publishers and
searchers must use the matching model and dimensions. Changing them rebuilds
local embeddings on the next index pass; reindex all shared publishers before
searching. For a shared model or dimension change, explicitly activate the server
settings and rebuild each member's shared views; a new group ID is unnecessary.
Model identifiers and dimensions prevent mismatched
queries from being compared with the stored embedding space.
