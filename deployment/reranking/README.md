# Modal reranking worker

This optional worker scores search candidates for Mash's contextual reranking.
It is a separate Modal app from the embedding worker, so searches never queue
behind indexing batches. Deploying it builds an image and downloads the pinned
model; invoking it starts billable GPU work. Local CPU reranking remains the
default and the offline path.

From this directory, using your existing Modal account configuration:

```sh
uv sync --locked
uv run modal deploy modal_worker.py
```

Use a Modal proxy token (the embedding worker's token works) as the
`token_env` variable, and the emitted `rerank` endpoint URL in the node's
`[reranking]` configuration (see `docs/configuration.md`). Keep the account API
credential used for deployment separate from this invocation token. See
[Modal proxy authentication](https://modal.com/docs/guide/webhook-proxy-auth).

The worker permits one T4 (or L4) container, no warm pool, a 60-second
invocation timeout and no automatic retries. It scales down after 15 idle
minutes, longer than the plugin's ten-minute catch-up interval, and Mash sends
a text-free warm-up when a search is likely. Each request holds one query of at
most 16 KiB and at most 64 documents totalling one MiB of UTF-8 text. Request
bodies and errors never log or echo chat text.

Requests and replies use versioned binary frames (`MRK1`/`MRR1`); an empty
batch with an empty query is a warm-up and runs no inference. Replies carry one
little-endian f32 raw logit per document ID. Requests above one kilobyte are
zstd compressed and marked `Content-Encoding: zstd`.

The recipe pins `jinaai/jina-reranker-v1-turbo-en` at the revision Mash
installs, FP32 ONNX inference, FastEmbed's pair tokenization (512-token
longest-first truncation, batch-longest padding) and the first logit. Mash
refuses the worker when its installed reranker revision differs, and validates
every reply before using it. CUDA execution is required; the worker fails
rather than falling back to CPU.

Local contract checks do not invoke Modal. Set `MASH_TEST_RERANKER_DIR` to an
installed reranker snapshot to also check tokenization and CPU scoring:

```sh
uv run ruff check .
uv run ty check .
uv run pytest -q
```
