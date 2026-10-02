# Causal adjacent-turn pooling on incomplete stored vectors

This controlled exploratory experiment uses the same frozen 149-message coding
cohort as the deferred prototype, with 74 stored vectors and 75 missing vectors.
It does not change eligibility, source text, embeddings, labels, cosine threshold
or production behavior. No inference, downloads, paid calls or LLM are used.

## Method

`topic_units.py` restores original native user-turn order from the frozen source
provenance, separately per thread/provider. The next embedded turn joins the
current unit only if cosine against the prior unit vector sum reaches .55,
the original routing threshold. Missing geometry breaks the unit and remains
as a singleton without geometry. Topic changes also end the unit. Units cap at
eight turns and 8,192 UTF-8 bytes including separators; an oversized original
turn remains an unchanged singleton, rather than being truncated or dropped.
No assistant-context borrowing, rewrites or new semantic episode inference is
performed. This is a cheap adjacent-similarity control, not TextTiling or a
validated topic-boundary detector.

Routing uses normalized means of existing vectors, with one vote per unit.
Deferred capture reconstructs sums/counts from ORIGINAL source vectors under
their unit's routing anchor; memberships and lexical support then use original
messages, original vectors and the original DF denominator. Copying the unit
vector into every member would create artificial duplicates; we do not do so.
All 149 source identities remain in the refreshed view. This experiment is an
offline adapter, not a persistable replacement for the production index or a
lifecycle/parity claim. Unit formation depends only on prior same-thread turns;
the driver uses simulated round-robin unit arrival. Reversed arrival reverses
the preformed units, not each thread's native unit-formation order.

## Results

149 source turns became 144 units. Only three units were multi-turn, with five
accepted joins and a largest unit of four turns. The minimum accepted join
cosine was .706452. The refreshed view still had 57 leaves, 57 weak nodes,
two forward and one reverse multi-tag messages. Forward/reverse co-membership
pair sets matched the original prototype exactly. Their order Jaccard remained
.710526. Thus this control does not fix fragmentation on this sparse-vector
cohort; it does not establish that segmentation generally cannot help.

Two runs matched every non-runtime result. A one-off independent scalar check
verified unit partition conservation, per-thread order, member/byte bounds and
accepted join cosines using `math.fsum`; it did not reconstruct every rejected
join, routing or label calculation. The audit result is retained as
`topic-unit-pooling-audit-20261002.json`. Five fixtures cover related turns,
topic changes, missing geometry, thread separation, bounds and empty inputs.
The research suite passed 629 tests in 5.441 seconds.

Pooling, routing and two complete deferred refreshes took 697.52/684.56 ms,
excluding initial JSON loading. External target RSS maxima were
138,136/136,196 KiB and PSS 130,379/128,448 KiB (103/100 samples).
Approximate status HWM matched RSS counters here. Target scope includes imports,
loading and both arrival orders; observer/children memory is excluded. Samples
are lower bounds and monitoring can perturb timings. These are not isolated
pooling overhead, full ingestion, production concurrency or a <=10% overhead
gate. Stable outputs pin the baseline, method and direct dependency hashes.

Private first/repeat outputs and external memory sidecars have prefix
`topic-unit-pooling-*-20261002` under mash-artifacts. The original baseline
remains unchanged. Next perform a separately pinned complete-local-embedding
control on the SAME sources, using already installed assets and explicit model
window coverage, measuring all additional inference. Do not silently fill or
overwrite the missing-vector baseline. Independent coding-session judgments,
complete-ingestion performance and selected-method Rust parity remain open.
