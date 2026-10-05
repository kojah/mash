# Local reranker affinity: bounded diagnostic

Freeze this diagnostic before scoring any corpus pairs. The previous heuristic
screen is closed; this does not reopen its cutoff/parent choices. Confirmation
is sealed and no candidate will be promoted from these results alone.

## Inputs and model

Use the same 160 development histories and all 1,051 development reference
pairs, including ambiguous pairs at inference time. The inference runner reads
pair identities, never verdicts. Compare exactly two input inventories from the
frozen input follow-up: `lean_separate` and `context_whole`. Concatenate each
turn's existing input text in its recorded order, separated by two newlines.
This preserves the input inventories rather than generating summaries or
rewriting them. Repeated context across windows is an explicit limitation.
Validate input JSON against the immutable parent report, and validate literal
source/role/ancestry coverage with the existing source guard.

Use only the already installed `jinaai/jina-reranker-v1-turbo-en` ONNX snapshot
`b8c14f4e723d9e0aab4732a7b7b93741eeeb77c2`. Model SHA256:
`c1296c66c119de645fa9cdee536d8637740efe85224cfa270281e50f213aa565`;
tokenizer SHA256:
`0046da43cc8c424b317f56b092b0512aaaa65c4f925d2f16af9d9eeb4d0ef902`.
CPU ONNX Runtime, one intra/inter-op thread, batch one. No downloads,
generative model, remote service, training or new dependency.

For each side, retain the literal prefix ending at the 510th tokenizer token
offset, or the full text if shorter. Pair encoding includes its four special
tokens and must fit 1,024 tokens. Reject unexpected tokenization/overflow rather
than silently apply another truncation. Report retained/full token counts and
truncated-turn counts. This is a deliberate experimental budget, below the
model's documented 8,192-token capacity; it may omit answers in large turns.

Score both directions for every pair. Record raw finite logits, sigmoid scores,
and their arithmetic mean as the single symmetric affinity. Directional
asymmetry is diagnostic; do not choose max/min/another reducer after observing
results. Save every completed history to private immutable files so an
interruption can resume without repeating inference. Validate model, protocol,
source and pair-identity hashes on resume.

## Analysis and stop rule

Use development labels only after inference. Report ROC AUC, related/unrelated
medians, and the same explicitly optimistic scalar recall-at-P>=.85 diagnostic
as the prior representation audit, keeping score ties together. Report all,
short, long, nearby and distant slices for both inventories; compare the fixed
context-cosine descriptive baseline. Report median/p95 absolute directional
score differences and truncation by long/short histories.

Measure inference time per history and external process peak RSS. Annotated-pair
timing is not full clustering throughput: report the full directed-pair counts
required for all-pairs clustering separately and label any extrapolation as
unmeasured. Topic equivalence is not the model's training target. Its
[model card](https://huggingface.co/jinaai/jina-reranker-v1-turbo-en) describes
query-document search relevance and sigmoid logits, so it must earn usefulness
on our reference labels rather than inherit its search benchmark claims.

No candidate threshold or clustering configuration is selected in this audit.
If long scalar separation is meaningfully better than the fixed context
baseline, register one bounded affinity-based clustering study including real
construction costs. Otherwise retire this model as an affinity candidate and
evaluate a stronger encoder. Do not sweep budgets, directional reducers or
input inventories on these scores. Existing acceptance criteria remain intact.

Pre-scoring correction: the first invocation stopped during tokenizer setup,
before any history or pair was scored. The installed tokenizer uses four pair
special tokens (`<s> A </s> </s> B </s>`), rather than the initially assumed
three. Correct the guard and this description before resuming. Side budgets
remain 510 each and total budget remains 1,024; no experimental scores informed
this correction. Preserve the failed log and initial protocol copy.
