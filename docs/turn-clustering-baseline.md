# Current experimental baseline: voyage4-joint-v1

Frozen by user decision on 2026-10-06 after the
[calibrated encoder comparison](voyage-calibration-results.md). This is the
reference for future turn-clustering experiments. Its exact contract is
[turn-clustering-baseline.toml](turn-clustering-baseline.toml), using the executed
implementation at Git revision `a2698cc`.

The historical [complete-answer-v1 reference](complete-answer-baseline.md) and
[original manifest](complete-answer-baseline.toml) remain preserved. MiniLM stays
the local production encoder; this freeze does not deploy a remote model or
change message embeddings, indexing or search behavior.

| Component | Frozen choice |
|---|---|
| Encoder | Voyage 4 via OpenRouter, 1024-dimensional raw float responses |
| Inputs | Current lean prompts plus every visible assistant segment in the turn, with role markers |
| Joint embedding | Embed the combined text once when it fits the conservative 28,000-byte budget |
| Oversized turns | Lossless paragraph/line splitting, with UTF-8-safe fallback; no source characters discarded |
| Pooled view | Normalize the mean of normalized piece vectors |
| Piece view | Retain normalized individual joint-piece vectors |
| Smoothing | Original branch-aware alpha .10/window four, pooled view only |
| Clustering | BIRCH radius .50; membership cosine .75; relative-best margin .05 |
| Retention and fusion | Two distinct turns per primary/surfaced group; namespaced membership union |
| Additional denoising, summaries and cross-thread context | Disabled |

Most turns have one joint vector. For those turns, both views start with that
same vector; smoothing changes only the pooled view. The two-view union remains
part of the measured baseline. Do not silently replace it with pooled-only
clustering or introduce overlapping MiniLM-sized windows.

## Recorded evidence and freeze rules

On the former 52-thread development evaluation subset, precision is 83.66%,
recall 87.56%, F1 85.57%, long-thread F1 80.18%, and distant recall 70.37%
(38/54 related pairs). The paired F1-gain interval against MiniLM is
[-0.86, +5.27] percentage points and includes zero. These are provisional
agent-authored labels, not production acceptance evidence.

- Treat all 156 matched histories, including the former 52-thread evaluation
  subset, as development data from this point onward. Choosing this baseline
  after viewing evaluation results consumes that subset for model selection.
- Keep the original confirmation set sealed. Confirmation evaluation requires
  a separate, registered next step; this freeze does not run it.
- Preserve parameters, text construction, source ordering, smoothing, retention,
  memberships, cached vectors and recorded results. New changes must be named
  candidates; do not retune the frozen reference against these labels.
- Verify the selected report, split, annotations, executed code and every
  matrix against the manifest/catalogues before reuse. Private catalogues and
  vectors remain outside Git. No inference or paid API calls are needed to
  freeze or reuse these cached matrices.
- Remote weights cannot be pinned, and repeat calls can vary slightly. The
  cached matrices define the exact experimental reference; fresh inference
  is a new build whose stability must be checked before comparing results.

## Remaining MiniLM assumptions

The joint-turn builder uses UTF-8 byte bounds and provider-returned vectors;
it does not call the MiniLM tokenizer, use its 512-token limit, or pool its hidden
states. BIRCH, vector normalization and smoothing are dimension-independent.
Smoothing and fusion choices were inherited from the earlier pipeline and remain
explicit frozen choices, rather than claims that they are optimal for Voyage.

Historical Python encoder/build scripts still intentionally pin MiniLM assets,
384 dimensions and a 512-token context, and historical comparison helpers still
pin complete-answer-v1. MLflow `Summary` now requires an explicit baseline ID:
future experiments using this reference set `baseline="voyage4-joint-v1"`.
Historical public runners explicitly name their original control; stored runs
are not rewritten or relabeled.

Production local ONNX/Core ML loading remains MiniLM-specific. Production remote
embedding configuration supports model names and dimensions, including Voyage;
this research freeze does not generalize the local model loader or migrate
production index generations.
