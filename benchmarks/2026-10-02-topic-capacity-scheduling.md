# Adequate capacity recovers coverage; refresh cadence dominates prototype cost

## Procedure

Reuse the frozen 128-thread cohort and all 1,850 admitted source occurrences
from `2026-10-02-128-thread-topic-pipeline.md`. The immutable database SHA256 is
`251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`;
selection SHA256 is
`270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`.
The 7,038 empty projections stay excluded. No inference, download, LLM or paid
call occurs. This is exposed development data, not a held-out quality test.

`topic_capacity_control.py` compares three fixed arms: capacity 128 with refresh
requests every 16 events, capacity 2,000 with the same cadence, and capacity
2,000 with one final refresh. Capacity 2,000 accommodates all 1,850 sources
within the existing exact hierarchy helper bound. The research constructor now
permits this explicit capacity; its default remains 128. The cosine threshold
stays .55, source/text budgets stay 2,048/8 MiB, and no forced merges or nearest
assignments compensate for capacity. Each arm runs forward and reverse arrival
and verifies checkpoint/refreshed restart. The fixed arm order is a timing
confound. Final-only publication offers no intermediate topic views.

## Results

| Forward diagnostic | Capacity 128 | Capacity 2,000, either schedule |
| --- | ---: | ---: |
| Anchored sources | 785 | 1,850 |
| Capacity rejections | 1,065 | 0 |
| Sources with a final tag | 822 (44.43%) | 1,840 (99.46%) |
| Leaves | 128 | 870 |
| Singleton leaves | 57 | 593 |
| Weak-label leaves | 70 | 698 |
| Multi-tag sources | 107 | 241 |
| Weak tree nodes | 81 | 924 |
| Forward/reverse pair Jaccard | .675079 | .757117 |

Adequate-capacity reverse arrival produces 856 leaves, seven untagged sources,
234 multi-tag sources and 899 weak nodes. Co-membership pairs are 23,171/27,130.
Both adequate-capacity schedules produce EXACTLY the same complete final
snapshots in both orders, including memberships, labels, hierarchy, versions
and thread/ancestor tags. All non-runtime fields match the repeated experiment.
The capacity-128 snapshots exactly reproduce the prior cohort experiment.

Coverage is recovered, not proven topic quality: 593/870 leaves are singletons
and 698/870 have fewer than two source-supported label terms. Duplicate inputs
can contribute support without independent evidence. Ancestor unions remain
structural rather than validated semantic relationships. No production method
is selected from these results.

## Scoped cost

| Forward measure, first / repeat | Capacity 128, every 16 | Capacity 2,000, every 16 | Capacity 2,000, final only |
| --- | ---: | ---: | ---: |
| Routing wall seconds | 11.43 / 11.64 | 35.45 / 40.01 | 7.07 / 4.54 |
| Through final publication seconds | 15.72 / 15.54 | 47.18 / 53.66 | 16.46 / 11.89 |
| Process CPU seconds | 16.30 / 15.99 | 49.27 / 55.50 | 14.09 / 11.77 |
| Completed background jobs | 7 / 7 | 8 / 8 | 1 / 1 |

Final-only publication is 2.87/4.51 times faster than frequent refresh at the same
adequate capacity. Python GIL contention, host scheduling, caches and the
observer affect timings. These are routing plus topic-refresh measurements,
not full ingestion with parsing, embedding, persistence and networking. The
provisional <=10% complete-ingestion overhead target remains untested.
Whole three-arm/two-order/restart experiments take 203.18/177.97s wall and
191.61/182.33s CPU after load, before serialization/comparison.

External sampled target smaps RSS peaks are 503,776/508,920 KiB; PSS peaks
495,828/501,042 KiB. Approximate status HWM is 503,584/508,020 KiB;
same-valued rusage was sampled before serialization, not at final exit.
Rusage at case completion is cumulative across the program, not attributable
to an arm or worker. The target includes imports, loading, all arms, restarts,
20.65 MB result serialization, and repeat comparison. Do not subtract counters
to estimate production worker memory. There are 6,374/6,052 samples, two
vanished reads each, and maximum gaps 166.81/55.01ms. Samples are lower bounds,
status counters approximate, observer/children excluded, monitoring intrusive.

## Verification and next step

The independent auditor imports no producer modules, shares sklearn's static
English stoplist, and reconstructs lexical source support and corpus DF.
It checks six snapshots, 7,410 nodes and 21,125 label entries; membership score
bounds, exact source/tree/thread unions, baseline equality and schedule/repeat
equality pass. It does not reconstruct routing winners, numeric label scores,
native parsing or semantic quality. The research suite passes 637 tests in
4.607s, including a 129-orthogonal-source capacity and restart fixture.

Private result SHA256:

- First: `ec263fea496a7ab70a747d61b6d6c8705969a8aee35ce7f908e6657671d19261`.
- Repeat: `f3a6e0c81c5e04d0aca929ac12d822815c78f699c1ef0c8e63f89ad861779482`.

Artifacts are `topic-capacity-{first,repeat}-20261002.json`, corresponding memory
sidecars/logs, and `topic-capacity-audit-20261002.json` under mash-artifacts.
Code pins are captured in results. Source text, labels and private artifacts
are not published in Git. Next evaluate a budgeted online refresh policy and
label/topic granularity using independent coding-chat judgments. Final-only
control is a performance diagnostic, not the selected online policy.
