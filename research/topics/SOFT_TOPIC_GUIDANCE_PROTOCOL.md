# Nonexclusive parent-rank guidance

Frozen before outcomes, 2026-10-02, P0
`mash-c0d.19.2.20.8.3.3.3.9.3`. Prior grounded-query label gates discard
relevant sources. Test a ranking bonus without changing direct eligibility.

Reuse the pinned navigation capture and original counts/ready IDs/word IDF/
stoplist/lexical-average groups. Same 1,095 specific training queries and
1,206 spans, meeting-conditioned and global scopes, top 20 positive-score
original sources, unchanged original reference denominators and metrics.
No display-label gate, top-group restriction, fallback, gold feature, answer,
topic-name feature, new inference/model/download/LLM, held-out source or grid.

Direct arm ranks ALL eligible positive-cosine sources, ties ascending global
source ID. Guidance ranks ALL positive full-group word cosines within scope,
ties ascending global group ID. Give each direct candidate score:

`1/(60+source_rank) + 1/(60+parent_group_rank)`

The parent term is zero if its full-group cosine is not positive. Both ranks
start at one. This adapts Cormack/Clarke/Buettcher SIGIR2009 reciprocal rank
fusion (https://cormack.uwaterloo.ca/cormacksigir09-rrf.pdf), whose constant60
was fixed before their validation. It is NOT their same-document permutation
experiment: a shared parent rank is broadcast to multiple sources. Group size
and repeated broad vocabulary may therefore bias the bonus. Freeze equal
weights/60 before outcomes; no tuning from results. Final ties use source ID.
All direct candidates remain eligible; no zero-source-score candidate is added.
Return the same min(20,positive sources) count, though rankings/hits can change.

Direct scores/IDs/metrics must exactly replay the prior navigation control.
Record returned source IDs, base score/rank, parent group/score/rank, fused
score, and full direct candidate counts; query/reference/accounting is frozen.
All query identities, zero cases and every original reference source remain.
Paired recall@20/MRR/NDCG/precision/gains/losses, costs and memory required.
Global groups remain a union of meeting-local inventories; other meetings are
unjudged, not known negatives. This is lexical research, not mash hybrid search.

Require complete non-timing repeat with begin/end pins; independent original
counts/query/span/score/rank/formula/metric audit, with scalar normalization
rounding versus executable exact ties explicitly separated. No selection from
mean improvements alone. Independent coding/semantic labels, multitag/ancestor
quality, held-out guided agent utility, online/global lifecycle and full
ingestion<=10% loss plus selected Python/Rust executable parity remain gates.
