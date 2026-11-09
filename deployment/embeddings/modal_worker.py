"""Explicitly deploy an authenticated GPU worker; importing does not launch jobs."""

from pathlib import Path
from time import perf_counter

import modal
from fastapi import HTTPException, Request, Response
from pydantic import ValidationError

from contract import (
    MAX_BODY_BYTES,
    REQUEST_MAGIC,
    REVISION,
    ContractError,
    decode_batch,
    encode_reply,
)
from legacy import decode_legacy_batch, encode_legacy_reply

app = modal.App("mash-embeddings")
MODEL_ROOT = "/opt/mash-model"


def download_model() -> None:
    """Runs during explicit image construction, before a GPU is allocated."""
    from huggingface_hub import snapshot_download

    snapshot_download(
        "Xenova/all-MiniLM-L6-v2",
        revision=REVISION,
        local_dir=MODEL_ROOT,
        allow_patterns=["onnx/model.onnx", "tokenizer.json"],
    )


image = (
    modal.Image.from_registry("nvidia/cuda:12.4.1-cudnn-runtime-ubuntu22.04", add_python="3.12")
    .pip_install(
        "onnxruntime-gpu==1.23.2",
        "numpy==2.2.6",
        "tokenizers==0.22.1",
        "huggingface-hub>=0.35,<1",
        "fastapi>=0.115,<1",
        "pydantic>=2.12,<3",
        "orjson>=3.11,<4",
    )
    .add_local_file(Path(__file__).with_name("contract.py"), "/root/contract.py", copy=True)
    .add_local_file(Path(__file__).with_name("legacy.py"), "/root/legacy.py", copy=True)
    .run_function(download_model)
)


@app.cls(
    image=image,
    gpu=["L4", "A10"],
    cpu=2,
    memory=8192,
    timeout=120,
    startup_timeout=180,
    max_containers=1,
    min_containers=0,
    scaledown_window=30,
    retries=0,
)
class Worker:
    @modal.enter()
    def load(self) -> None:
        import onnxruntime as ort
        from tokenizers import Tokenizer

        self.tokenizer = Tokenizer.from_file(f"{MODEL_ROOT}/tokenizer.json")
        self.tokenizer.enable_truncation(max_length=512)
        self.tokenizer.enable_padding(pad_id=0, pad_token="[PAD]")
        options = ort.SessionOptions()
        # ONNX schedules small shape operations on CPU even with CUDA active.
        # Require the CUDA provider below and disable whole-session fallback.
        self.session = ort.InferenceSession(
            f"{MODEL_ROOT}/onnx/model.onnx", options, providers=["CUDAExecutionProvider"]
        )
        if self.session.get_providers()[0] != "CUDAExecutionProvider":
            raise RuntimeError("CUDA inference is unavailable")
        self.session.disable_fallback()

    @modal.fastapi_endpoint(method="POST", requires_proxy_auth=True, docs=False)
    async def embed(self, request: Request) -> Response:
        import numpy as np

        body = bytearray()
        async for chunk in request.stream():
            if len(body) + len(chunk) > MAX_BODY_BYTES:
                raise HTTPException(413, "embedding request exceeds byte limit")
            body.extend(chunk)
        binary = body[:4] == REQUEST_MAGIC
        request_id: bytes | None = None
        legacy_id: str | None = None
        try:
            if binary:
                decoded = decode_batch(bytes(body))
                request_id = decoded.request_id
                inputs = decoded.inputs
            else:
                legacy_id, inputs = decode_legacy_batch(bytes(body))
        except (ContractError, ValidationError):
            # Diagnostics may echo input text; never return them.
            raise HTTPException(400, "invalid embedding batch") from None
        started = perf_counter()
        results: list[tuple[int, bytes]] = []
        legacy_results: list[tuple[int, list[float]]] = []
        # Amortize network cost with a larger request while bounding the GPU's
        # attention workspace and padding separately for each inference batch.
        for start in range(0, len(inputs), 32):
            chunk = inputs[start : start + 32]
            encodings = self.tokenizer.encode_batch([item.text for item in chunk])
            feed = {
                "input_ids": np.asarray([item.ids for item in encodings], dtype=np.int64),
                "attention_mask": np.asarray(
                    [item.attention_mask for item in encodings], dtype=np.int64
                ),
                "token_type_ids": np.asarray([item.type_ids for item in encodings], dtype=np.int64),
            }
            outputs = np.asarray(self.session.run(None, feed)[0], dtype=np.float32)
            mask = feed["attention_mask"][..., None].astype(np.float32)
            vectors = (outputs * mask).sum(axis=1) / mask.sum(axis=1).clip(min=1)
            vectors /= np.linalg.norm(vectors, axis=1, keepdims=True).clip(min=1e-12)
            if vectors.shape != (len(chunk), 384) or not np.isfinite(vectors).all():
                raise HTTPException(500, "invalid model output")
            if binary:
                # Halves are enough for the int8 vectors Mash stores, and keep
                # the response five times smaller than JSON float32 text.
                encoded = vectors.astype("<f2", copy=False)
                results.extend(
                    (item.id, row.tobytes())
                    for item, row in zip(chunk, encoded, strict=True)
                )
            else:
                legacy_results.extend(
                    (item.id, values) for item, values in zip(chunk, vectors.tolist(), strict=True)
                )
        inference_milliseconds = round((perf_counter() - started) * 1000)
        if request_id is not None:
            reply = encode_reply(request_id, inference_milliseconds, results)
            return Response(reply, media_type="application/octet-stream")
        assert legacy_id is not None
        reply = encode_legacy_reply(legacy_id, inference_milliseconds, legacy_results)
        return Response(reply, media_type="application/json")
