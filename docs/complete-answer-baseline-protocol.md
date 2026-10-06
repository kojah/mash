# Complete-answer baseline rerun

Fix the experimental turn answer-selection defect before testing denoising.
Both views now select **all nonempty assistant messages belonging to the turn**
by default. Preserve their order, IDs, source offsets, code and negation.
Explicit `last` remains an archival control; old registered builders request it
explicitly so their frozen artifacts remain reproducible.

Freeze two Optuna trials: `last` and `all`. No denoising, token weighting, model
change, reranking, clustering tuning or new annotations. Use the installed
quantized MiniLM, singleton inference, 160 development histories from cohort
SHA256 `d779a9a1850cb29558987e510ddd2452878aca20ebb57dd12ee5c657d5192805`.
Confirmation stays sealed. Keep whole-context equal-role pooling, heading-aware
chunk boundaries/budgets, uniform smoothing alpha .10/window four, BIRCH radius
.60, cutoff .75 and namespaced membership union fixed. Bounded ancestor context
is unchanged; the coverage fix concerns the current turn's own answers.

The last-only control must reproduce the preceding downweighting study's uniform
pooled/chunk matrices bit-for-bit, not merely similar scores. Verify original
last-only inputs against frozen parents too. Reuse only the same model's verified
singleton whole-text cache; input bytes determine cache identity.

Full-answer inputs must cover every selected assistant segment in the structural
view, plus every selected prompt, without restoring sibling/other-turn messages.
Check every non-whitespace source character against source locations. Whole-view
token windows retain their existing codec/budgets and preserve un-tokenized
answer edge characters so selected segments cannot silently disappear. The
archival last-only windows stay byte-identical. Report any representation
coverage differences between views. Preserve original data and archival artifacts.

Score the unchanged 1,051 original development pairs, keeping 121 ambiguous
judgments outside metrics. Report TP/FP/FN/TN, precision, recall, F1 overall/long,
2,000 paired thread-bootstrap intervals, changed predictions, and paired
long-distance related-pair misses. Report source-quote coverage before/after,
represented source counts, points, tokens, build/fit timing, peak RSS and hashes.
Use the same independent blind judgments; do not tune cutoffs to rescue a result.

This fixes the input contract regardless of point estimates. Distinguish a
source-complete experimental baseline from a demonstrated quality improvement.
The earlier 38%/19% source-coverage association is not proof of causality. Keep
production message search unchanged; document actual results in the paper.
