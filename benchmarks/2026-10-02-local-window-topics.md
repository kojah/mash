# Local-window word communities: negative calibration and dependence audit

Bead `mash-c0d.19.2.35`. Following the whole-question graph's failure, this
prospective experiment tests smaller source contexts without changing the
vocabulary, question counts, membership rules or proxy gate. **None of 216
settings qualifies.** No Rust method, semantic tree or reserved-outcome winner
is selected.

## Hypothesis and fixed method

[WNTM](https://arxiv.org/pdf/1412.5404) constructs word networks from local
contexts and learns word-topic distributions. It motivates testing locality;
this experiment is **not WNTM**: it uses nonoverlapping windows, binary window
incidence, hypergeometric filtering and the previously verified Infomap/PLSA
comparator, rather than its sliding-window/Gibbs-sampling method.

Before outcomes, freeze windows of 16/32/64 lexical tokens. Partition each
original title and distinct admitted body source separately; never concatenate
sources or remove stopwords before counting distance. Use the existing `TOKEN`
regex and unchanged 629-word eligible unigram vocabulary. Each word votes once
per window. Preserve every source character/UTF-8 byte in contiguous spans,
token counts and SHA256; zero-eligible windows remain in the null-model population.
Cap 4,096 windows per policy and fail without truncation. Existing body-unit
boundaries still constrain windows; this is not semantic episode segmentation.

All source counts, hashes, balanced question mass, exposed 64-question cohort,
2,016 pairs/97 positives and previous capture/code pins must match. The original
vocabulary remains transductive over 128 question sources plus 72 background
sources, but reserved tags/outcomes are unused. Test all 197,506 unordered word
pairs under each window population. Compare p<=.05 and BH q=.05 with the full
family. Windows from the same parent are correlated and lengths vary; this does
not demonstrate corpus FDR control or supply independent observations.

Keep three seeds, ten trials, one thread, hard mass/50-step sparse PLSA, coverage
denominator, relative .5 threshold, absolute .10/.20/.30 and budgets 3/8 unchanged:
216 configurations. All three seeds must satisfy precision >=.70 and >=20 links;
then maximize worst recall, worst precision and lexical key. No cutoff,
stopword or source repair follows observed outcomes. This remains a shared-tag
development proxy, not independent agent-chat topic quality.

## Results

All window policies cover **14,051 unfiltered lexical tokens**.

| Window | Windows / zero eligible | Raw edges | BH edges | Raw topics, seeds 17/29/101 | BH topics |
| --- | --- | ---: | ---: | --- | ---: |
| 16 | 1,031 / 44 | 3,388 | 113 | 50 / 46 / 46 | 43 |
| 32 | 617 / 19 | 4,567 | 113 | 40 / 37 / 38 | 40 |
| 64 | 418 / 9 | 5,919 | 92 | 31 / 31 / 28 | 32 |

Raw graphs cover all 629 words. BH covers 131/120/100 respectively; its
partitions agree across the three seeds. Repeatability does not imply useful
topics.

For raw graphs, descriptive best precision among settings with >=20 links:

| Window | Hard TP/FP; precision; recall | Mixture TP/FP; precision; recall |
| --- | --- | --- |
| 16 | 8/17; .320; .082 | 5/19; .208; .052 |
| 32 | 8/21; .276; .082 | 6/25; .194; .062 |
| 64 | 16/38; .296; .165 | 11/44; .200; .113 |

These maxima all use seed17/budget3. Hard results use threshold.30; mixture
thresholds are .10/.30/.30. They are post-outcome descriptions, not selected
settings or held-out gains. BH best hard precisions are .108/.139/.167 and
mixture .148/.050/.050. The robust selector returns null. The raw 16-window
hard result leaves 23 questions unassigned and emits 45 memberships.

## Distinct-parent support: diagnostic after outcomes

Independently count the number of different source questions with each selected
pair **co-occurring inside a window**. This is a post-outcome explanation,
not an extra filter secretly added to the experiment:

| Window | Raw edges supported by only one parent | BH edges supported by only one parent |
| --- | --- | --- |
| 16 | 3,022 / 3,388 (89.2%) | 63 / 113 (55.8%) |
| 32 | 3,902 / 4,567 (85.4%) | 63 / 113 (55.8%) |
| 64 | 4,853 / 5,919 (82.0%) | 41 / 92 (44.6%) |

Requiring each **word** to appear in at least two parents does not require its
**pair** to recur across parents. Multiple windows within one question can make
an edge look strongly supported. This identifies a dependence/replication issue
to address prospectively; it does not prove all single-parent edges are noise.
Simply counting more windows cannot be treated as more independent documents.

## Validation, artifacts and limits

Two fresh processes match every non-timing field. Independent reconstruction
checks every source partition, Unicode/byte span, SHA, token budget, incidence,
DF/co-occurrence, all **1,185,036** combinatorial probability tests/correction
decisions, and **435,456** membership/pair decisions. The unchanged sparse EM
implementation retains its dense-reference tests and earlier replay evidence;
this audit does not independently replay all 18 new mixture fits. All **278**
lab tests pass with the optional graph extra. Unicode, CRLF, combining marks,
empty evidence, pre-filter distances, source separation and caps are tested.

Total after imports is 11.265/8.525 seconds for all three policies and 216
configurations. Source-window construction takes .045–.153s/policy; filtering
.274–1.661s/filter. This includes repeated bounded research fits, not a single
production update. No model inference or downloads occur; earlier inference
remains charged. RSS, CPU, p95, deletion/restart and <=10% complete-ingestion
throughput gate remain unverified. No cheap online contract follows.

Outside-Git artifacts under `topic-public-stackoverflow-20261002`:

- `windows-first.json`: `62133ec99657c8bd9bf6e6e86473218a2e27347aae206d46922a337e347e0f50`
- `windows-second.json`: `0d86d0c5b18ce58f3fa16e5838f5b621f258eb0eacd3b1cb558c5b45991bd9a4`
- `verify-windows.py`: independent span/probability/pair/dependence audit.

Next preregister independent-parent pair support and parent-balanced evidence as
separate controls. Freeze them before outcomes, preserve these failures and
avoid presenting exposed development improvements as generalization. New groups,
independent primary/secondary/ancestor judgments, guided-search evaluation and
full maintenance cost remain necessary before selection or MCP/Rust promotion.
