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

Both endpoints must reach the same Qdrant 1.20+ instance. If an API key is
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

## Local embeddings

Semantic search uses the installed all-MiniLM-L6-v2 model (384 dimensions).
Run `mash models install` explicitly to install embedding and reranking models;
indexing and search never download models. See [model setup](../indexing.md#enable-semantic-search).

The same embedding space is used for local and shared search. A server needs
local model assets when rebuilding shared views from saved history.
