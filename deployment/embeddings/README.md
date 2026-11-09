# Modal embedding worker

This optional worker generates indexing vectors for Mash. Query inference remains
local. Deploying it builds an image and downloads the pinned model; invoking it
starts billable GPU work. No background deployment or model download happens on
ordinary Mash startup.

From this directory, using your existing Modal account configuration:

```sh
uv sync --locked
uv run modal deploy modal_worker.py
```

Create a dedicated proxy token through Modal's workspace settings. Export its
ID and secret joined by a period as `MASH_EMBEDDING_TOKEN` in the indexing process.
Use the emitted endpoint URL in the node's `[embeddings]` configuration. Keep the
account API credential used for deployment separate from this invocation token.
See [Modal proxy authentication](https://modal.com/docs/guide/webhook-proxy-auth).

The worker permits one L4/A10 container, no warm pool, a 120-second invocation
timeout, and no automatic retries. It retains a loaded model while the container
is warm and scales down after 30 idle seconds. Each request holds at most 256 inputs
and one MiB of UTF-8 text. The worker runs GPU batches of at most 32 inputs to bound
memory use. Request bodies and errors never log or echo chat text.

Requests and replies use versioned binary frames (`MSH1`/`MSR1`). Inputs stay
UTF-8 text with explicit lengths, and reply vectors travel as little-endian f16,
which carries more precision than the int8 vectors Mash stores while keeping the
payload about ten times smaller than JSON. Requests above one kilobyte are zstd
compressed and marked `Content-Encoding: zstd`; smaller bodies stay uncompressed.
The worker accepts only this versioned binary contract.

The recipe pins Xenova's MiniLM model revision, FP32 ONNX inference, 512-token
truncation, attention-mask mean pooling and L2 normalization. Mash validates the
recipe and each vector, then performs its common quantization before storage.
CUDA execution is required; the worker rejects unavailable GPU execution rather
than silently falling back to CPU. This deployment is experimental until its
end-to-end benchmark and retrieval checks have been completed.

Local contract checks do not download models or invoke Modal:

```sh
uv run ruff check .
uv run ty check .
uv run pytest -q
```
