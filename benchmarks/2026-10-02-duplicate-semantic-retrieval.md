# Complete-source local semantic retrieval control

Bead `mash-c0d.19.2.30.3`. This is a development representation experiment,
not selection of a production topic algorithm. The source, split and lexical
controls are described in [preparation](2026-10-02-duplicate-retrieval-preparation.md)
and [lexical retrieval](2026-10-02-duplicate-lexical-retrieval.md); source coverage
and batching are verified in [window fidelity](2026-10-02-semantic-window-fidelity.md).

## Frozen procedure

Retain all 32,176 CQADupStack programmers candidates and the same 64 development
queries. Reserved query text/outcomes stay unopened. Use original corpus title
and body, query text only, without opaque metadata, reference features, source
rewriting or automatic synonym rules. Explicit positive duplicate judgments
define binary relevance; do not expand them through graph connectivity. Missing
judgments do not establish irrelevance. Every query has a positive reference,
so this cannot evaluate no-match abstention.

Use only the installed Xenova all-MiniLM-L6-v2 snapshot
`751bff37182d3f1213fa05d7196b954e230abad9`, quantized ONNX SHA256
`afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`,
tokenizer SHA256 `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.
No downloads, generative models or paid calls. The
[model card](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2)
lists Stack Exchange training data; unknown overlap prevents a zero-shot claim.

The pinned preflight contains all 77,979 contiguous whole-word windows, at most
254 payload plus two special tokens each. Preserve all original Unicode/UTF-8
spans and token/type IDs. Infer each window as a singleton on CPU with one
intra/inter-op thread, mask-aware float32 mean pooling and existing symmetric
int8 quantization followed by float64 unit normalization. The earlier mixed-batch
drift forbids assuming batching is equivalent. Record every unit vector.

Within each field, average unit directions by payload-token count and normalize
again, including singleton fields and queries. Title-only uses the title field;
balanced uses the normalized equal-weight mean of title and body fields. Empty
or canceling fields fail explicitly; none occur. Preserve signed cosine scores,
rank all candidates by descending exact score with lexical original-ID ties.
The fixed hybrid sums `1/(60+rank)` from complete balanced and BM25 rankings,
following [RRF](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf).
No mixture/threshold sweep or reserved-outcome selection is performed.

Environment: Linux x86-64, AMD EPYC 9645, Python 3.11.15, NumPy 2.4.6,
ONNX Runtime 1.30.0, tokenizers 0.23.2; OpenMP/OpenBLAS/MKL threads set to one.
Runner/dependency, source, cohort, preflight, asset and lexical-result hashes
are retained in the manifest's enumerated direct-file pins. Tracked research
sources are at Git `3d24898`; the pin list does not independently enumerate
every transitive helper. All complete vectors and scores stay outside Git.

## First-run outcomes

Macro averages over the same 64 queries:

| Representation | MRR@10 | nDCG@10 | Recall@10 | Recall@100 | Queries with judged hit @10 / @100 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Word BM25 control | .2621 | .2525 | .3268 | .5025 | 25 / 37 |
| Title cosine | .2818 | .2594 | .3550 | .6641 | 29 / 46 |
| Balanced cosine | .3445 | .3519 | .4889 | .7397 | 37 / 51 |
| Balanced + BM25 RRF60 | .3515 | .3535 | .4563 | .6905 | 34 / 48 |

Balanced recovers 7/16 common lexical top-100 misses; title recovers six and
RRF five. This supports testing the complete semantic representation further,
not topic memberships, labels, ancestors or improved agent navigation.

Posthoc paired percentile bootstrap (5,000 resamples, seed 17) on per-query
nDCG@10 gives balanced minus BM25 mean .0993, interval [.0030, .1993]; balanced
minus title .0925 [.0382, .1508]; RRF minus balanced .0016 [-.0845, .0887].
These are descriptive development intervals, without multiplicity correction
or a preregistered selection test. First-judged-rank wins/ties/losses are
41/8/15, 38/14/12 and 20/14/30 respectively. RRF reduces Recall@100 for five
queries and increases it for one; the negligible average nDCG change does not
justify promoting fusion.

Revisit the same first three lexical misses, chosen by ID before semantic
outcomes rather than selecting successful examples:

| Query → judged candidate | Word BM25 rank | Title rank | Balanced rank | RRF rank |
| --- | ---: | ---: | ---: | ---: |
| `118140` coworker benefits → `194922` rubber-duck debugging | 16,279 | 2,254 | 4,012 | 7,435 |
| `144717` single-page tradeoffs → `231062` AJAX/layout/modals | 691 | 353 | 188 | 298 |
| `153581` RESTful definition → `23891` REST in simple English | 1,586 | 4 | 3 | 36 |

The first two remain outside the top 100. The third improves strongly under
semantic scoring, while fusion demotes it. This does not establish that
unjudged higher-ranked documents are irrelevant or isolate a causal mechanism.

## Measured costs and verification

First feature build takes 678.582 seconds, including 627.305 seconds attributed
to inference. Total runner wall time after imports is 693.597 seconds. This is
an offline full-corpus experiment, not the complete initial/incremental ingestion
pipeline. Preflight preparation is separate. First-run rusage reports 1,613,884
KiB; that counter alone cannot attribute a workload peak because Linux may
retain pre-exec launcher peaks. The fresh monitored repeat completes in
788.944 seconds after imports, with feature build 753.114 seconds/inference
690.924 seconds. Monitoring and concurrent audit work confound timing;
this is not a controlled speed comparison or an ingestion gate.

The external monitor records 16,976 samples with three vanished-address-space
reads. The requested sleep interval is 20 ms; actual successive observation
gaps have median 42.38 ms and maximum 277.04 ms because sampling itself takes
time. Maximum observed status VmRSS/VmHWM and smaps_rollup RSS are 1,615,372
KiB; observed PSS is 1,607,528 KiB (about 1.54/1.53 GiB RSS/PSS). Repeat rusage
is separately 1,613,268 KiB. Do not subtract or equate the different counters.
Status counters are approximate; sampled RSS/PSS maxima are lower bounds,
not continuous peaks. Scope is the single target process, excluding children
and observer; monitoring may perturb runtime. Source loading, ledgers and
retained vectors contribute to this research process, so these values do not
describe production local-worker memory or complete ingestion overhead.

With stored query vectors, scoring plus complete ranking and evaluation p50/p95
is 27.82/39.14 ms (title), 26.25/36.77 ms (balanced) and 33.42/43.94 ms (RRF).
A separate fresh warm encoder reproduces all 64 raw unit vectors, aggregated
query fields, complete top-100 scores, judged ranks and metrics exactly, using
64 model calls and zero cache hits. Model initialization is 129.88 ms; query
encoding plus field aggregation p50/p95 is 2.98/3.86 ms. Encoding plus complete
offline evaluation is 21.78/27.84 ms (title), 21.04/26.15 ms (balanced) and
27.34/32.98 ms (RRF).
The RRF check reuses the frozen BM25 score row, excluding lexical query scoring.
These exclude source loading/initial build and include evaluation metrics absent
from production. Read order and OS cache are uncontrolled; HTTP, CLI/MCP,
network, filters and reranking are absent. These are not production latency.

The warm checker initially compared a raw unit to a post-aggregation field.
The first query differed by 2.78e-17 despite exact same-stage agreement. Correcting
the checker preserved experiment/model/dataset semantics. Stage-aligned comparisons
and singleton normalization are now explicit in the Rust port contract.

The independent audit verifies all 77,979 unit norms, complete field aggregation
and balanced vectors (maximum component error 5.55e-17), plus 16 fixed probe
rows exactly against the executable singleton reference. A separate einsum
reduction checks all 6,177,792 scores within 1e-12 (maximum error 6.66e-16),
with scalar fsum checks on every 503rd document for each query (maximum error
2.22e-16). RRF reconstruction and independent sorting of archived exact scores
verify all top-100 scores, judged ranks, per-query and macro metrics. This shares
the verified source loader and does not independently infer every unit or prove
cross-platform close-score tie stability. The fresh full-inference repeat
matches every nontiming manifest field, including all eight complete vector/score
file hashes, metrics and source/model/code pins. A separate decoded-manifest
comparison confirms that only the top-level timing object is excluded.
Python reproducibility is not Rust parity. No production topic
method, semantic tree, guided-search gain or ≤10% ingestion-loss claim follows.

## Reproduction and retained evidence

Run `cqadup_semantic.py --root SOURCE_ROOT --preflight PREFLIGHT_JSON
--preflight-sha256 PIN --snapshot INSTALLED_SNAPSHOT --lexical LEXICAL_DIRECTORY
--output-directory FRESH_DIRECTORY`, then repeat with
`--compare FIRST_DIRECTORY/results.json`. For Linux memory evidence, wrap the
repeat with `memory_audit.py --output FRESH_AUDIT_JSON -- PYTHON RUNNER ...`.
Do not edit pinned runner/dependencies while it is live.

Artifacts under `/home/james/scratch/mash-artifacts`:

- First result: `topic-cqadup-semantic-first-20261002/results.json`, SHA256
  `d6fec0c4f7fba4023487e2fc217cdf25820ea79d480bced0c370371bef350fb3`.
- Warm check: `topic-cqadup-semantic-warm-first-20261002.json`, SHA256
  `88920ae0f993803f1c28f3feac4524d884b4057fcd0f233a2a27a8bb531183c6`.
- Posthoc analysis: `topic-cqadup-semantic-analysis-first-20261002.json`, SHA256
  `686a0b0d0820a93768a233e384c168be80fa5c528b3a2316537ac5b88b7068ec`.
- Repeat result: `topic-cqadup-semantic-second-20261002/results.json`, SHA256
  `6c3b9cd9471b514270b31b6df4561545bac45511f9aad9298b27457601627109`.
- Monitor: `topic-cqadup-semantic-memory-second-20261002.json`, SHA256
  `efe16aeeedf165158032903ba04ba36b5886a14fa431bc10cad7a63aadc3756d`.

Before promotion, independent agent-topic judgments, rare-topic/overlap and
ancestor correctness, cross-provider quality, lifecycle/order stability, full
ingestion/RSS/p95 measurements and differential Rust parity remain required.
