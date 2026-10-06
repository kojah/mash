# Fresh MiniLM / Voyage 4 / Voyage Code 4 comparison

Production and the frozen complete-answer-v1 baseline remain unchanged.

Matched cohort: 156 threads; 104 calibration / 52 evaluation. 4 histories excluded by credential screening.

This is a thread-separated holdout within previously examined development data, with provisional agent-authored labels. Original confirmation stays sealed. Do not treat this as independent final validation.

Each arm received the same 48 Optuna trials. Parameters and view were selected using calibration F1 only. Evaluation results did not select or retune models.

## Implementation audit

Response indices/model/dimensions, finiteness, unit norms, frozen source hashes, point ownership and complete-source splitting were validated. Reconstructed MiniLM fixed predictions match the frozen baseline exactly.

Original targeted-pooling inputs: 0. Prior Code 4 codec parity checked 156 histories: maximum coordinate difference 0.016327537; mean corresponding-vector cosine 0.9999908187. The first remote cache discarded raw floats; this study retains them before any normalization/quantization.

## Evaluation after calibration

| Representation | Precision | Recall | F1 | Long F1 | Distant recall | F1 gain vs frozen control (95% CI) |
|---|---:|---:|---:|---:|---:|---|
| minilm | 82.00% | 84.97% | 83.46% | 77.21% | 57.41% | +0.00 pp [+0.00, +0.00] |
| code-float | 78.03% | 90.16% | 83.65% | 77.25% | 68.52% | +0.19 pp [-3.43, +3.87] |
| code-int8 | 77.93% | 89.64% | 83.37% | 76.72% | 66.67% | -0.09 pp [-3.79, +3.66] |
| code-complete | 80.61% | 81.87% | 81.23% | 72.82% | 61.11% | -2.23 pp [-6.75, +2.03] |
| general-float | 80.37% | 91.19% | 85.44% | 80.35% | 74.07% | +1.98 pp [-2.14, +6.19] |
| general-int8 | 80.45% | 91.71% | 85.71% | 80.87% | 74.07% | +2.25 pp [-1.46, +6.15] |
| general-complete | 83.66% | 87.56% | 85.57% | 80.18% | 70.37% | +2.11 pp [-0.86, +5.27] |

Intervals use 2,000 paired thread bootstrap resamples; they are exploratory and unadjusted for multiple arms.

## Chosen parameters

| Arm | Radius | Cutoff | Relative margin | View |
|---|---:|---:|---:|---|
| minilm-selected | 0.60 | 0.75 | 0.05 | union |
| code-float-selected | 0.60 | 0.70 | 0.00 | union |
| code-int8-selected | 0.60 | 0.70 | 0.00 | union |
| code-complete-selected | 0.50 | 0.75 | 1.00 | pooled |
| general-float-selected | 0.50 | 0.75 | 0.05 | union |
| general-int8-selected | 0.50 | 0.75 | 0.05 | union |
| general-complete-selected | 0.50 | 0.75 | 0.05 | union |

## Fixed-setting view and codec ablations

Radius 0.60, cutoff 0.75, relative margin 0.05; evaluation threads only.

| Arm | Pooled F1 | Chunk F1 | Union F1 | Union precision | Union recall |
|---|---:|---:|---:|---:|---:|
| minilm | 81.08% | 75.52% | 83.46% | 82.00% | 84.97% |
| code-float | 77.23% | 71.88% | 81.52% | 79.70% | 83.42% |
| code-int8 | 77.23% | 72.25% | 81.52% | 79.70% | 83.42% |
| code-complete | 79.23% | 78.51% | 80.66% | 74.03% | 88.60% |
| general-float | 80.00% | 82.99% | 80.86% | 69.12% | 97.41% |
| general-int8 | 80.00% | 82.90% | 80.60% | 69.00% | 96.89% |
| general-complete | 80.45% | 79.90% | 80.36% | 70.59% | 93.26% |

## Calibrated model comparisons

| Arm | F1 gain vs calibrated MiniLM | 95% paired interval |
|---|---:|---|
| code-float | +0.19 pp | [-3.43, +3.87] |
| code-int8 | -0.09 pp | [-3.79, +3.66] |
| code-complete | -2.23 pp | [-6.75, +2.03] |
| general-float | +1.98 pp | [-2.14, +6.19] |
| general-int8 | +2.25 pp | [-1.46, +6.15] |
| general-complete | +2.11 pp | [-0.86, +5.27] |

## API usage

| Model | Billed tokens | Recorded cost |
|---|---:|---:|
| voyageai/voyage-code-4 | 5,564,920 | $0.667790 |
| voyageai/voyage-4 | 5,564,550 | $0.333873 |

Raw embeddings, receipts, per-thread predictions, Optuna and MLflow provenance remain in private research storage. All variants can now be rescored without additional embedding charges.

Complete-turn inputs join current user prompts and all visible assistant segments with role markers. They use a lossless conservative 28,000-byte structural budget, normalized mean pooling only for oversized turns. This changes both input granularity and external role weighting; it does not isolate context length alone. No LLM summaries are used.

## Repeat-call stability probe

A diagnostic three-input Code 4 repeat call, using differently batched inputs, produced corresponding raw-vector cosines 0.99980824–0.99994350, with maximum coordinate difference 0.00257826. Its $0.00004440 charge is included above. This does not determine whether variation comes from batching, server numerics or other provider behavior. It is not a full clustering stability study. The probe overlapped the general-model build, briefly adding one request beyond the two-request builder limit; no quality trial or configuration was added. Float/int8 comparisons use the same fresh raw vectors, avoiding this repeat-call confound.

## Harness correction

The initial queued trial was counted as completed, yielding 47 rather than 48 executed trials. Detected after MiniLM evaluation, before remote-arm evaluation. Restarted calibration from scratch on the unchanged split and raw cache with 48 completed trials per arm. The initial local winner was the frozen configuration. No new inference, labels, configuration choices, or model selection were introduced. Original runner hash and preliminary records are archived.

## Interpretation

Model-specific calibration matters: Code 4 float union improves from 81.52% at the local settings to 83.65%; Voyage 4 float improves from 80.86% to 85.44%. The local frozen configuration remains its calibration winner.

Float versus int8 changes little at fixed settings. Code 4 fixed union has identical aggregate precision/recall/F1 in both precisions; Voyage 4 differs slightly. This does not implicate quantization as the main cause of the original regression.

View quality is model-dependent: Voyage 4 chunk-only outperforms the other chunk-only arms at the fixed settings, whereas Code 4 chunk-only is weaker. A naive union can increase false positives: fixed Voyage 4 union has high recall but low precision. Calibration narrows its radius from 0.60 to 0.50 and restores a more useful balance.

Complete-turn Code 4 is weaker than its calibrated short-window version; complete-turn Voyage 4 trades some recall for precision with a similar overall F1. Direct whole-turn embedding is not a universal improvement.

Voyage 4 is promising for distant-turn recall, but all remote F1 gain intervals include zero. The frozen local baseline stays unchanged. These observations do not establish model equivalence or a statistically clear winner. Bootstrap intervals capture sampled-thread uncertainty, not provider inference variability or annotation bias.
