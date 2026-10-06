# Model-dependent capacity rerun

Frozen reference: voyage4-joint-v1. All 156 matched histories are development data; the original confirmation remains sealed. Same 104/52 subdivision, 48 identical Optuna draws per arm. These are exploratory, reused-development outcomes.

## Results after calibration

| Model | Dimensions | Input budget | Precision | Recall | F1 | Long F1 | Distant recall | F1 gain interval vs reference (pp) |
|---|---:|---|---:|---:|---:|---:|---:|---|
| voyage-code-4 | 1024 | legacy_bytes | 80.61% | 81.87% | 81.23% | 72.82% | 61.11% | [-8.82, -0.48] |
| voyage-code-4 | 1024 | model_tokens | 82.68% | 76.68% | 79.57% | 68.78% | 44.44% | [-11.32, -1.45] |
| voyage-code-4 | 2048 | legacy_bytes | 75.21% | 92.75% | 83.06% | 76.92% | 81.48% | [-5.78, +0.77] |
| voyage-code-4 | 2048 | model_tokens | 75.21% | 92.75% | 83.06% | 76.92% | 81.48% | [-5.78, +0.77] |
| voyage-4 | 1024 | legacy_bytes | 83.66% | 87.56% | 85.57% | 80.18% | 70.37% | [+0.00, +0.00] |
| voyage-4 | 1024 | model_tokens | 83.66% | 87.56% | 85.57% | 80.18% | 70.37% | [+0.00, +0.00] |
| voyage-4 | 2048 | legacy_bytes | 81.19% | 84.97% | 83.04% | 75.47% | 66.67% | [-5.92, +0.36] |
| voyage-4 | 2048 | model_tokens | 81.28% | 85.49% | 83.33% | 76.06% | 66.67% | [-5.54, +0.62] |

## Frozen-setting ablations

Radius .50, absolute cosine .75, relative margin .05 and union; only model/dimensions/input construction differ.

| Model | Dimensions | Input budget | Precision | Recall | F1 | Distant recall |
|---|---:|---|---:|---:|---:|---:|
| voyage-code-4 | 1024 | legacy_bytes | 82.68% | 76.68% | 79.57% | 44.44% |
| voyage-code-4 | 1024 | model_tokens | 82.68% | 76.68% | 79.57% | 44.44% |
| voyage-code-4 | 2048 | legacy_bytes | 85.56% | 79.79% | 82.57% | 46.30% |
| voyage-code-4 | 2048 | model_tokens | 85.56% | 79.79% | 82.57% | 46.30% |
| voyage-4 | 1024 | legacy_bytes | 83.66% | 87.56% | 85.57% | 70.37% |
| voyage-4 | 1024 | model_tokens | 83.66% | 87.56% | 85.57% | 70.37% |
| voyage-4 | 2048 | legacy_bytes | 83.42% | 86.01% | 84.69% | 72.22% |
| voyage-4 | 2048 | model_tokens | 83.50% | 86.53% | 84.99% | 72.22% |

## Procedure and interpretation

The model-specific public tokenizers share the same pinned tokenizer.json bytes. Context limits and dimensions are declared in validated capabilities; no MiniLM tokenizer, 512-token cap, 384-dimensional shape, or implicit MiniLM reference is used in the new driver. The frozen manifest supplies reference settings. Full source characters survive splitting, with no LLM summarization or other-thread context.

Only 14 turns change segmentation under the larger token budget; the longest full turn has 24,544 model tokens, so all fit within 31,992 tokens. A larger context limit cannot affect the many turns already fitting the byte budget. Additional dimensions do not guarantee better clustering: they change geometry and the useful threshold/radius tradeoff. Code 4 at 2048 dimensions recovers more related pairs after calibration but introduces additional false positives. Voyage 4 remains strongest here at the frozen 1024-dimensional configuration. Neither a dimension increase nor a context increase establishes a positive F1 gain against the reference. Both calibrated Code 4 1024-dimensional F1-gain intervals are entirely negative; other non-reference calibrated intervals include zero. These exploratory intervals are unadjusted for multiple arms and reuse development labels. Do not claim equivalence or an independently verified winner. Fixed settings isolate representation changes; calibrated results additionally include training-subset parameter selection.

