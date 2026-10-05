# Structure-aware turn chunks: prospective comparison

Frozen before new representation scores, following the user's segmentation
proposal. Use the same cohort, blind labels, installed encoder and source
selection as the context-window study: all prompts and the last nonempty
assistant segment. Preserve the original 80+80-token ancestor prefix and
whole-input pooling. No LLM, new model, answer selection, threshold tuning or
temporal smoothing. Confirmation stays sealed.

## Deterministic segmentation

- Never pack across source messages, speakers or Markdown heading sections.
- Detect ATX headings (one to six `#` characters followed by whitespace) and
  fenced code blocks (three or more backticks or tildes). Preserve code fences
  and inner indentation; an unclosed fence extends to the message end.
- Blank lines separate prose paragraphs. A heading belongs with its first
  following block when possible. Pack adjacent prose paragraphs within the
  same section toward 128 tokens; a larger paragraph that fits the encoder
  budget stays whole. Code blocks stay separate from prose, except a preceding
  heading may accompany the code block if the combined input fits.
- If a prose block exceeds the input budget, pack at English sentence-ending
  punctuation followed by whitespace and an uppercase/digit/quote/bracket
  starter. This is a deterministic heuristic, not a grammatical guarantee.
  Oversized code blocks split at complete line boundaries. If a single
  sentence or code line still exceeds the budget, use the existing 32-token
  overlapping fallback and record each forced token split.
- Strip only outer whitespace from embedding inputs, preserving exact source
  offsets and every selected non-whitespace character. Do not discard short
  prompts or standalone headings. Keep the encoder limit at 512 tokens;
  use the existing boundary headroom when prefixes are present.

Compare two explicit treatments:

1. **Plain structure:** the structural body plus unchanged ancestor context
   for prompt inputs.
2. **Heading context:** additionally copy at most 32 tokens of the nearest
   preceding heading into later chunks in its section. Do not duplicate the
   heading when the body already contains it. Copy original source text rather
   than generating a label. Reserve its tokens before body splitting. This
   treatment can therefore change both heading context and chunk budget.

Both prompts and answers can have several chunk vectors. Keep points separate,
require two distinct owning turns for retained groups, then union memberships
per turn. Source/window multiplicity may influence clustering; do not silently
introduce weighting or remove low-content units after inspecting scores.

## Fixed comparison, gates and stopping

Use only BIRCH at the existing pairs (radius, cosine cutoff) (.75,.65) and
(.60,.75), with the same centroid cutoff and relative-best margin .05.
Two treatments give four new cells. The two original context-window BIRCH
cells are unchanged controls, pinned by their development report SHA256:
`126544f75c82034e0c400863311c918d00096a95878357721993d3322f404eba`.
Check exact old input reproduction before building new inputs. No new algorithm
or setting sweep; freeze every rule before scores.

Require overall P>=.85, R>=.70, F1>=.75 and F1 gain>=.10 over chronological-four;
also require P/R/F1 absolute floors on 8+ turn development histories. Select
highest eligible F1, then preparation+fit seconds and name. At most one final
choice may proceed to unopened confirmation once; no winner means this screen
fails. Do not retry parameters on confirmation.

Report all original strata, paired thread-bootstrap intervals, nearby (<=4)
unrelated FPR and distant (>4) related recall, exact source coverage, chunk
types/counts, forced splits, cache/new inference costs and externally measured
full-process peak RSS. Keep construction p95<=300s/history, preparation+fit
p95<=10s and peak RSS<=4GiB. Distinguish cache-assisted timings from cold costs.
Synthetic fixtures must cover Unicode, message/heading boundaries, code fences,
unclosed fences, oversized atoms and preserved encoder budgets. Structural
boundaries are not evidence of topic coherence; evaluate that through frozen
labels rather than claiming it from the parser alone.
