# Three-factor turn-clustering ablation: frozen protocol

The user fixed the current best result as the baseline and requested ablations
of answer-chunk context, smarter fusion and generic-chunk weighting. Freeze
this protocol before any treatment scores. No confirmation labels will be
opened; completing these ablations does not require promoting an algorithm.

## Baseline and design

Pin the smoothed pooled/heading-chunk union report SHA256
`9835b7cd6549b07ebb4e3c55ee4190ac7294854d5a9d6dacd968b08ddfb083ce`.
The pooled parent uses original uniform alpha=.10/window=4 smoothing, linear
distance weights and ancestor/descendant restriction. Both views use BIRCH
radius .60, centroid membership cutoff .75 and relative-best margin .05.
The chunk view keeps the existing heading-aware structural boundaries and
source selection. Model/tokenizer, source identities, turn delineation and
labels stay fixed. Recompute actual baseline memberships and require exact
parity of every reference prediction and confusion count before treatment
analysis. Group IDs need not match; measured memberships/costs now replace
the previous pair-only diagnostic's unavailable composite coverage/costs.

Run all eight on/off combinations C/F/G on the same 160 development histories:
000 baseline, 100 context only, 010 fusion only, 001 generic weighting only,
110, 101, 011 and 111. This includes baseline-relative one-factor comparisons
and full-model leave-one-factor-out comparisons. No parameter grids, model
replacement, selection of alternative reducers or post-result tuning.

## C: current-prompt conditioning of answer chunks

User-prompt chunks remain bit-identical. Prefix each answer chunk with the
literal first 80 tokens of the current turn's concatenated lean prompts,
labelled `Current request`, followed by `Answer excerpt`. Do not borrow another
turn's prompt or restore excluded intermediate answers. This bounded prefix
is repeated on every answer chunk. Pool only answer-excerpt token states,
including its existing heading context; the prompt influences attention but
is excluded from output pooling to limit repeated-prompt dominance.

Keep existing chunk boundaries when they fit the 512-token model budget.
When the extra prefix causes overflow, split that existing chunk using the
same sentence/code-line boundary helper and token fallback; never merge old
chunks. Reserve ten tokens beyond the prefix's token count, then reject any
remaining overflow. Preserve selected non-whitespace body coverage and literal
source offsets. Validate prompt and answer provenance separately, and require
the combined text/pooling span to equal their deterministic construction.
Report chunk multiplicity, splits, retained prompt coverage and construction
cost. Use the installed encoder's unchanged f32 mean/L2/i8 codec and its
selection-aware cache namespace; no generative inference or downloads.

## F: evidence-aware fusion

Do not refit partitions or centroids. For memberships supported by only one
view, require centroid confidence >=.80, keeping the existing .05 relative-best
rule. Relationships supported by both views can retain the baseline .75.
Implement this as namespaced strong groups from each view plus supported
intersections of their ordinary groups. A pair matches when it shares one of
these groups. Never merge groups transitively or use judged pair identities to
construct groups. With F off, use the original namespaced union.

## G: downweight generic answer chunks

Keep BIRCH insertion and centroids fixed. Subtract .05 from generic answer
chunks' centroid membership confidence before the .75/.80 gates; retain their
remaining signal rather than delete their source text. Prompt chunks and the
pooled view are unaffected. A chunk is generic when it has fewer than three
distinct content terms, or no content term appearing in at most 20% of the
development turns. Use lowercase ASCII technical-word tokens
`[a-z][a-z0-9_+-]+`, scikit-learn's English stoplist and the fixed additions
`thanks, thank, hello, hi, hey, okay, ok, sure, please, yes, yeah, done`.

Build document frequencies from each development turn's lean prompts and last
nonempty selected answer, counting a term once per turn. No annotation labels,
confirmation histories, repeated context or window overlap enter those counts.
For chunk classification use its own answer-source spans only, excluding added
prompt/context/formatting. Freeze and hash the resulting background vocabulary
and record the stoplist hash. Unknown terms have zero frequency. This
experimental corpus-wide statistic would require an explicit frozen generation
or invalidation policy in production; do not claim it is automatically online.

## Reporting and stopping

Report all eight overall and long-history P/R/F1 and confusion counts, paired
thread-bootstrap baseline gains, nearby-related recall/unrelated FPR, distant
recall, provider/node/length/sampling slices, actual group sizes and coverage,
and preparation/fit p95 plus externally measured peak RSS. Measure the new
embedding build separately. Report one-factor and leave-one-out effects
without treating repeated cells as independent observations or claiming
unadjusted reused-development intervals prove generalization.

Preserve the existing overall P>=.85/R>=.70/F1>=.75 and the same long-history
floors; report legacy chronological-four gain>=.10 separately from gains over
the newly fixed baseline. Performance budgets remain construction p95<=300s,
preparation+fit p95<=10s and process peak RSS<=4GiB. A winning point estimate
is not production acceptance. No confirmation scoring, deployment or threshold
retuning is part of this requested ablation task. Log procedures, outcomes and
limitations in the LaTeX paper, including null/negative results, and finish
with a baseline-relative recommendation backed by the complete eight-cell run.
