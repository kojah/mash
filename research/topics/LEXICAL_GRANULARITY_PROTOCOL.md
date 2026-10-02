# Lexical evidence granularity with frozen source-pair support

Frozen before corpus outcomes, 2026-10-02. Dense granularity alone failed to
improve topic structure. Change representation to source lexical counts under
the SAME average-linkage and granularity controls, not another seed/cut sweep.

All162 original training meetings,84,434 inherited ready sources, original
physical-block exclusions and known-source class weighting remain identical.
Reuse exact-marker-cleaned count ledger, original vocabulary and already fitted
full-initial-snapshot smoothed IDF from the audited lexical reference. Each
feature uses sublinear per-term TF, then frozen IDF and L2. No static stoplist
or tuned weighting change. Vocabulary/IDF use the explored training snapshot:
this is a batch reference, not causal online fitting or a deployed shared frame.

Three lexical arms: raw original message counts; sum ORIGINAL counts within
the exact existing semantic five-turn/512-model-token/gap-stop context boundaries;
original16-turn/512-token physical block counts. Apply TF only AFTER count
pooling, rather than average transformed vectors. Counts are not duplicated in
labels because this stage does not generate new labels. Context borrowing is
not validated reference resolution. Preserve zero lexical rows in geometry and
source support; do not silently discard them or force false unit normalization.
Unsupported originals stay in ledger, outside the inherited ready geometry.

Cosine average linkage in original source order, no gold input or topic counts.
Raw/context subdivide original physical blocks. All arms EXCLUDE same original
physical-block positives, keeping all/far/recurring denominators
7978214/7923483/84323. Replay original lexical-block topology/merges exactly and
metric totals within the existing1e-12 tolerance. Compare against the frozen
dense-granularity artifact, with all lineage/accounting unchanged. No new
inference, LLM, model download, projection, cutoff/window grid or held-out read.

Record complete trees/merges/metrics, count/coordinate lineage and zero support,
stage costs plus external memory, begin/end code pins, complete non-timing
repeat and independent source/count/normalization/LCA/linkage audit. Fixing a
source representation does not establish useful label/topic memberships or
semantic ancestry; these remain next-stage evaluations, without reopening
failed coherence cuts. Shared generations, edits/deletes/restart, global cost,
full ingestion<=10% overhead, coding multitag/ancestor/guided-search checks and
chosen executable Python/Rust reference/differential CI precede promotion.
