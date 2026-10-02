# Original-space online cosine discovery: feasible, order-sensitive microgroups

Bead `mash-c0d.19.2.30.4.7`. This distinct discovery control covers every source
within the frozen cluster budget and avoids projection. It produces about 6,000
groups with many singleton groups and weak order agreement. Feasibility and
numerical cohesion do not establish useful semantic topics or a hierarchy.

## Prospective contract and prior art

Reuse the complete pinned [singleton semantic vectors](2026-10-02-duplicate-semantic-retrieval.md)
for all 32,176 sources. Original 384-dimensional balanced title/body unit vectors
are the only features. No query/qrel features, new inference, projection, fixed
cluster count, refitting or adaptive threshold. Insert in forward and reverse
canonical original-ID order, once each. Scan all current normalized cumulative
source-sum centroids; join the largest cosine iff it is at least .55, otherwise
create a cluster. Equality joins; exact ties select the earliest created centroid.
Update its cumulative sum/count and normalized direction. At 8,192 clusters,
record the attempted insertion and unprocessed count rather than truncate,
raise the cap or repair the cutoff. Retain every decision, nearest score,
assignment, sum/count matrix and number of centroid comparisons.

[Spherical k-means](https://www.jstatsoft.org/article/view/v050i10) provides prior
art for cosine prototype clustering. This control grows clusters at a fixed
cutoff; it is not a reproduction of fixed-K spherical k-means or its solvers.
[Links](https://arxiv.org/abs/1801.10123) specifically addresses streaming
high-dimensional unit vectors. Its algorithm is not implemented here; its
face/voice application evidence does not validate agent topic quality. This
simple baseline isolates original-space online centroid behavior after the
[unprojected BIRCH budget failure](2026-10-02-topic-routing-birch-budget.md) and
[overcompressed PCA25 groups](2026-10-02-fixed-pca-topic-routing.md). The numerical
.55 cosine cutoff has different semantics from a .55 Euclidean CF radius;
these are not equivalent settings or a controlled algorithm-only comparison.

Report insertion membership (no final reassignment), full-source ARI and the
previously frozen 512-source coassignment sample; cluster sizes, singleton mass
and final member-to-centroid cohesion. For each nonsingleton cluster, compute
mean ordered-pair cosine as `(norm(sum)^2 - n) / (n * (n - 1))`; singleton
pair affinity is undefined and excluded with an explicit denominator. This
identity uses unit source vectors and is not a semantic-quality estimator.
Report final centroid drift below the original insertion cutoff, without evicting
or silently reassigning members. No labels, overlapping tags, semantic parents
or retrieval outcomes are manufactured for this discovery-only control.

## Outcomes

| Diagnostic | Forward | Reverse |
| --- | --- | --- |
| Complete / unprocessed sources | 32,176 / 0 | 32,176 / 0 |
| Clusters | 6,018 | 5,965 |
| Singleton clusters | 3,693 | 3,653 |
| Singleton fraction of groups / documents | 61.37% / 11.48% | 61.24% / 11.35% |
| Median / p95 / maximum size | 1 / 23 / 407 | 1 / 24 / 394 |
| Median / p05 final member cosine | .7523 / .6104 | .7523 / .6100 |
| Members below .55 after centroid drift | 60 | 66 |
| Nonsingleton clusters / macro pair cosine | 2,325 / .5750 | 2,312 / .5736 |
| Exact centroid comparisons | 123,504,855 | 123,616,562 |

Full insertion ARI is .394702. Sample coassignment Jaccard is .282123:
101 intersecting pairs out of a union of 358, among 130,816 evaluated pairs.
Many groups have only one source; singleton document mass is much smaller than
singleton group fraction. Retaining rare sources is intentional, not a reason
to discard them as noise. Similar aggregate sizes/cohesion do not establish
stable source memberships. Insertion admission does not guarantee the cutoff
for earlier members after centroid movement; no lifecycle remedy is selected.

These are much finer groups than PCA25's 20/11 summaries, but a finer partition
is not automatically better semantic organization. No independent topic gold,
label quality, semantic ancestry or improved guided search is measured here.
This setting is not promoted to production. Bounded aggregation/assignment and
semantic hierarchy need their own evidence; structural parents are not tags.

## Verification and cost

374 research tests pass, including exact ties/cutoff equality, cumulative sums,
reverse source indexing, explicit capacity failure, singleton affinity and bad
input rejection. Fresh complete runs match all decoded nontiming fields and all
six array hashes. Independent alternate-reduction replay checks 64,352 inserted
events, all nearest/join/create decisions, cumulative and direct source sums,
counts/assignments, final cohesion and drift. Maximum score discrepancy is
1.22e-15; replay sum difference is zero. ARI replay uses the same sklearn
estimator; sample coassignment uses separate scalar pair loops. Frozen inference
is not independently re-executed by this audit.

First forward/reverse clustering costs 7.590/7.125 s; repeat 10.129/9.972 s.
First insertion p50/p95 is .230/.422 ms forward and .216/.391 ms reverse;
repeat .255/.682 and .246/.634 ms. These include Python scan/update/event
recording, not source parsing, encoding, labels, persistence or shared publication.
First/repeat after-import total is 16.491/21.923 s, CPU 16.326/21.126 s;
setup 1.024/1.023 s. The repeat overlaps independent auditing and sampling;
no controlled speed comparison or ≤10% complete-ingestion result follows.
Exact scan work grows with the current number of groups; no ANN speedup is claimed.

Separate rusage peaks are 543,292/542,208 KiB. External 666/827 samples yield
smaps RSS 532,044/537,180 KiB, PSS 524,344/512,439 KiB; approximate status
VmHWM 543,292/542,208 KiB. Requested sleep 20 ms, actual median/max gaps
27.07/48.83 and 27.86/76.73 ms, zero/one vanished reads. Both processes exit zero.
Sampled RSS/PSS maxima are lower bounds, status counters approximate; one target
excludes children and observer memory. Imports, retained source vectors, bounded
state/event arrays and monitoring contribute. These are research process costs,
not deployed worker memory, restart recovery or per-message ingestion latency.

## Reproduction and decision

Run `cqadup_online_cosine.py --semantic SEMANTIC_FIRST --semantic-repeat
SEMANTIC_SECOND --output-directory FRESH_DIRECTORY`, externally wrapped by
`memory_audit.py`; repeat adds `--compare FIRST_DIRECTORY/results.json`.
Use the pinned science environment and single-thread CPU settings of the semantic
study. Captures outside Git under `/home/james/scratch/mash-artifacts`:

- First `topic-cqadup-online-cosine-first-20261002/results.json`, SHA256
  `c33734421596da49d521551f9dcf82f469036afc8c94471478b55f37ded19eeb`.
- Repeat `topic-cqadup-online-cosine-second-20261002/results.json`, SHA256
  `8ec0e78a9f436bf806035e8e7c49f5c3e405b28ce8c9a20b971a9471629d2d0d`.
- Independent checker `topic-cqadup-online-cosine-audit-20261002.py` and verified
  `topic-cqadup-online-cosine-audit-first-20261002.json`.
- Monitors `topic-cqadup-online-cosine-memory-first-20261002.json` and
  `topic-cqadup-online-cosine-memory-second-20261002.json`.

Complete coverage under the budget is a useful feasibility result. It does not
justify replacing search or publishing thousands of microgroups as a topic tree.
Do not tune this exposed cutoff into a grid. Independent topic/name/navigation
quality, online edits/deletes and restarts, shared ownership/publication,
full-ingestion costs and Rust differential CI remain unverified requirements.
