# Apple Silicon embedding sequence buckets

Measured on 2026-09-30 with the installed FP32 MiniLM model, 32-text
batches, and Core ML MLProgram using all available compute units.

The reference uses fixed 512-token padding for 32 texts. The candidate chooses
64/128/256/512 tokens using the model tokenizer and pads directly, without the
old extra long dummy input. This reference is slightly cheaper than the previous
33-input implementation. Both retain the same maximum token length and weights.

Each profile embeds 320 texts over ten batches, after session creation and two
warm-up batches. Inputs repeat a word 8/80/180/400 times, plus a unique batch
index, punctuation, and Unicode text. These are controlled inference benchmarks,
not measured speedups for the entire history-indexing job. Tokenization and bucket
selection are included; there is no embedding-result cache or duplicate reuse.

| Machine | Words per text | Fixed 512 total ms | Bucketed total ms | Speedup |
| --- | ---: | ---: | ---: | ---: |
| Dev MacBook, M2 Max | 8 | 926.50 | 103.36 | 8.96x |
| Dev MacBook, M2 Max | 80 | 924.06 | 204.26 | 4.52x |
| Dev MacBook, M2 Max | 180 | 928.84 | 422.60 | 2.20x |
| Dev MacBook, M2 Max | 400 | 957.29 | 982.05 | 0.97x |
| Personal MacBook, M4 | 8 | 5693.43 | 534.94 | 10.64x |
| Personal MacBook, M4 | 80 | 6016.45 | 1772.42 | 3.39x |
| Personal MacBook, M4 | 180 | 7009.96 | 2579.59 | 2.72x |
| Personal MacBook, M4 | 400 | 6313.83 | 5893.15 | 1.07x |

The dev MacBook was on AC power with Low Power Mode off. The personal MacBook
was on battery with Low Power Mode on; its indexing process was paused during
measurement and automatically resumed. Background system activity and thermal
conditions were not controlled. Treat the long-input differences as timing noise,
not an improvement: both paths select 512 tokens there.

Every tested quantized vector had cosine similarity at least 0.99999988 to the
fixed-512 reference. Tests also cover empty and partial waves, Unicode input,
bucket boundaries, and inputs longer than the 512-token limit. A separate
real-model test checks mixed-length batches against the CPU query backend and
repeatability. The embedding generation stays unchanged because model weights,
token truncation, pooling, and storage quantization are unchanged.

Sessions are loaded lazily, compiled once per shape, and cached on disk in
separate directories. At most four sessions are retained per indexer. First-use
compilation is excluded from the warmed timings above.

Reproduce with explicitly installed models on Apple Silicon:

```bash
MASH_TEST_MODELS_DIR="/path/to/mash/models" cargo test --release --lib \
  coreml_bucketed_batches -- --ignored --nocapture
```

This change also buffers the temporary NDJSON publication file in 256 KiB blocks
and explicitly flushes it before HTTP upload. The HTTP regression test verifies
that a complete snapshot reaches the service and is acknowledged. No isolated
publication-throughput number is claimed here.
