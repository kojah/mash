# Configure Mash

Start with the TOML file created by `mash init`. Run `mash status` to see its
resolved location, or select a file explicitly with `mash --config PATH status`.
Use TOML for this file.

To change history selection, coverage, or sharing interactively, rerun
`mash init` in a terminal and choose **Review and edit settings**. The wizard
preserves your other settings and keeps the previous file as a backup; comments
and formatting are regenerated when saving edits.

For one machine, keep the local setup created by `mash init`. Choose your
history folders and search preferences below; shared connection settings are
only needed when you enable multi-machine sharing.

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
files are never modified.

To include all ages permanently, replace `since_days = 90` with
`all_history = true`. Do not set both. One-off indexing flags such as
`--all-history` override the configured age selection.

## Embedding inference

Embedding generation runs locally by default. To send indexing batches to an
authenticated Mash embedding worker, add:

```toml
[embeddings]
backend = "remote"
url = "https://YOUR-WORKER.modal.run"
token_env = "MASH_EMBEDDING_TOKEN"
timeout_seconds = 120
```

Set the named environment variable in the process running `mash index`, including
watchers. Use a Modal proxy token in `TOKEN_ID.TOKEN_SECRET` form, not your Modal
account API key. The credential value is never written to configuration or status.
HTTPS is required except for a worker on loopback.

This opt-in sends prepared searchable text to the worker, which may contain
sensitive conversation content. It does not grant the worker database access.
Search-query embeddings and reranking remain local; install local models with
`mash models install` for offline semantic searches.

A worker that shuts down when idle can take minutes to start, and search waits
for new messages to be embedded. To keep routine refreshes quick, add
`local_below` to the table:

```toml
local_below = 256
```

A batch with fewer new texts than this is embedded by the local model, which
needs `mash models install`; larger batches, such as a first index or a
backfill, still go to the worker. Both produce the same vectors. The value can
be at most 1024, and leaving it out sends every batch to the worker. Each
`mash index` result reports how many texts the worker embedded
(`remote_inferences`, out of `model_inferences`) and how long its requests took
(`remote_milliseconds`).

Existing matching vectors are reused. Failed remote requests return an error;
Mash does not automatically retry the request or silently switch to local
inference. A timed-out request may still have incurred GPU cost. Check the worker
before explicitly retrying indexing. To return to local inference, remove the
table or set only `backend = "local"`.

The worker must implement Mash's model-specific contract; an arbitrary embedding
API is not interchangeable. Changing the embedding recipe can require rebuilding
search data. Deployment instructions live in `deployment/embeddings/README.md`
in the source repository.

## Memory capture

Automatic capture of new outcomes is enabled by default when using the current
agent plugin. To disable capture and explicit backfill:

```toml
[records]
enabled = false
```

The optional `backfill_since` is a setup-time cutoff in Unix milliseconds. It
does not schedule work. Historical chats are reviewed only during an explicitly
requested [memory backfill](records.md#backfill-earlier-memory), never as part
of a newer conversation. Recall of existing memory remains available.

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
[semantic model setup](indexing.md#enable-semantic-search); changing intent alone
does not create dense search data.

## Share across your machines

A local-only setup needs no `[shared]` table. To publish as well, follow
[multi-machine setup](shared-index.md), add its connection settings, and set
`[index] destination = "both"`.

Each machine publishes its own indexed history. Use a different node ID and
token on each machine, with the same group ID and server URL.

## Local embeddings

Semantic search uses the installed all-MiniLM-L6-v2 model (384 dimensions).
Run `mash models install` explicitly to install embedding and reranking models;
indexing and search never download models. See [model setup](indexing.md#enable-semantic-search).

The same embedding space is used for local and shared search. A server needs
local model assets when rebuilding shared views from saved history.
