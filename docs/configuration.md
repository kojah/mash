# Configure Mash

`mash init` creates the configuration file. Run `mash status` to see its
location, or select another file with `mash --config PATH status`. The file
must be TOML. A single machine needs only this:

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

To change history selection, coverage or sharing interactively, rerun
`mash init` in a terminal and choose **Review and edit settings**. The wizard
preserves your other settings and keeps the previous file as a backup; comments
and formatting are regenerated.

## Choose histories and an age limit

Each provider listed under `[providers]` is indexed. Roots must be absolute
paths to that provider's history, not to the source-code project you want to
search. Use the paths detected by `mash init` or `mash status --installation`;
those above are examples. Original provider files are never modified.

`since_days` defaults to 90. To include all ages, replace it with
`all_history = true`; do not set both. One-off indexing flags such as
`--all-history` override the configured age selection.

## Connected agents

`mash init` records the agents it connected:

```toml
[agents]
connected = ["claude-code", "codex"]
```

`mash status --installation` reports a listed agent as needing repair if its
Mash registration disappears, so remove an agent you disconnect on purpose.
Without this table, an agent is expected only while its own settings register
Mash. `mash init --no-agents` leaves the list as it is.

## Search defaults

```toml
[views.query]
unit = "chats"
limit = 20
intent = "exact-mention"
```

`intent` is `exact-mention` (every word, in any order, best matches first; the
default) or `related-discussion` (by meaning). CLI and MCP arguments override
these for one request. Changes take effect
without rebuilding. To search by meaning, follow
[semantic model setup](indexing.md#enable-semantic-search); changing `intent`
alone does not create dense search data.

## Local embeddings

Semantic search uses the all-MiniLM-L6-v2 model (384 dimensions) for local and
shared search. Indexing and search never download models; run
`mash models install` for the embedding and reranking models. See
[model setup](indexing.md#enable-semantic-search). A server needs the model
assets to rebuild shared views from saved history.

## Embedding inference

Embeddings are generated locally by default. To send indexing batches to an
authenticated Mash embedding worker, add:

```toml
[embeddings]
backend = "remote"
url = "https://YOUR-WORKER.modal.run"
token_env = "MASH_EMBEDDING_TOKEN"
timeout_seconds = 120
```

This sends prepared searchable text to the worker, which may contain sensitive
conversation content. The worker gets no database access. Search-query
embeddings stay local, so semantic search still needs `mash models install`.

- Set the variable named by `token_env` in every process that runs
  `mash index`, including watchers. Use a Modal proxy token in
  `TOKEN_ID.TOKEN_SECRET` form, not your Modal account API key. The value is
  never written to configuration or status.
- `url` must be HTTPS unless the worker is on loopback.
- A failed request returns an error; Mash neither retries it nor switches to
  local inference. A timed-out request may still have incurred GPU cost, so
  check the worker before retrying indexing. Existing matching vectors are
  reused.
- To return to local inference, remove the table or set only
  `backend = "local"`.

The worker must implement Mash's model-specific contract; an arbitrary
embedding API is not interchangeable. Changing the embedding recipe can require
rebuilding search data. Deployment instructions are in
`deployment/embeddings/README.md` in the source repository.

An idle worker can take minutes to start. To keep routine refreshes quick, add
`local_below` to the table:

```toml
local_below = 256
```

Batches with fewer new texts than this use the local model, which needs
`mash models install`; larger ones, such as a first index, go to the worker.
Both produce the same vectors. The maximum is 1024; without the key, every
batch goes to the worker. Each `mash index` result reports the texts the worker
embedded (`remote_inferences`, out of `model_inferences`) and its request time
(`remote_milliseconds`).

## Remote reranking

Searches rerank their best candidates on your CPU by default. To score them on
an authenticated Mash reranking worker, add:

```toml
[reranking]
backend = "remote"
url = "https://YOUR-RERANKING-WORKER.modal.run"
token_env = "MASH_RERANKING_TOKEN"
timeout_milliseconds = 1500
```

This sends the query and the excerpts being ranked to the worker.

- Set the variable named by `token_env` in every process that searches: MCP
  servers, the dashboard and your shell. A Modal proxy token for the embedding
  worker also works here.
- `timeout_milliseconds` accepts 100 to 10000. A cold worker takes much longer
  than that.
- Keep `mash models install` in place. When the worker does not answer in time
  or the token is missing, that search reranks on your CPU. Failed requests are
  never retried.
- `mash search --diagnostics` reports `reranker_engine` as `remote`, `cpu` or
  `cpu-fallback`, with a `reranker_fallback` reason.
- When searching by meaning is your default (`intent = "related-discussion"`
  with reranking), MCP and dashboard start, and the agent plugin's
  session-start refresh, send a text-free warm-up request to the worker. With
  word search as the default the worker is left alone until a search by
  meaning needs it. Set `MASH_RERANK_WARMUP=off` to disable warm-ups.

## Memory capture

With the current agent plugin, new outcomes are captured automatically. To
disable capture and explicit backfill:

```toml
[records]
enabled = false
```

Recall of existing memory remains available. The optional `backfill_since` is a
setup-time cutoff in Unix milliseconds; it does not schedule work. Historical
chats are reviewed only during an explicitly requested
[memory backfill](records.md#backfill-earlier-memory).

## Local Qdrant

The defaults are HTTP port 6333 and gRPC port 6334 on localhost. For a service
at another address, edit:

```toml
[qdrant]
url = "http://127.0.0.1:6333"
grpc_url = "http://127.0.0.1:6334"
memory = "durable"
# api_key_env = "MASH_LOCAL_QDRANT_KEY"
```

Both endpoints must reach the same Qdrant 1.20+ instance. If an API key is
required, put it in the named environment variable, not in the URL.

`mash qdrant install` serves the ports configured here, so set them before
running it, or run it again after changing them. It accepts only plain `http`
addresses on this machine without `api_key_env`; any other endpoint is a
service you run yourself.

## Share across your machines

A local-only setup needs no `[shared]` table. To publish as well, follow
[multi-machine setup](shared-index.md), add its connection settings, and set
`[index] destination = "both"`.

Each machine publishes its own indexed history. Use a different node ID and
token on each machine, with the same group ID and server URL.
