# Configure Mash

Start with the TOML file created by `mash init`. Run `mash status` to see its
resolved location, or select a file explicitly with `mash --config PATH status`.
Configuration is strict TOML: unknown fields and unsupported formats are errors.

## Choose histories and an age limit

Each configured provider is enabled. Roots must be absolute paths to that
provider's history, not to the source-code project you want to search.

```toml
version = 1

[providers.codex]
roots = ["/home/you/.codex/sessions"]

[providers.claude-code]
roots = ["/home/you/.claude/projects"]

[index]
since_days = 90
destination = "local"
```

Use the paths detected on your machine by `mash init` or
`mash status --installation`; the paths above are examples. Original provider
files are never modified. See the [provider reference](../indexing.md#provider-history-and-identity)
for formats and alternate locations.

To include all ages permanently, replace `since_days = 90` with
`all_history = true`. Do not set both. One-off indexing flags such as
`--all-history` override the configured age selection.

## Local Qdrant

The defaults use HTTP port 6333 and gRPC port 6334 on localhost. For an existing
service at another address, edit:

```toml
[qdrant]
url = "http://127.0.0.1:6333"
grpc_url = "http://127.0.0.1:6334"
memory = "durable"
# api_key_env = "MASH_LOCAL_QDRANT_KEY"
```

Both endpoints must reach the same Qdrant 1.19+ instance. If an API key is
required, put it in the named environment variable, not in the URL.

## Search defaults

```toml
[views.query]
unit = "chats"
limit = 20
intent = "exact-mention"
```

CLI and MCP arguments override these defaults for one request. Query defaults
take effect without rebuilding. To search by meaning, follow
[semantic model setup](../indexing.md#enable-semantic-search); changing intent alone
does not create dense search data.

For weights, recency, reranking, and diagnostics, see
[tuning semantic ranking](search-reference.md#tune-semantic-ranking).

## Share across your machines

A local-only setup needs no `[shared]` table. To publish as well, follow
[multi-machine setup](../shared-index.md), add its connection settings, and set
`[index] destination = "both"`.

Every node keeps its own local database and Qdrant search views. Sharing
publishes exactly that local index; there is no shared-only ingestion mode.
Use a different node ID and token on each machine, with the same group ID and
API URL. Normal search then combines local results with the other nodes.

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
