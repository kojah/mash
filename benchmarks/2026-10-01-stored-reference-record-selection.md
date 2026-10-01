# Stored-vector reference-record selection

2026-10-01; `mash-c0d.19.2.29`, following the visible-dialogue review.
This is a retrieval diagnostic, not accepted-passage selection or a promoted topic method.

Keep the original raw64 contexts and all21 reviewed anchors fixed. Inventory119
unique anchor/context records (35 user,84 assistant), checking canonical source
identity/content, embedding generation,384-dimensional signed-byte vectors and
stored norms. Every record has a compatible vector. The installed tokenizer counts
complete source text without truncation. Three assistant records and one user
record exceed512tokens. FastEmbed6.0.1 defaults to512-token truncation and consumes
the main encoding, so prefix vectors cannot be treated as complete-source evidence.
Generation and source checks do not independently establish historical embedding
input or model-byte parity.

Before inspecting rankings, freeze five controls: all-source recency; recency on
2–512-token records; and cosine against current-user, preceding-user, or normalized
captured-user-chain-mean vectors on the same token-supported assistant candidates.
No inference is performed. Query sources are causal, intact and visible; missing,
redacted, omitted, oversized or canceling query vectors abstain. Cosine uses
normalized float64 directions; equal scores use ascending source ordinal/id.
Recency uses descending ordinal then ascending id. Judgments never enter scoring.

Evaluate whether any manually cited assistant record appears in the first1/3
results. User citations are grounding evidence, not assistant retrieval targets.
Three references cite oversized assistant records and are unsupported for the
full-window controls. Other missing query support changes denominators explicitly.

| Method | Own supported n | Hit@1 | Hit@3 | Common n | Common hit@1 | Common hit@3 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Recency, all sources | 20 | 18 | 19 | 13 | 12 | 13 |
| Recency, full window | 17 | 16 | 17 | 13 | 12 | 13 |
| Current user cosine | 17 | 8 | 15 | 13 | 6 | 11 |
| Preceding user cosine | 14 | 7 | 11 | 13 | 7 | 10 |
| Captured user-chain cosine | 13 | 5 | 11 | 13 | 5 | 11 |

Recency beats these cosine controls on the common cohort. It still selects a broad
"fix all" interpretation where a reply narrows scope to easy/next-tier cases, and
an installation update where a request refers to deferred reorganization. A cited
compatibility proposal can also be correctly retrieved even though the user rejects
it. Therefore citation hits do not establish endorsement, fine accepted scope,
semantic precision, topic quality or guided-search gain. These exposed single
coding-assistant judgments cover two Codex groups, not independent human truth.
Fine accepted/background/rejected source-unit judgments remain the next step.

Two fresh captures match every nonmeasurement field, including complete method
grids, rankings, scores, source/vector hashes and parameters. Selection across all
21anchors/five controls takes2.64–2.67ms; tokenizer preflight21.34–26.20ms; total
validation/loading/ranking601.42–626.89ms. These exclude imports and are descriptive
batch costs, not ingestion throughput, production latency or memory gates.

Runners: `record_selection.py` and `compare_record_selection.py`. The140-test lab
suite covers strict token support, numerical ties/cancellation, causal query
eligibility, duplicate identities, role checks and complete repeat grids.
Private final captures under `topic-python-review-v3-20261001`:
`record-selection-confirmed-{first,second}.json`, SHA256
`ccccc17d0ef89b04a0da8d49f50ebaf053ee816ac54c8ac3f67209d3e5fbcf92`,
`34b34ccc0ad3c7bd46a364ee664068d87684abd8df6d7fc450d4511f99caa120`.
`record-selection-verified.json` records repeat verification. Earlier captures
without final timing/parameter provenance are excluded. Sources and judgments
remain outside Git; no production Rust behavior changes.
