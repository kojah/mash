# Grounded-query navigation on frozen topic groups

Frozen before outcomes, 2026-10-02. P0
`mash-c0d.19.2.20.8.3.3.3.9.2`. Evaluate utility before promoting clustering
or more populated labels. No LLM/inference/download, new stoplist, topic/cut/
threshold/weight sweep, answers or native topic names as retrieval features.

Reuse all 162 author-training meetings, 1,095 specific queries with 1,206
inclusive reference spans, and the pinned source-label-support capture.
General queries have no reference spans and are outside this defined task,
not silently dropped from specific-query denominators. Queries and spans enter
after discovery/label ranks are frozen. Never use answers. Preserve every
specific query, empty/OOV query, original reference source and unsupported
source denominator. Merge duplicate/overlapping spans by original source ID.

Features: existing original marker-cleaned counts, unchanged static stoplist,
frozen full-snapshot word IDF, sublinear TF before IDF and L2 normalization.
No fitting from queries or labels. Same frame for direct source words, original
ready-source counts pooled per group, and displayed label term bags (one count
per displayed term, no extra score weight). Include query word counts with the
existing tokenizer and exact marker accounting; unsupported vocabulary remains
explicit. Pooled counts precede TF/IDF, not means of source directions.

Four fixed arms: direct source retrieval; select groups from full group-word
evidence; select groups from physical-block-supported displayed labels; select
groups from original-message-supported displayed labels. Topic arms select at
most TWO groups, positive scores only, then use the SAME direct-source scores
within those groups. Each arm returns at most TWENTY positive-score original
ready sources; no zero-score padding, automatic full-source fallback or gold
choice of topic count. Exact float descending score and ascending global ID
break ties. This is a diagnostic hard navigation gate, not implemented mash
ranking or a benchmark of its hybrid/dense/reranked search.

Run paired scopes: meeting-conditioned (known thread analogue), and global
across all 162 meeting inventories. Topic discovery remains meeting-local;
global candidates are the union of those groups, not a shared discovered
taxonomy or online/global lifecycle implementation. Each query's reference is
its original meeting; other meetings cannot earn hits. Report macro recall@20,
fixed-budget precision@20, MRR and NDCG@20; all original relevant source IDs
stay in recall/ideal DCG denominators, including unsupported ones. Also report
returned/selected counts, zero-result queries, reference ready coverage, and
navigation candidate relevant-source coverage with its full denominator.

Capture all query identities/text pins/spans, selected groups and original
returned IDs/scores/metrics, all zero cases and fixed support. Require complete
non-timing repeat, begin/end code/input pins, independent original span/query/
count/score/rank/metric audit and timing/external RSS/PSS. Audit scalar rounding
differences within 1e-12 separately from exact executable tie/repeat behavior.
Inspect failures without modifying this protocol or tuning on results.

This is exploratory meeting-task evidence, not held-out coding-session
generalization, semantic-label/multitag/ancestor truth, a causal agent policy,
or a complete-ingestion/performance result. Independent coding quality, global
online lifecycle/edit/delete/restart, <=10% full-ingestion overhead and selected
Python/Rust executable parity remain promotion gates.
