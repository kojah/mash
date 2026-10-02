# Original-space topic admission: rejected frozen configuration

Bead `mash-c0d.19.2.30.4.3`. Preserve the complete PCA25 discovery and document
memberships, but replace projected query-topic comparisons with original-space
source-mean prototypes. This single preregistered admission intervention makes
retrieval worse under the unchanged cutoffs. It is not evidence that every
original-space prototype or threshold would fail.

## Procedure

Pin both [PCA25 captures](2026-10-02-fixed-pca-topic-routing.md), both original
semantic captures and the repeated [query-union retrieval baseline](2026-10-02-topic-filtered-retrieval.md).
Verify manifest identities and every topic/semantic array hash. For each of the
four order/membership snapshots, accumulate original 384D balanced vectors in
original-ID order, each assigned document contributing mass `1 / retained tags`
to each of its topics. Persist vector sums and masses. Normalize sums as topic
directions; empty or norm ≤1e-12 topics remain inactive, represented by zero
directions and -1 admission scores. Mass division would not change direction.
No inactive topics occurred: 20/20 forward and 11/11 reverse in both modes.

Keep original topic-index ties, .55 absolute/.85 relative/up-to-three selection
and original query directions. Document memberships and lexical labels never
change. Reconstruct the complete source-ID union before checking the 8,192
vector cap. On overflow retain metadata/coverage and explicit failure, no
truncated scoring or invented ranking. Under-cap unions use the previous exact
original-vector scorer, chunks 256, top 1,000 by signed cosine then source ID.
No hidden fallback or outside-union scores. All 256 records complete; none
exceeds the cap. Empty admission returns no candidates.

References enter only union-coverage/ranking metrics after candidate admission.
No query/reference fitting, reserved outcomes, new inference/downloads,
generative or paid calls occur. Prototypes are an offline retrofit over fixed
memberships, not online startup, edit/delete support or proven semantic parents.
Changing geometry changes the meaning of the same cosine cutoff; unchanged
numbers are a controlled policy, not equally calibrated acceptance rates.

## Results

| Order / membership | No topic, out of 64 | Union reference coverage | nDCG@10 | Recall@100 | Projected-admission Recall@100 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Forward / hard | 50 | .150391 | .031974 | .086914 | .314063 |
| Forward / overlap | 50 | .146647 | .026730 | .091634 | .308854 |
| Reverse / hard | 52 | .127930 | .024982 | .078613 | .276042 |
| Reverse / overlap | 53 | .109375 | .020211 | .062500 | .223958 |

Direct balanced semantic Recall@100 remains .739676. Macro denominators include
all 64 queries in each setting, including abstentions. Reverse hard candidate
recall is .127441, slightly below union coverage .127930: some admitted judged
references rank outside the top-1,000 cap. Other candidate recalls equal their
union coverage on this cohort; that is not a general guarantee.

Vector scores total 29,353/28,957/39,574/25,318 (123,202 overall), versus
73,868/71,416/84,267/57,397 for the unchanged projected-admission union
baseline. Lower work here results from more abstentions, not a useful speed
gain. Maximum unions 4,607/4,407/5,891/4,437 remain below 8,192. Hard prototype
mass sums to all 32,176 documents; overlap sums to 24,704/18,649 assigned
documents within floating reduction error. Unassigned sources are retained.

This control rejects the original-space prototype plus frozen cutoff policy;
it does not isolate whether query-to-broad-topic mismatch, cutoff calibration,
source aggregation or topic geometry causes the loss. Do not retrospectively
lower thresholds to repair the exposed result. Improving automatic routing
does not itself validate topic labels, multi-topic precision or semantic ancestry.

## Audits and cost

Fresh repeats match every decoded nontiming field. Independent per-topic
reductions rebuild sums, mass, active masks and normalized directions from
original vectors and fixed memberships. Maximum sum error zero, mass error
4.78e-12 and direction error 2.78e-17. A separate membership traversal checks
all 256 admitted unions, selection scores/ties/cutoffs and operational states.
All 123,202 vector scores (47,309,568 component products), candidate sorting,
reference ranks, top-100 records, coverage and macro metrics pass. Maximum
alternative-reduction score error is 3.34e-16. Ranks use verified captured
scores; no cross-platform threshold/tie proof or inference parity follows.
Duplicate judgments are development retrieval proxies, not intrinsic topic gold.
362 tests pass, including equal document mass, empty/canceling prototype
abstention and complete metadata retained beyond the vector cap.

Same Linux/AMD EPYC/Python/science environment as preceding studies, one numeric
thread. First/repeat after-import wall time 8.126/8.832 s; setup 2.417/2.771 s;
four prototype fits .161/.101/.118/.077 s first, .149/.115/.134/.079 s repeat.
First admission/union/retrieval/metric p50/p95 milliseconds:
17.25/22.98, 17.39/21.31, 17.97/25.54, 17.80/23.29. Repeat:
19.80/24.74, 19.05/24.76, 19.32/26.87, 17.27/22.68.

The metadata helper scans all 32,176 document membership rows per query;
its `posting_visits` field counts admitted hits, not all traversed records.
Across four snapshots/64 queries it checks 8,237,056 rows and 7,377,472
individual memberships. This is a research diagnostic, not an efficient
production inverted-posting implementation. Timers include research validation
and metric construction, exclude inference/storage/CLI/MCP/HTTP; concurrent
audit and monitor overhead prevent controlled speed comparisons.

External first/repeat samples 410/405, one/zero vanished reads, requested sleep
20 ms; actual median/max gaps 28.25/54.19 and 29.78/50.17 ms. Smaps RSS
maxima 547,112/547,340 KiB, PSS 539,558/535,569 KiB; approximate status VmHWM
548,992/547,992 KiB; separate rusage has the same HWM values here. These
counters differ in scope/timing; do not subtract them. Sampled maxima are lower
bounds, target-only scope excludes children/observer, and retained snapshots are
not production worker memory or ≤10% complete-ingestion evidence.

## Reproduction and decision

`cqadup_topic_prototypes.py` takes the previous frozen topic/semantic first/repeat
arguments plus `--baseline PACKING_FIRST --baseline-repeat PACKING_SECOND
--output FRESH_JSON`. Wrap with `memory_audit.py`; fresh repeat adds
`--compare FIRST_JSON`. Outside-Git artifacts under mash-artifacts:

- First `topic-cqadup-prototypes-first-20261002.json`, SHA256
  `9235aee1183cd53bccc0515317ccad125a980fb85bcefa5212ab85489d6695b7`.
- Repeat `topic-cqadup-prototypes-second-20261002.json`, SHA256
  `639410b13f0a31fe28dc2290d95ff734e2c05086ea32b190cd3a2b482b9fd173`.
- Final audit `topic-cqadup-prototypes-audit-final-20261002.json`; initial audit retained.
- Monitors `topic-cqadup-prototypes-memory-first-20261002.json` and
  `topic-cqadup-prototypes-memory-second-20261002.json`.

Reject this configuration without tuning exposed thresholds. Intrinsic topic
and label quality must be evaluated separately from automatic query admission;
ordinary search should not depend on an unvalidated exclusive topic gate.
No topic method is selected. Independent topic/ancestor judgments, actual guided
MCP navigation, provider generalization, online lifecycle, full ingestion and
Rust parity remain unresolved under the parent bead.
