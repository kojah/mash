# Discovered topics versus plain thread context

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.2`, Python-only research.
[Frozen protocol](../research/topics/THREAD_CONTEXT_PROTOCOL.md).
This control isolates clustering from ordinary context aggregation. No Rust,
application or deployment change, production selection or new model inference.

## Procedure

Same 162 QMSum training meetings, 88,336 original sources, 84,434 ready sources,
1,095 specific queries and 1,206 reference spans as the preceding experiment.
Same original counts/static stoplist/frozen full-snapshot IDF/sublinear TF/L2
source vectors. Pool original eligible counts before weighting to construct
one parent per meeting (162), versus the existing 1,296 fixed-eight lexical
topic groups. No answers/topic names as features, new filler list, held-out
read, grid, download, inference or LLM.

Compare direct positive-source retrieval and topic/thread parent guidance with
the fixed adaptation `1/(60+source_rank)+1/(60+parent_rank)`; parent term zero
if not positive. Complete positive rank pools, exact float/global-ID ties,
top 20, no mandatory gate, fallback or candidate cap. Parent rank is broadcast
to its sources: this is an adaptation, not ordinary same-document RRF.
Unsupported references remain in denominators. Global judgments cover only the
original meeting; other meetings are unjudged, not known negatives.

Source banks are exactly equal; all direct/topic records replay the frozen
reference. Within a known meeting, the sole thread parent adds a constant:
all 1,095 thread result orders and metrics must exactly equal direct retrieval.
Begin/end code pins and complete non-timing repeat pass.

## Results

Macro metrics over all queries, including zero-result cases:

| Scope / arm | Recall@20 | Precision@20 | MRR | NDCG@20 | Zero results |
|---|---:|---:|---:|---:|---:|
| Meeting / direct | .193067 | .171872 | .537946 | .276575 | 13 |
| Meeting / topic | .195939 | .173470 | .542060 | .280952 | 13 |
| Meeting / thread | .193067 | .171872 | .537946 | .276575 | 13 |
| Global / direct | .050725 | .024886 | .153729 | .060144 | 0 |
| Global / topic | .078522 | .042237 | .197217 | .090787 | 0 |
| Global / thread | .068532 | .036484 | .210416 | .086009 | 0 |

Topic-versus-thread recall improves/declines/ties on 111/34/950 global queries,
45/12/1,038 meeting queries. Versus direct, global counts are 238/13/844.
Candidate reference coverage stays .951957; 56,494 original contributions,
53,691 ready and 2,803 unsupported. No new zero-result cases.

Ordinary thread context reproduces substantial global gains. Topics provide
additional recall, precision and NDCG, but thread guidance has better MRR.
Mixed outcomes do not establish a universally better method, semantic topic
quality, or that clustering caused all earlier gains. Separate topic discovery
from search-ranking decisions; preserve the simpler baseline in held-out work.
Training meeting relevance is not coding-session gold or guided-agent utility.

## Cost and reproducibility

Median/p95 milliseconds per scope/arm, including retrieval and metric work:

| Scope / arm | First | Repeat |
|---|---:|---:|
| Meeting / direct | .199/.308 | .195/.304 |
| Meeting / topic | .355/.648 | .346/.588 |
| Meeting / thread | .195/.438 | .185/.382 |
| Global / direct | 8.243/12.635 | 7.697/10.871 |
| Global / topic | 24.873/50.981 | 22.716/33.400 |
| Global / thread | 24.113/51.802 | 22.061/32.190 |

Query projection excluded; direct includes reference equality checking, topic
equality checking is outside its measured interval. Fixed order direct/topic/
thread, cache/host effects uncontrolled: not a pure ranking-only comparison or
deployed E2E estimate. Complete first/repeat after-import wall 93.481/83.268s,
CPU 87.559/78.643s, setup 13.971/15.101s, feature construction 122.918/151.824ms.
Imports/final serialization/repeat comparison excluded from these main timings,
included externally. Large validation retains unused embedding/context ledgers;
all arms share one target. Scientific runs sequential; brief result inspection
overlaps repeat setup. No full-ingestion overhead claim.

Artifacts under `/home/james/scratch/mash-artifacts/`, outside Git:

- `topic-qmsum-thread-context-first-20261002.json`, SHA256
  `1d23e6af3bc94373b47ebb3900eb913fe974a6a8efae9815528059f7428f1dd4`.
- `topic-qmsum-thread-context-second-20261002.json`, SHA256
  `4b5eab908207c086c6b0632fd1dc39bc21b890a132501b3443b9c1dee58c5a0c`.
- `topic-qmsum-thread-context-audit-20261002.json`, SHA256
  `3f72a766886dad7c4aedbb0396dcabd4b23003d469a8c8398ae0ca87e57760e7`.
  Independent audit passes all 1,095 queries, 6,570 cases, 121,512 selected
  records and 702,141 feature coordinates; maximum error 2.22e-16. Original
  thread counts/pooling, queries/spans, ownership, reciprocal-rank formula,
  metrics and sole-parent invariant reconstructed without producer imports.
  Scalar fsum geometry checked within 1e-12; exact rank replay separately
  shares NumPy/SciPy IEEE kernels, not scalar tie-winner parity. Shared pinned
  counts/IDF/topic inventory/sklearn stoplist; tokenizer previously audited.
  Auditor SHA256 `6f8654286ecf4e8735201880b2387f99bd66b10a233bce4c1f55c316accc0619`;
  reusable audit reference SHA256
  `a8335735133093502aed4115905b3bdc2c2fcbe81413d89d406127f6751d55a7`.

Rusage peaks 2,143,240/2,143,052KiB; external RSS/PSS maxima
2,142,084/2,134,259 and 2,144,184/2,132,850KiB. Approximate status HWM
2,143,240/2,144,184KiB. Samples 2,629/2,417, vanished reads 1/1;
median/max gaps 35.120/77.794 and 33.920/77.290ms. One target, all arms;
children/observer excluded, monitoring intrusive, sampled maxima lower bounds,
status approximate, launcher rusage may retain high water. No subtraction,
per-arm memory attribution or worker-memory claim.

Two new sole-parent/zero/positive-pool fixtures bring the enabled research suite
to 570 passing tests. Numerical near-tie portability, held-out/coding quality,
semantic labels/multitags/ancestors, guided-agent utility, bounded shared online
lifecycle/edit/delete/restart, complete ingestion <=10% loss and selected
executable Python/Rust parity remain open gates.
