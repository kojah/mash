# Complete-answer prompt-conditioning ablation

Freeze this protocol before constructing or scoring the treatment. The reference
is complete-answer-v1, selected report SHA256
`7e104679c302eb34a8eaac3b35fba7f47e0d854f08938969861c05a072770a35`.
Use only the same 160 development histories and original blind judgments.
Confirmation stays sealed. This reused-development comparison is exploratory.

Register exactly two fixed Optuna trials, `none` and `first80`. No adaptive search,
new annotation, model download, denoising, weighting, cutoff tuning or new fusion.
The baseline remains frozen regardless of this comparison's result.

`none` reuses both complete-answer baseline matrices and inputs unchanged, and
must reproduce all baseline pair predictions and counts exactly. `first80`
reuses the identical pooled matrix, original smoothing alpha .10/window four,
and original user chunk vectors. Reconstruct the frozen heading-aware chunks
with **all own-turn assistant segments**, checking equality to the saved inputs.

For every assistant chunk, prepend the first at most 80 tokens of the turn's
concatenated lean prompts with the existing `Current request` / `Answer excerpt`
wrapper. Pool only answer-excerpt token states; the prompt affects attention.
Preserve existing structural bodies and headings when they fit. If the prefix
causes overflow, split the original body with the existing structural splitter;
reserve ten tokens beyond the prefix count and reject inputs exceeding 512 tokens.
Keep every selected non-whitespace source character. Validate body coverage and
prompt provenance separately. Do not borrow another turn's prompt or add ancestor
context to answer chunks. Original user chunks remain bit-identical.

Use the same installed quantized MiniLM, singleton inference and codec/cache
namespace. No inference batch companions or statistics from other threads.
Existing selected-pooling cache entries may be reused only with identical model,
input bytes and selection spans. Cache transactions remain durable, at 128 vectors.
Freeze protocol, cohort, model, baseline report/matrix and source hashes in a
resume contract. Reject stale/partial artifacts rather than silently overwrite.

Both views retain BIRCH radius .60, membership cutoff .75, distinct-turn support
and namespaced union. Generic penalties and strict consensus remain disabled.
The treatment jointly tests prompt attention and answer-only pooling, as in the
earlier conditioning experiment; it does not isolate these two mechanisms.

Report overall/long TP/FP/FN/TN, precision, recall, F1; paired thread-bootstrap
F1-gain intervals (2,000 resamples); changed predictions, long recovered/introduced
misses, distant long recall (gap >=4) and acceptance gates. Report prompt-prefix
truncation, answer source coverage, chunk counts/overflow, inference/cache costs,
build/fit/scoring timing and peak RSS. Do not use rounded precision to claim a pass.
An inconclusive gain is not evidence of equivalence.

Use Optuna for the two registered trial records and MLflow for tracking. Persist
MLflow metadata in local SQLite and artifacts under a private research directory.
Log parameters, source/provenance hashes, protocol and aggregate metrics/results.
Do not log turn text, detailed per-thread predictions, embeddings or annotations
to MLflow. Keep those in private experiment artifacts. No autologging or hosted
tracking service. Record results and limitations in the LaTeX paper as completed.
