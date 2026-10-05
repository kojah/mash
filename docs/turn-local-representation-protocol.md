# Thread-local representation comparison

Freeze this protocol before building or scoring new representations. The user
approved three levers: broader evaluation (completed separately), reducing
boilerplate impact, and better context selection. No thread's input preparation,
vectors or clustering may depend on another thread. Production is unchanged.

Use the frozen 200-history cohort SHA256
`d779a9a1850cb29558987e510ddd2452878aca20ebb57dd12ee5c657d5192805`.
Run only its 160 development histories. Keep confirmation sealed. Use the existing
installed quantized MiniLM encoder and codec, CPU single-thread, batches of eight,
512-token limit. Pin all parent vectors and input units against the original
eight-cell ablation report, SHA256
`19ae4fd9f2c25b243416ac0a27fd735f3e286d9e5115be68fcb1830b712fd099`.

The pooled whole-context view remains unchanged, with original uniform smoothing
(alpha .10, ancestor/descendant window four). Preserve heading-aware chunk
structure, source coverage, BIRCH radius .60, cutoff .75 and namespaced membership
union. No strict fusion, corpus-frequency weighting, model download or LLM call.

## Six fixed Optuna trials

Cross three context modes with boilerplate off/on. Declare all six in advance;
Optuna records them in a persistent bounded study, not adaptive parameter search.

- **none:** original structural input, no repeated current prompt.
- **first80:** existing conditioning with the first 80 current lean-prompt tokens.
  This off-weighting cell must reproduce prior C-only predictions.
- **selected80:** split the current lean prompt into sentence/paragraph spans,
  then at most 80-token nonoverlapping windows. Embed all candidates using the
  same encoder. For each original answer chunk, select the candidate with highest
  cosine to its original unconditioned vector; exact ties select source order.
  Use a candidate only if cosine is at least .20. No ancestor, future turn,
  other history or label enters selection. This threshold is fixed, not tuned.

For conditioned answers, preserve the existing wrapper, prefix budget and
structural overflow splitting. Pool answer token states only; prompts affect
attention, not direct output pooling. User chunks remain unchanged.

**Boilerplate on:** downweight fixed, anchored conversational scaffolding in
assistant answer text only. Recognize polite openers (happy/glad to help, thanks
for asking, hello/hi/hey) and closers (hope this helps, let me know if you have
questions/need more help, feel free to ask further questions). Match complete
phrases at sentence/line boundaries, never arbitrary substrings. Protect fenced
and inline code. Do not prune recurring technical words or infer rules from
other threads. Original text and transformer attention remain unchanged.
In mean pooling, payload tokens have weight 1 and overlapping filler tokens .25,
then the existing normalization and quantization. Use exact integer-relative
weights 4/1. Special tokens retain full weight. No detected span means exact
original pooling and cache reuse. Cache identities include selected pooling
region, filler spans and encoder generation.

## Validation, evaluation and reporting

Before scoring, validate full non-whitespace body coverage and prompt ownership.
Check baseline and first80-off parity against all frozen judged predictions.
Verify no-filler vector parity, token-weight effects and code/technical-content
protection. Test isolation by preparing/encoding/clustering the same thread
alone and in reordered/mutated surrounding cohorts, using fresh caches; output
vectors and memberships must match. Cache reuse may save work but cannot alter
the scientific output. Record source, protocol, model and artifact hashes.

Score the unchanged original development labels (1051 pairs) as the main
comparison. Separately report the new 62-pair targeted blind review, including
reviewer sensitivity; do not combine its purposive prevalence with the original
benchmark. Record overall and long-history precision/recall/F1, changes in pair
predictions and paired thread-bootstrap intervals (2000 resamples), with the
existing acceptance and runtime gates. The repeated development study remains
exploratory, not a confirmatory significance test. Optuna's objective is overall
development F1; the largest value does not override precision/long-history gates.

Record construction throughput, newly inferred vectors, filler prevalence,
selection acceptance/switches, token truncation/overflow, clustering timing and
peak RSS. Report negative outcomes as outcomes; do not add trials, retune
thresholds or open confirmation to rescue a result. Log procedures/results in
the paper. Completing this comparison requires actual scored artifacts for
both levers, not just implementation or a study ledger.
