# Query-relevant retrieval inside frozen topic filters

Bead `mash-c0d.19.2.30.4.2`. Packing-only control on the same complete public
duplicate corpus and 64 development queries. Ranking admitted members by query
relevance improves routed Recall@100, but topic admission still limits results
far below direct semantic search. This does not validate topic quality or agent
navigation and is not an equal-work speed comparison.

## Prospective control

Freeze the [PCA25 experiment](2026-10-02-fixed-pca-topic-routing.md) first/repeat
captures and [original semantic vectors](2026-10-02-duplicate-semantic-retrieval.md).
Require exact manifest pins, every captured array hash in both repeats and all
source/query identities. No discovery, document tags, labels, projection,
query-topic thresholds or selected topics change. Recompute admission to check
it matches the archived record before retrieval. Query/reference fitting is
forbidden; reserved queries stay unscored. No inference, model download,
generative or paid calls occur.

For each of 256 order/mode/query records, reconstruct the complete admitted
posting union, deduplicate original source identities and count posting visits.
Score only that union using original 384D cosine in chunks of 256. Work cap is
8,192 unique vectors; fail explicitly on overflow, never silently truncate.
No overflow occurs; previous maximum union is 5,891. Retain top 1,000 by signed
cosine descending, then original ID ascending. No outside-union prescoring or
fallback. Capture every union identity/score, candidates, reference ranks and
top-100 results. Empty admission remains an empty result.

Reexecute the archived centrality-packed baseline with its original selected
candidates and original-vector scoring. Verify exact metrics/reference ranks/
top-100 identities and score agreement within 1e-12. Its candidate packing is
already captured, so its timer excludes regenerating centrality packing; both
timers include research metric construction, while union timing includes posting
reconstruction, vector validation and selection. Neither measures production
filtered ANN, query encoding, HTTP/MCP/CLI or end-to-end search.

## Results

| Order / membership | Centrality nDCG@10 | Query-union nDCG@10 | Centrality Recall@100 | Query-union Recall@100 | Query-union candidate recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| Forward / hard | .108054 | .148504 | .238021 | .314063 | .397396 |
| Forward / overlap | .124828 | .140620 | .230729 | .308854 | .376563 |
| Reverse / hard | .050521 | .124535 | .140625 | .276042 | .341146 |
| Reverse / overlap | .073755 | .101680 | .153646 | .223958 | .286458 |

Direct balanced cosine remains nDCG@10 .351858/Recall@100 .739676. The union
control reproduces the earlier archived-score oracle Recall@100 through actual
retrieval, without using references to select candidates. Its candidate recall
equals admitted-union judged-reference coverage on this cohort. This is an
observed coincidence, not a guarantee that all union references rank top 1,000.
All queries, including the unchanged 27/39 forward/reverse abstentions, remain
in macro denominators. Membership/order quality is unchanged, not improved by
the downstream ranking control.

Vector scoring work across each set of 64 queries:

| Order / membership | Centrality vectors | Query-union vectors |
| --- | ---: | ---: |
| Forward / hard | 37,000 | 73,868 |
| Forward / overlap | 36,841 | 71,416 |
| Reverse / hard | 25,000 | 84,267 |
| Reverse / overlap | 25,000 | 57,397 |

Total 123,841 versus 286,948 vectors (about 2.32 times), not equal work.
All union scores use 110,188,032 component products. Improvements show the
centrality packing policy discards query-relevant members; they do not prove
better discovery or labels. Topic admission remains a major bottleneck:
even this improved retrieval cannot find sources outside admitted unions.
Original-space topic prototypes with existing memberships fixed are a separate
preregistered follow-up, not a retrospective threshold adjustment.

## Verification and cost

Two fresh runs match every decoded nontiming field. Independent audit rebuilds
unions from document memberships rather than helper posting lists, checks all
256 query records and all 286,948 scores using elementwise component products
and summation instead of matrix-vector multiplication, and independently checks
candidate sorting, top-100 ranks, metrics, macro means and operation counts.
Maximum score difference is 4.44e-16. Minimum observed top-1,000 cutoff gap is
5.09e-7; this does not establish every close-tie or cross-platform decision.
Ranking uses verified archived scores to preserve the defined implementation
contract. References are explicit existing development judgments, not topic gold.
360 research tests pass, including deduplicated overlapping postings, explicit
work overflow, signed scores/ties, chunk boundaries and no outside-union fallback.

Same Linux/AMD EPYC/Python/science environment as the PCA study, one numerical
thread. First/repeat after-import wall times 20.171/13.700 s; setup
2.786/3.246 s; separate rusage 392,476/391,900 KiB. The repeat overlaps its
independent audit; concurrent work and warming are uncontrolled. Repeat
centrality versus union p50/p95 milliseconds:

| Order / membership | Centrality p50 / p95 | Query-union p50 / p95 |
| --- | ---: | ---: |
| Forward / hard | 13.73 / 31.58 | 17.87 / 28.91 |
| Forward / overlap | 14.87 / 42.97 | 19.05 / 46.94 |
| Reverse / hard | 12.04 / 32.78 | 13.57 / 34.11 |
| Reverse / overlap | 13.00 / 34.46 | 14.02 / 40.30 |

External first/repeat monitors retain 529/492 samples, two/zero vanished reads,
requested sleep 20 ms, actual median/max gaps 38.71/162.12 and 32.65/102.06 ms.
Smaps RSS maxima 394,048/393,744 KiB, PSS 386,207/385,554 KiB; approximate
status VmHWM 393,788/393,744 KiB. Sampled maxima are lower bounds, target-only
scope excludes children/observer, and monitor overhead can perturb runtime.
This runner reuses snapshots rather than ingestion state; memory and timings
do not establish production worker use or the ≤10% full-ingestion gate.

## Reproduction

Run `cqadup_filtered_topics.py --topics PCA_FIRST --topic-repeat PCA_SECOND
--semantic SEMANTIC_FIRST --semantic-repeat SEMANTIC_SECOND --output FRESH_JSON`,
wrapped by `memory_audit.py`. Fresh repeat adds `--compare FIRST_JSON`.
Outside-Git artifacts under `/home/james/scratch/mash-artifacts`:

- First `topic-cqadup-filtered-first-20261002.json`, SHA256
  `076172dfeb1f9f78b428ec58f6249fb9ece70c99cf6c0f18506bc3b01b6086f0`.
- Repeat `topic-cqadup-filtered-second-20261002.json`, SHA256
  `7c32194bd6d74c4d0bd2cbdbf66ae4be6f7c196feea496b5594ac853295fff1d`.
- Audit `topic-cqadup-filtered-audit-first-20261002.json`.
- Monitors `topic-cqadup-filtered-memory-first-20261002.json` and
  `topic-cqadup-filtered-memory-second-20261002.json`.

No production method is selected. Intrinsic topics/labels, semantic ancestry,
provider generalization, actual MCP navigation, online lifecycle/startup,
complete ingestion and Rust parity gates remain open.
