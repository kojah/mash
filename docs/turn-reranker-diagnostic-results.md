# Local Jina affinity diagnostic: completed

The [frozen diagnostic](turn-reranker-diagnostic.md) evaluated the installed
`jina-reranker-v1-turbo-en` on all 1,051 development reference pairs in both
directions, with two fixed input inventories and arithmetic-mean sigmoid
affinity. Inference did not read verdicts. No new model, download, generative
inference, training or confirmation evaluation occurred.

**Decision: do not use this setup for turn-clustering affinity.** Neither input
inventory separates the labels as well as the existing whole-context cosine.

| Affinity | Overall AUC | Long AUC | Overall optimistic R at P>=.85 | Long optimistic R at P>=.85 |
| --- | ---: | ---: | ---: | ---: |
| Existing context cosine | .859 | .843 | .828 | .688 |
| Jina / lean own inputs | .735 | .723 | .556 | .347 |
| Jina / context inputs | .653 | .608 | .323 | .089 |

The threshold-scan recall is optimistically fitted on the same development
labels, not a candidate threshold or held-out performance. Ambiguous pairs
were inferred but excluded from these metrics. For distant pairs, lean/context
AUC is .686/.556 and optimistic recall .000/.022. This does not establish that
Jina is poor at its intended query-document search relevance task, or that
other cross-encoders would fail.

Important limitations: concatenated cached windows repeat context, and literal
510-token prefixes truncate 1,185/3,396 lean turns and 1,478/3,396 context turns.
Long histories account for 1,095 and 1,359 truncated turns respectively.
Retained/full token totals are 1,076,284/1,429,151 lean and
1,271,344/1,765,280 context. Prefixes can omit answer content. Directional score
difference median/p95 is .132/.512 lean and .104/.392 context. Task mismatch,
truncation and repeated context are plausible contributors, not isolated causes.
No budgets, reducers or inputs will be tuned from these diagnostic results.

Inference took 809.32s (13m29s), with peak RSS 1,401,108 KiB. Measured inference
sums are 358.87s lean and 438.50s context; per-history p95 is 5.57s/6.07s for
annotated pairs only. Each inventory scored 2,102 directed pairs. Full all-pairs
construction would require 267,448 directed pairs per inventory, 266,386 from
long histories. Those full costs were not measured and the annotated-pair
timings do not prove a complete clustering performance gate. Analysis took
6.83s, peak RSS 1,039,032 KiB.

The first invocation stopped before scoring because the tokenizer has four
pair-special tokens, not the initially assumed three. The guard/protocol were
corrected before any pair inference, preserving side/total budgets. Failed logs
and the initial protocol remain archived. All 160 completed history records
pass model/runtime, input, identity, cohort and annotation provenance checks;
stored sigmoid values and symmetric means reproduce exactly.

Private artifacts:
`/home/james/scratch/mash-artifacts/turn-reranker-diagnostic-20261005`.

- Corrected protocol SHA256: `10c05e42fc256d04ca3ddea523ac4e1b55e9a40fa658690b4e0d9f33cc751610`.
- Analysis SHA256: `f87d91750b84c2ae60f0adef0ce2343f4a3dadc75dfc86b7c353a154c890094d`.

Retire this affinity setup. Preserve Jina's existing production search use;
this experiment does not evaluate or change it. Confirmation remains sealed.
