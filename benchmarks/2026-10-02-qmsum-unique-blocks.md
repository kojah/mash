# Unique-block return geometry before topic discovery

The [message-expanded control](2026-10-02-qmsum-block-geometry.md) exposed heavy
duplication. This prospectively frozen follow-up returns one result per physical
block for **every** branch and deduplicates branch rankings before RRF. It changes
result granularity, so its AP/NDCG values are not direct improvements over the
previous message-level numbers. No topic-discovery algorithm is selected here.

## Frozen procedure

Reuse the complete audited source/16-turn512-original-token/gap blocks, ready
mask, raw directions, exact-marker counts and full-training-source IDF. Original
geometry SHA256
`3f0b2c8632b8dda5ad34f3d058032cb24afb99d776998060e603b92cac63fdb1`;
block file SHA256
`95882ec78c20cf5ff0065343bf9463f94101428aecb3a62b22b6edd13cefb436`.
Same162 meetings/88,336 turns/8196 blocks,77,892 singleton-known sources and2410
return queries across29 native topics/21 meetings. No new fitting, budget/grid,
inference, model download, LLM or author held-out source access.

Candidate sources match the original control exactly: singleton-known, original
gap>4, another physical block, and not the same native topic/component. Same-topic
different-span sources are positive. Group eligible sources by physical block;
the smallest eligible source ordinal is a stable identity/tie key, not a promised
best-passage display location. Branch score is maximum eligible source score:
raw/causal/lexical/chronological branches retain their strongest passage, pooled
branches have repeated block-owned scores. Positive-only round12 descending ranks
then identity ties. Deduplicate dense and lexical branches **before** RRF60;
each block supplies at most one branch vote and one displayed result.

