# Flat multi-label topic refresh control

## Scope and procedure

The user removed semantic hierarchy from the first milestone. This experiment
implements a genuinely flat research path, not just a tree result with hidden
parents. `topic_flat.py` produces topic records, memberships and thread tag unions
without constructing linkage, branch nodes or ancestor tags. Production remains
Rust-only; no algorithm or public MCP behavior is promoted by this control.

Assignment, tokenization/counting and label scoring are shared in
`topic_features.py`. The tree control preserves its historical class statistics;
flat labels use only flat topic classes, avoiding repeated ancestor contributions
to class length/frequency. Global label eligibility is computed once per refresh
rather than repeatedly per topic. This optimization affects both matched arms.
Thus old timings are not an isolated measure of hierarchy removal.

Reuse the same frozen exposed 128-thread/1,850-admitted-occurrence cohort,
compatible stored vectors and 7,038 explicit exclusions. Database SHA256
`251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`;
selection SHA256
`270fdf50405136931791905b8555f5e4d7a35cb5a5fd935c4a201fd72b618167`.
The historical capacity control SHA256 is
`ec263fea496a7ab70a747d61b6d6c8705969a8aee35ce7f908e6657671d19261`.
No new inference, download, LLM, paid call, selection or threshold fitting.
This is development data, not held-out semantic evidence.

Four fixed arms run forward/reverse arrival and exact checkpoint/refreshed
restart: tree final-only, flat requests every 16 events, flat every 256 events,
and flat final-only. All use capacity 2,000, threshold .55, relative margin .8,
maximum three tags, vocabulary 8,192, source support two and maximum source DF
.4. Original source counts are retained. The queue has one worker and one
coalesced request, not a snapshot backlog. The 256-event cadence is a control,
not a chosen CPU-budget or maximum-staleness policy. Final-only has no
intermediate publication. Fixed arm order, caches, Python GIL, scheduling and
monitoring constrain timing comparisons. Complete-ingestion costs remain open.

## Interpretation

Hierarchy removal should not be presented as a topic-quality improvement.
Assignments are deliberately unchanged in this control. Flat class statistics
can change label scores/ranks; scalar-score correctness is not label usefulness.
Many singletons and weak-label topics still require granularity and independently
reviewed coding-session evaluation. A supported keyword is not a semantic gold
topic, and repeated identical inputs are not independent evidence.

## Results and verification

The shared-code tree control exactly reproduces the historical complete snapshots
in both orders, despite precomputing global label eligibility. Flat memberships
and source versions exactly match that control. All three flat schedules have
identical complete final snapshots, and all non-runtime fields repeat exactly.
Forward retains 870 topics, 593 singletons, 698 weak-label topics, 241 multi-tag
sources and ten untagged admitted sources (99.46% covered); reverse has 856,
578, 673, 234 and seven respectively. Pair Jaccard remains .757117, with
23,171/27,130 co-membership pairs. This is stability, not accuracy.

Flat-only class statistics change 249 forward topic label arrays (scores and/or
rankings) relative to the tree control. This is an intentional formula-domain
change, not a claim of improved labeling. Weak-label counts do not improve.

The auditor imports no producer modules. It independently counts source tokens,
reconstructs flat class TF/length/global frequency and evaluates emitted label
scores with scalar arithmetic. It checks six flat snapshots, 5,178 topics and
5,454 label scores: maximum error 1.77636e-15. Source membership unions, scoped
thread/provider tags, support/DF constraints and schedule/repeat equality pass.
The static English stoplist and sklearn max-feature selection are shared; this
does not independently validate routing winners, full vocabulary/rank selection,
native parsing, labels' semantic usefulness or guided-agent quality.

The research suite passes 641 tests in 4.642 seconds, including four new
fixtures for no-tree overlap, flat-only label arithmetic, restart/edit/delete/queue
behavior and empty/invalid inputs. No selected-method Rust parity is claimed.

## Matched runtime and memory

| Forward measure, first / repeat | Tree final | Flat every 16 | Flat every 256 | Flat final |
| --- | ---: | ---: | ---: | ---: |
| Routing wall seconds | 5.21 / 4.54 | 23.84 / 27.31 | 15.47 / 25.98 | 4.35 / 5.00 |
| Through final publication seconds | 9.84 / 7.72 | 26.57 / 29.81 | 17.76 / 34.29 | 6.14 / 7.21 |
| Process CPU seconds | 8.50 / 7.70 | 28.69 / 25.59 | 19.09 / 21.30 | 6.08 / 6.85 |
| Foreground upsert p95 ms | 6.15 / 4.93 | 31.83 / 43.76 | 25.52 / 59.63 | 5.33 / 6.03 |
| Completed refreshes | 1 / 1 | 15 / 14 | 8 / 8 | 1 / 1 |

p95 uses the sorted sample at floor(.95*(N-1)). Flat final publication is
1.60/1.07 times faster than the matched tree final control. Flat 256-event
refresh reduces CPU versus every 16, but is slower in the second wall-time run;
it is not a selected policy or consistent wall-time win. Flat final publication
is faster than frequent refresh in both runs, but withholds intermediate views.
Complete-ingestion <=10% throughput loss is untested. The full four-arm,
two-order/restart experiment is 154.94/200.19s wall and 158.40/161.43s CPU
after load, before serialization/repeat comparison. Runtime variation prevents
precise production extrapolation.

External target RSS peaks are 493,576/495,864 KiB and PSS 482,438/484,645 KiB.
Approximate status HWM is 501,776/495,300 KiB; pre-serialization rusage has the
same values, but is not a final-exit peak. The target holds the previous tree
baseline, all four arms, restarts and 12.81 MB result JSON, and repeat comparison.
These are not isolated flat-worker memory measurements. There are 5,325/5,631
samples, two/one vanished reads and maximum gaps 67.70/152.82ms. Samples are
lower bounds; status counters approximate; observer/children excluded; monitoring
intrusive. Do not subtract cumulative rusage or attribute it to an arm.

Result SHA256:

- First: `9b0abe50883562377e7871d640473cfdd888fb2f407574ddca9f451c3615cb68`.
- Repeat: `e2827a8490196dfd6616fc9493757db77d6294e4d9dd72eedabf05127538a004`.

Private outputs, logs and Linux memory sidecars use `topic-flat-{first,repeat}`
prefixes under mash-artifacts. Raw sources, vectors and topic labels are not
published in Git. Runtime counts cover the whole research target, not an isolated
production worker; per-case rusage is cumulative and cannot be subtracted.
