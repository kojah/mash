# Fixed lexical-frame quality control

Frozen before tree outcomes, 2026-10-02. Follow STABLE_LEXICAL_PROTOCOL.md
without changing its encoding preflight. Rotation-only, raw/filtered hash frames
and exact term-identity TF controls, forward/reverse order. No parameter grid,
new inference, held-out reads, source dropping or semantic promotion.
Term identity is a collision-free evaluation oracle, not trained online weights.
Compare all/far/crossspan source LCA purity to immutable offline TF-IDF average
reference, on identical source-pair support. Report necessary recurring loss
<=.02 in both orders plus all broad-topic losses without waiving them.
Report every distinct physical-block cosine error, aggregated signed/absolute
sum and maximum; these are not semantic error labels.

Display cut: min(8, physical-block count) groups by repeatedly splitting largest
leaf-population node (ties lexicographic original block-membership lists).
This never uses gold topic count or labels. Existing c-TF-IDF formula on original
unhashed counts after static noise exclusion, within the same meeting and cut.
Top8 positive terms with lexical ties; flagged raw fallback blocks provide no
label contributions. Representative: original block maximizing dot product
with the group's unnormalized mean direction, ties earliest source block.
Capture every display group/label/representative, not only selected examples.
No calibrated semantic cut, topic-name overlap or label-quality score is claimed.

Preserve source/frame/code pins and complete repeat outside timings. Independent
reconstruction must check tree source ownership, feature geometry, source-pair
LCA accounting and labels. Small per-meeting controls do not establish global
bounded state, edit/delete/restart, full ingestion, coding-agent multitag/ancestor
quality or selected Python/Rust parity. Keep parent active until those gates.
