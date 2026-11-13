"""Explicitly deploy an authenticated GPU reranker; importing does not launch jobs."""

import asyncio
import threading
from pathlib import Path
from time import perf_counter

import modal
from fastapi import HTTPException, Request, Response

from contract import (
    MAX_BODY_BYTES,
    REPOSITORY,
    REQUEST_MAGIC,
    REVISION,
    ContractError,
    Document,
    decode_batch,
    decompress_request,
    encode_reply,
)
from scoring import MODEL_FILES, load_tokenizer, score

app = modal.App("mash-reranking")
MODEL_ROOT = Path("/opt/mash-reranker")


def download_model() -> None:
    """Runs during explicit image construction, before a GPU is allocated."""
    from huggingface_hub import snapshot_download

    snapshot_download(
        REPOSITORY,
        revision=REVISION,
        local_dir=str(MODEL_ROOT),
        allow_patterns=MODEL_FILES,
    )


image = (
    modal.Image.from_registry("nvidia/cuda:12.4.1-cudnn-runtime-ubuntu22.04", add_python="3.12")
    .pip_install(
        "onnxruntime-gpu==1.23.2",
        "numpy==2.2.6",
        "tokenizers==0.22.1",
        "huggingface-hub>=0.35,<1",
        "fastapi>=0.115,<1",
        "zstandard>=0.23,<1",
    )
    .add_local_file(Path(__file__).with_name("contract.py"), "/root/contract.py", copy=True)
    .add_local_file(Path(__file__).with_name("scoring.py"), "/root/scoring.py", copy=True)
    .run_function(download_model)
)


# A separate app from embedding, so searches never queue behind indexing
# batches. A T4 is ample for a six-layer cross-encoder over at most 64 short
# pairs. Mash warms the worker when a search is likely; fifteen idle minutes
# outlast the plugin's ten-minute catch-up interval.
@app.cls(
    image=image,
    gpu=["T4", "L4"],
    cpu=1,
    memory=4096,
    timeout=60,
    startup_timeout=180,
    max_containers=1,
    min_containers=0,
    scaledown_window=900,
    retries=0,
)
@modal.concurrent(max_inputs=4)
class Worker:
    @modal.enter()
    def load(self) -> None:
        import onnxruntime as ort

        self.tokenizer = load_tokenizer(MODEL_ROOT)
        options = ort.SessionOptions()
        # ONNX schedules small shape operations on CPU even with CUDA active.
        # Require the CUDA provider below and disable whole-session fallback.
        self.session = ort.InferenceSession(
            str(MODEL_ROOT / "onnx/model.onnx"), options, providers=["CUDAExecutionProvider"]
        )
        if self.session.get_providers()[0] != "CUDAExecutionProvider":
            raise RuntimeError("CUDA inference is unavailable")
        self.session.disable_fallback()
        # One GPU: overlap request transfer with inference, never inference itself.
        self.compute = threading.Lock()
        # Initialize CUDA kernels before the first search arrives.
        score(self.session, self.tokenizer, "warm up", ["warm up"])

    def infer(self, query: str, documents: list[Document]) -> tuple[int, list[tuple[int, float]]]:
        with self.compute:
            started = perf_counter()
            try:
                scores = score(
                    self.session, self.tokenizer, query, [item.text for item in documents]
                )
            except ValueError:
                raise HTTPException(500, "invalid model output") from None
            inference_milliseconds = round((perf_counter() - started) * 1000)
        return inference_milliseconds, [
            (item.id, value) for item, value in zip(documents, scores, strict=True)
        ]

    @modal.fastapi_endpoint(method="POST", requires_proxy_auth=True, docs=False)
    async def rerank(self, request: Request) -> Response:
        body = bytearray()
        async for chunk in request.stream():
            if len(body) + len(chunk) > MAX_BODY_BYTES:
                raise HTTPException(413, "reranking request exceeds byte limit")
            body.extend(chunk)
        raw = bytes(body)
        encoding = request.headers.get("content-encoding")
        if encoding is not None:
            if encoding != "zstd":
                raise HTTPException(415, "unsupported content encoding")
            try:
                raw = decompress_request(raw)
            except ContractError:
                raise HTTPException(400, "invalid reranking batch") from None
        if raw[:4] != REQUEST_MAGIC:
            raise HTTPException(400, "invalid reranking batch")
        try:
            batch = decode_batch(raw)
        except ContractError as error:
            # Contract messages never contain input text; keep the recipe and
            # limit reasons distinguishable for the client.
            raise HTTPException(400, str(error)) from None
        if not batch.documents:
            # Warm-up: the container and model are loaded; no inference.
            return Response(
                encode_reply(batch.request_id, 0, []), media_type="application/octet-stream"
            )
        inference_milliseconds, scores = await asyncio.to_thread(
            self.infer, batch.query, batch.documents
        )
        try:
            reply = encode_reply(batch.request_id, inference_milliseconds, scores)
        except ContractError:
            raise HTTPException(500, "invalid model output") from None
        return Response(reply, media_type="application/octet-stream")