Binary relevance means any eligible known member matches the target native return
topic. Graded gain is relevant-known/eligible-known member count, not a judgment
that unannotated members are irrelevant. NDCG10 uses linear gain, `log2(rank+1)`
discount and ideal top10 normalization, an explicit variant of
[graded cumulative-gain evaluation](https://researchportal.tuni.fi/en/publications/cumulated-gain-based-evaluation-of-ir-techniques).
Unknown/multi-annotated turns remain in pooled features but outside conditional
gold. Report full query group ownership/counts/gains, positive ranks/top10/block
identities/hash, AP/MRR/Hit10/NDCG10, topic-macro summaries and zero-score abstention.

Same54 reserved-internal and108 development meetings. Both were explored before
these experiments, and IDF uses all training sources: this is not pristine held-out
evaluation or model calibration. Source turns represented by final closed-block
snapshots remain pseudoqueries; block query vectors can include later turns until
closure. No online draft, semantic segmentation, clock flushing or lifecycle claim.

## Complete outcomes

Across all query candidate sets (not unique global objects),657,019 source entries
collapse to77,689 block entries;168,646 positive source entries become22,454
positive block entries,4834 with mixed target/non-target known members. These are
query-summed counts; source support and original eligibility remain unchanged.

| Branch | MAP | MRR | Hit10 | NDCG10 | Topic-macro AP |
|---|---:|---:|---:|---:|---:|
| Raw dense | .409942 | .518856 | .868880 | .364970 | .359529 |
| Causal-five dense | .415841 | .524765 | .876763 | .371095 | .366333 |
| Block dense | .414960 | .534932 | .904979 | .364722 | .409335 |
| Raw lexical | .344398 | .490227 | .845228 | .340797 | .310253 |
| Block lexical | .444324 | .544308 | .950207 | .406822 | .444490 |
| Raw RRF60 | .410183 | .516931 | .865560 | .362721 | .364554 |
| Block RRF60 | .434942 | .560762 | .951037 | .394485 | .438835 |
| Chronological distance | .325771 | .184808 | .512033 | .186933 | .285844 |

Raw lexical has30 zero-result queries, other branches none. Reserved-internal703
queries/11 topics/9 meetings: raw dense MAP/Hit10/NDCG .373289/.908962/.346211;
block dense .373231/.923186/.336840; block lexical .423489/.943101/.399059;
block RRF .405174/.960171/.375114. Dense pooling is mixed, not an unequivocal
upgrade. Lexical blocks are promising candidate evidence for macro discovery;
fusion is not a clear winner over lexical alone on average/graded relevance.

Explicitly post-hoc,2000 paired meeting bootstrap resamples of all162 original
meetings with replacement, seed20261002, retain each correlated query cluster and
query weighting. Contrasts were chosen after inspecting exploratory outcomes;
only21 meetings have return support, so uncertainty is substantial:

| Contrast | Mean difference | Descriptive percentile95 interval |
|---|---:|---:|
| Block lexical minus raw dense MAP | +.034381 | [-.010033,.080861] |
| Same NDCG10 | +.041852 | [-.021012,.105168] |
| Same Hit10 | +.081328 | [.003090,.200573] |
| Block dense minus raw dense MAP | +.005018 | [-.009300,.023823] |
| Same NDCG10 | -.000248 | [-.018688,.021878] |
| Same Hit10 | +.036100 | [.015990,.057955] |

Do not treat correlated queries/pairs as independent or call these exploratory
intervals unseen-corpus confidence. Average/graded lexical-gain intervals include
zero. Block-RRF minus lexical MAP -.009381 interval[-.035121,.016919], NDCG
-.012337[-.049645,.024772], Hit10+.000830[-.035220,.044053]. Prioritize a lexical
macro-discovery pilot with dense and chronology controls, not production promotion.
Retrieval of broad meeting returns does not establish semantic cluster/label/ancestor
quality, stable global tags or guided-search gains on coding sessions.

## Validation and scoped cost

Four new fixtures check unique ownership/mixed known gain, no duplicate branch
votes, stable identities/maxima independent of relevance, abstention/ideal DCG,
invalid duplicate sources and missing score ownership. Full enabled lab:480 pass.
Complete fresh repeat matches every nontiming field, group/count/gain/rank/hash,
source/protocol/code pin and summary. First/repeat result SHA256:
`6f17cddf1619e27039283cb76a2ad80e59ed5bd09dfdbff77c0cc4d7aa93e254` /
`12c73c754c4651ef1a65de6fd59d4c9300d3dc5a5627ae28937081bdaefc2b94`.

Separate auditor imports no lab grouping/ranking/source/feature helpers. Original
JSONL memberships and eligibility, source lexical/count aggregation, cosine
branches, dictionary group ownership and maxima, Python sorting, duplicate-free
fusion, scalar linear-gain DCG/AP and summaries verify all2410 queries/77,689
groups/19,280 rankings/24 summaries. Integer support, all positive ranks/top10/
block identities/hash exact; max metric error3.33e-16. Shares NumPy/SciPy and
prerequisite original block/inference/count audits; not an independent encoder,
semantic coding review or Rust parity gate. Auditor SHA256
`6666b07991a2b0b3517c67a09c3b852c04ad44796132b449276794e65bd2801c`;
terminal `topic-qmsum-unique-blocks-audit-first-20261002.json`.

Offline first after-import wall/CPU20.466/16.583s, setup14.427s; repeat18.835/
15.697s, setup13.232s. Score assembly .677/.660s, grouping/evaluation3.289/2.975s.
These are scientific query-evaluation totals, not ingestion or production query
latency. Imports/final JSON/repeat comparison are outside stage timer, inside
external target scope. First briefly overlaps lab validation startup; no speed
claim from timing variation, monitoring is intrusive.

Rusage2048304/2048476KiB; sampled first RSS/PSS/status2049192/2041354/2049192KiB,
632 samples,one vanished,median/max gaps33.69/83.39ms. Repeat2048992/2041155/
2048992KiB,619 samples,one vanished,gaps34.11/70.34ms. Full offline capture/
ledger loading and scientific imports are not local model-worker memory. One
target includes output/comparison, observer/children excluded; sampled maxima
lower bounds, status approximate, launcher high water can persist; no subtraction.
No complete-ingestion throughput/p95, persistence or lifecycle gate.

Bootstrap/cost extraction script SHA256
`29f010cf0d2e93b8a2c6d134d5e61048d652a49c3c5aab2d17e376fffc513ae1`;
terminal `topic-qmsum-unique-blocks-diagnostics-first-20261002.json`. Artifacts
outside Git. Driver `qmsum_unique_blocks.py` uses previous geometry arguments plus
`--geometry ORIGINAL_DIR --output FRESH_FILE`, repeat `--compare FIRST.json`;
run via external `memory_audit.py`. Same pinned scientific environment/source.

Bead `mash-c0d.19.2.20.8.3.3.2.2` complete. P0 macro-discovery follow-up
`mash-c0d.19.2.20.8.3.3.3` freezes lexical/dense/temporal and compression/reference
controls before outcomes. Online feature-frame/IDF/vocabulary stability, source
labels/multi-tags/ancestors, provider/group namespaces, edit/delete repair,
complete-ingestion and chosen Python/Rust CI remain open. Full project goal active.
