# Held-out author-validation guidance protocol

Frozen before validation transcripts/queries/labels are downloaded, 2026-10-02.
P0 `mash-c0d.19.2.20.8.3.3.3.9.4.4`. Source metadata only has been inspected
through the author's immutable Git tree; no validation outcomes observed.

## Split and provenance

Author QMSum repository `https://github.com/Yale-LILY/QMSum`, commit
`83d7768c1f2b4dfeb091385d3dc7e239b8e5bb7e`, tree
`159791fb5fae5ef625bd0ce4d300fd3d53aca9c8`.
Use only `data/ALL/jsonl/val.jsonl`, Git blob
`f6d2b619c21fea5d1b6e35d697af483dd2802d24`, 2,714,748 bytes, plus pinned
README/LICENSE. Separate fresh outside-Git capture, strict file inventory and
protocol hash. Keep the existing training capture unchanged and test split
absent. Whole-meeting transcript hashes must be disjoint from training,
over ordered original turn texts (speaker aliases excluded), independently of
query/annotation content; report repeated turn-text prevalence,
do not treat common greetings as duplicate meetings. Reject drift/duplicates
before evaluation, rather than silently dropping records. Preserve original
speaker/text/ordinal/string-index inclusive spans and raw line identities.
Keep public corpus and underlying source rights distinct from repository license;
do not release transcript artifacts or treat code licensing as dataset clearance.

## Frozen features and inventory

Lexical evaluation only; no new embedding inference, LLM, API or model download.
Use the already installed pinned tokenizer (SHA256
`da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`),
singleton unpadded/untruncated with special tokens. Original readiness policy:
raw and equal-length exact-marker-masked inputs both nonempty and <=512 tokens.
Every excluded original identity remains in source/reference denominators with
explicit reason. Unsupported inputs are not silently truncated or searchable.
Lexical zero rows remain eligible sources with no positive match.

Freeze training vocabulary/IDF from the already pinned lexical-context capture;
do not re-fit vocabulary, DF/IDF, projection, stoplist or hyperparameters on
validation text, queries, answers or labels. Report source/query OOV counts and
all zero queries/results explicitly. Same exact-marker token counting, static
stoplist, pool original ready counts before sublinear TF/IDF/L2.

Use the same source-only physical blocks: maximum16 contiguous ready turns and
512 total installed-model tokens; gap/window flush and all ownership preserved.
Lexical-average hierarchy and largest-descendant fixed-eight cut per meeting,
with the existing deterministic ties, unchanged. This is a transductive
source-index build before any query/label evaluation; it is not causal online
discovery or a shared cross-meeting taxonomy. Thread inventory is one pooled
parent per original meeting. No topic-name or answer features or generated
summary; topic labels are not mandatory search gates.

## Arms, outcomes and decision rule

Evaluate every native specific query and complete original reference spans;
general summarization queries lack native spans and are counted separately,
not assigned fabricated relevance. Same top20 positive-source budget,
complete source/parent pools, no fallback/gate/cap. Exact-float direct,
thread-parent and topic-parent fixed k60 bonus are the primary arms. The frozen
integer-bin/exact-rational counterparts are secondary numerical diagnostics;
do not choose a bin width or winning arm after inspecting validation outcomes.
Complete the training numerical repeat/audit before held-out scoring.

Primary hypothesis: exact topic-parent guidance improves known-meeting macro
recall@20 over exact direct retrieval. Point estimate averages all queries,
including zero/OOV/unsupported-reference cases. Meeting-cluster bootstrap:
10,000 resamples of whole meetings with replacement, NumPy PCG64 seed20261002,
each resample retains all queries of each selected meeting and computes the
pooled per-query mean difference. Require the percentile95% interval's lower
endpoint >0; otherwise the generalization gate is not demonstrated. Report
MRR/NDCG/precision/zero cases and per-query gains/losses. MRR safety gate:
95% lower endpoint for topic-minus-direct must exceed -.02 (provisional
engineering noninferiority margin, not a user requirement or tuned optimum).
Keep plain thread context as a baseline; known-meeting sole-parent order must
exactly equal its corresponding direct arm.

Global union of validation meetings is diagnostic only: native judgments cover
the query's original meeting; other meetings are unjudged, not known negatives.
Do not use global scores alone to promote a shared topic taxonomy or guided
agent utility. Preserve complete per-query results/metrics and original
denominators, paired numerical order/membership drift and effect sizes.
No grid, additional stopwords, parameter change, arm selection or case removal
from held-out outcomes. If this split informs a revision, mark that revision
exploratory and freeze a new final protocol before using the untouched test set.

## Reproducibility and independent gates

Require fresh full repeat, begin/end input/code/model-tokenizer pins, independent
source-count/unit/ownership/query/span/order/formula/metric reconstruction,
meeting-bootstrap reproducibility, and external target RSS/PSS with rusage,
sample gaps/scope/observer overhead plus complete wall/CPU/per-scope costs.
Fixtures/research setup are not deployed E2E or full-ingestion measurements.
Passing the relevance gate is necessary, not sufficient for production choice:
coding-session semantic labels/multitags/ancestor inheritance, guided-agent
utility, bounded shared online edit/delete/restart lifecycle, <=10% complete
ingestion loss, justified numerical platform bounds and selected executable
Python/Rust parity remain open. No method is selected by this protocol alone.