New recorded API charges: $0.381081. Imported exact-text 1024-dimensional raw vectors remain distinct from new 2048-dimensional spaces. The cache key includes model, dimensions, task and tokenizer hash; text hashes encode input construction. Provider weights cannot be pinned and repeat inference can vary, so fresh-versus-cached comparisons retain that limitation.

## Implementation and verification

The local Python encoder derives hidden size, context limit and padding from installed model/tokenizer metadata, handles optional token-type inputs and validates returned hidden-state shapes. The installed MiniLM generation remains unchanged. Production int8 cosine conversion now permits larger vector spaces instead of asserting the old exact-integer range. Production remote configuration derives 1024-dimensional defaults for recognized Voyage 4 family names (including OpenRouter-prefixed names), retains 384 only for a recognized MiniLM model name, and requires explicit dimensions for unknown/custom models. Explicit dimensions always override known-model defaults. MLflow summaries require an explicit reference; historical public runners explicitly retain their original reference.

163 Python tests and the initial full run of 638 default Rust tests pass, including a 2048-dimensional cosine regression. The subsequent model-default and status regressions both pass; a broad rerun encounters unrelated export timeouts and a wider remote-test run hits an HTTP-fixture timeout under filesystem contention. Rust format and documentation checks pass. Stable library Clippy passes after the configuration change. Full nightly Clippy encounters compiler ICEs and an unrelated pre-existing Qdrant lint. An isolated checkout on stable exposes nine existing exact-float test lints; all-target Clippy passes there with only clippy::float_cmp allowed. No unrelated working-tree edits were reverted or included.

All 384 current calibration runs, 16 scoring runs and 1,248 matrix files are verified separately. Current-contract source is archived under executed-source with exact hashes; later baseline-ID-only changes to shared tracking callers preserve the algorithm and historical labels. Frozen Voyage reference replay is exact. Raw chats, annotations, vectors, detailed predictions, receipts and catalogues remain private.

See [the registered protocol](voyage-model-capacity-protocol.md). The frozen baseline and production default model remain unchanged.

## Paired isolated effects at frozen settings

These compare one capacity change at a time; no calibration-choice change is included. Intervals use the same 2,000 paired thread resamples and are exploratory/unadjusted.

| Model | Change | Other setting | F1 change (pp) | 95% paired interval (pp) |
|---|---|---|---:|---|
| voyage-4 | 1024 → 2048 dimensions | legacy_bytes | -0.88 | [-3.29, +1.38] |
| voyage-4 | 1024 → 2048 dimensions | model_tokens | -0.58 | [-2.76, +1.55] |
| voyage-4 | Byte → token budget | 1024 dimensions | +0.00 | [+0.00, +0.00] |
| voyage-4 | Byte → token budget | 2048 dimensions | +0.29 | [+0.00, +1.00] |
| voyage-code-4 | 1024 → 2048 dimensions | legacy_bytes | +3.00 | [+0.60, +6.15] |
| voyage-code-4 | 1024 → 2048 dimensions | model_tokens | +3.00 | [+0.60, +6.15] |
| voyage-code-4 | Byte → token budget | 1024 dimensions | +0.00 | [+0.00, +0.00] |
| voyage-code-4 | Byte → token budget | 2048 dimensions | +0.00 | [+0.00, +0.00] |

## Scoring runtime

Per-arm elapsed seconds include cache reads, matrix construction/compression, four-radius geometry construction, calibration/tracking and bootstrap/scoring; they exclude prior API/preflight work. Filesystem contention and cache reuse prevent treating them as ingestion benchmarks.

| Case | Seconds |
|---|---:|
| voyage-code-4-1024-legacy_bytes | 19.7 |
| voyage-code-4-1024-model_tokens | 82.3 |
| voyage-code-4-2048-legacy_bytes | 22.3 |
| voyage-code-4-2048-model_tokens | 24.2 |
| voyage-4-1024-legacy_bytes | 27.7 |
| voyage-4-1024-model_tokens | 72.4 |
| voyage-4-2048-legacy_bytes | 19.3 |
| voyage-4-2048-model_tokens | 26.3 |
