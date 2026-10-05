# Retained turn windows: prospective cache-only comparison

Frozen after the input/context and temporal-smoothing studies, before new scores.
This tests whether averaging prompt/answer windows loses minority subjects.
Multiple windows do not themselves establish multiple topics. The development
input audit found multiple windows within a role in 1,081/3,057 long-history
turns and 89/339 short-history turns.

Use the same 160 development histories, unchanged blind labels and installed
encoder's existing cached vectors. Do not read confirmation judgments. No new
inference, tokenizer changes, truncation, cleaning, weights or temporal smoothing.
Pin the source development report SHA256:
`d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e`.

## Representations and fixed grid

1. **Context windows:** use each original whole-context input window as a point,
   instead of role means and a turn mean. These inputs retain all prompts and
   the last nonempty assistant segment, exactly as in the completed study.
2. **Context prompts + full answers:** keep the same context-bearing prompt
   windows, but use all original raw separate-view assistant windows, including
   intermediate segments. Read their exact vectors from the existing cache.
   This distinguishes restoration of answer coverage from unpooling alone.

Each point retains its owning turn and exact source spans. Validate context
input provenance and original raw manifests against the frozen history; open
cache databases read-only. A missing or invalid vector fails the run rather
than invoking inference. Preserve unit normalization and the existing codec.

Cross the two representations with BIRCH and average-link at cuts .65 and .75.
BIRCH radius is .75 for cut .65 and .60 for cut .75, as previously used. Only
the existing centroid-membership rule applies: absolute cutoff and within .05
of the best centroid. Require two distinct owning turns for retained groups,
then union point memberships into turn memberships. Repeated windows from one
turn cannot supply two supports. No new role balancing or point-count correction
is introduced; more windows can influence fit and this is a limitation to report.
This is eight window cells, plus one unchanged pooled-context BIRCH .60/.75
centroid anchor. Its predictions must reproduce the preceding study.

## Gates and stopping

Keep precision >=.85, recall >=.70, F1 >=.75 and F1 gain >=.10 over the frozen
chronological-four baseline overall; require the same absolute precision,
recall and F1 floors on 8+ turn development histories. Select highest eligible
F1, with ties broken by cache-read/preparation plus clustering seconds and name.
At most one final eligible choice advances to unopened confirmation, once.
No eligible choice means this experiment fails; no repeated threshold tuning.

Report every cell, original provider/node/length/sampling-stratum/batch breakdown,
paired thread-bootstrap intervals, point counts and assignment coverage. Report
fixed nearby (<=4 turn positions) unrelated-pair FPR and distant (>4) related-pair
recall, including changes from the pooled anchor. Keep prior temporal results
as separately reported evidence, not additional tuned cells in this screen.

Measure complete process peak RSS and per-history cache preparation plus fit
time. Require p95 <=10 seconds and peak RSS <=4 GiB; base encoder construction
budgets already measured in the preceding study. Cache-only timings must not
be presented as fresh embedding construction costs. Preserve raw silver and
production message embeddings. Any accepted research choice still needs Rust
parity fixtures and integration before production promotion.
