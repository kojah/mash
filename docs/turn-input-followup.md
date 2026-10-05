# Bounded turn input/context follow-up

Specified after the negative 200-thread development comparison and its
development-only error audit, before building/scoring the new variants.
This is a follow-up experiment, not a revision of the original registration.

Use the same 160 development histories, frozen agent-authored labels and local
MiniLM assets. Preserve the original 40-history confirmation set and all silver
text. No generative model, API inference, downloads or annotation changes.

The diagnostics support two hypotheses: procedural/transport text can obscure
the concrete subject, and short replies sometimes need their actual ancestor's
context. Adding context can also contaminate unrelated topic changes. Later-turn
assignment drift is a separate effect; input improvements cannot be assumed
to cure it. No approval-phrase recognizer or vocabulary of individual bad cases.

## Four input variants

1. **Raw separate views:** the original normalized prompt mean and answer mean,
   equally combined. Use the frozen matrices as the unchanged control.
2. **Lean separate views:** preserve all prompts; for a supported, validated
   question/reply transport envelope retain its question and answer scalar text,
   excluding wrapper keys and opaque question IDs. Unknown/malformed envelopes
   remain untouched. Keep the last nonempty assistant segment rather than all
   intermediate segments. This is an explicit selection heuristic, not a claim
   that the segment is a complete final answer. Record excluded source coverage.
3. **Lean context, whole-input pooling:** use the same lean sources and add at
   most one structurally resolved prior turn to the prompt view. Limit prior
   prompt text to 80 tokens and its last assistant segment to 80 tokens. Current
   prompts use the remaining encoder budget, with existing 32-token overlap.
   Role markers identify context and current text. Supply context to all eligible
   prompts, without phrase-specific gating; unrelated pairs are the contamination
   controls. Answer pooling stays identical to variant 2.
4. **Lean context, current-prompt pooling:** exactly the same input as variant 3,
   but pool only token states overlapping the current prompt span after the
   transformer attends to all supplied text. Exclude padding and special-token
   zero offsets. Answer pooling stays identical to variant 2. This pooling is
   experimental and not established by the encoder's original training.

Source offsets refer to the original Unicode text, including borrowed context
and selected question/reply scalar ranges. Preserve exact source identities and
record the context dependency. Never borrow future text, cross an unresolved
branch, or silently manufacture an antecedent. Follow resolved parent links;
when the history is explicitly linear with no parent, use the preceding turn
only across an ordinary user-reply boundary. Explicit roots/unknown branches
abstain. With no usable context, use the lean prompt input. Empty selections
fall back to the original source rather than generating a zero vector.

Use the original tokenizer, 512-token limit, f32 pooling and signed-i8 codec.
Cache keys must include pooling/span selection as well as encoder and exact
input text. Preserve the original script and matrices; store follow-up outputs
in a separate private directory. Fix these rules before new scores.

## Fixed comparison and decision

Only BIRCH, with the two existing paired settings (radius, membership cosine)
(0.75, 0.65) and (0.60, 0.75). Evaluate retained primary memberships and the
existing centroid-membership rule as explicit modes. This gives sixteen
configurations across four variants, not a new hyperparameter sweep. Minimum
distinct-turn support remains two. State when a primary-only choice yields
one group per turn; do not present it as learned multi-topic assignment.

The simple baseline remains the original development winner, chronological
four-turn blocks. Require precision >=0.85, recall >=0.70, F1 >=0.75 and
absolute F1 gain >=0.10 overall. Because the previous near-miss concealed poor
long-history results, additionally require the same absolute precision/recall/F1
floors on the 8+ turn development stratum. Report all original breakdowns,
paired thread-bootstrap uncertainty and source/token coverage. Choose the
highest eligible development F1, tie-break by clustering seconds then name.
Do not retune preprocessing, thresholds or contexts after seeing this grid.

Freeze at most one choice before confirmation. Evaluate only that choice and
the fixed baseline once, requiring the same absolute quality floors and gain.
No winner means this follow-up fails its gates; keep the feature experimental
and report the outcome. No successive candidate retries on confirmation.
Performance budgets remain five-minute p95 artifact construction, ten-second
p95 clustering and 4 GiB measured process RSS. Report cold/cache-assisted costs
accurately. A research choice still needs Rust parity and integration before
production promotion.
