# Versioned, source-only lexical frame candidate

Frozen before corpus outcomes, 2026-10-02. Rotation-only full-training lexical
average return .624317/.603018 is promising but learned future vocabulary/IDF
cannot be treated as an online global frame. This experiment replaces that frame,
not the actual search embeddings or production configuration.

Fixed16384 buckets, nonnegative feature hashing. Token counts use original
Python3.11 Unicode lowercase and regex `(?u)\b\w\w+\b`. Subtract only exact
case-sensitive source `{disfmarker}`/`{vocalsound}` occurrences from those counts,
matching the earlier marker ledger (do not concatenate adjacent source words).
Every term uses SHA256(`mash.topic.lexical.v1\0` + UTF8(term)), first8bytes
little-endian mod16384. Compute1+ln(term_count) BEFORE summing collided terms,
then L2 float64. No learned DF/IDF, fitted vocabulary, projection, inference or LLM.
New terms never change existing coordinates/weights. Positive collisions can
inflate similarity and must be measured, not assumed harmless.

Two prespecified policies: marker-only raw TF; same plus the already fixed
label_noise.EXCLUDED English/acknowledgement list as a topic-only feature control.
This is an experiment on geometry, not a claim that label filters improve it.
If filtered counts empty but marker-only counts nonempty, explicitly use raw
geometry with `filtered_empty_fallback` state; never invent topic labels for it.
If marker-only counts empty, emit `empty` with no direction, retaining source
identity/unassigned status. No silent dropping or changed original-pair support.

Version descriptor/fingerprint includes hashing/buckets, token/Unicode rules,
normalization, marker behavior, policy/stoplist and fallback contract. Across
nodes/providers, same descriptor means same coordinates independent of arrivals.
Groups remain separate namespaces. A descriptor change requires explicit new
generation/rebuild/republication; reject mixed frames. Do not apply mutable
online IDF weights to stored subtree sums. Edits/deletes retract original unit
contributions; do not treat additive insertion as proof of those operations.
Python token/Unicode semantics still require chosen Rust parity fixtures.

Next corpus study: unchanged source blocks/count lineage/known-source scopes,
rotation-only both orders, marker-only/filtered frames plus collision-free
fixed-term-identity TF diagnostics and immutable offline TF-IDF upper reference.
Collision-free identity is an evaluation oracle, not a fitted online vocabulary.
Verify raw text counts exactly reproduce the original marker-only ledger before
using saved counts for efficiency. Report collisions/empty/fallback source IDs,
pair-cosine errors, all/far/crossspan and labels/representatives. Capture complete
source trees/frame pins; full repeat/independent original-source and hash/geometry/
LCA audit. No cutoff/grid/heldout/LLM/new inference. Same necessary return-loss
<=.02 both orders; broad loss and failures remain visible, not waived.

Measure hashing/tokenization, insertion, label/state persistence, edit/delete,
restart and globally consistent namespace costs separately. Small per-meeting
prototype timings do not pass global/state/ingestion gates. No production method
selected. Independent coding label/multitag/ancestor/guided-search evidence,
bounded global lifecycle and complete-ingestion<=10percent loss, chosen
Python/Rust differential CI remain promotion requirements.
