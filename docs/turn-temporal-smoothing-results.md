# Temporal embedding smoothing: completed

The [prospective protocol](turn-temporal-smoothing.md) fixed the anchor,
transformation, five settings and gates before scoring. All 160 development
histories were evaluated using cached vectors. No new inference or labels.
The unchanged control reproduced the preceding per-history predictions and
group sizes. Confirmation remains sealed.

**Decision: no eligible setting.** Smoothing improves development recall/F1,
but every setting fails the long-history quality floors.

| alpha / window | P | R | F1 | Long P | Long R | Long F1 | Nearby unrelated FPR | Distant related recall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 / 0 | .887 | .724 | .798 | .855 | .564 | .679 | .420 | .326 |
| .05 / 2 | .877 | .749 | .808 | .842 | .596 | .698 | .450 | .400 |
| .05 / 4 | .869 | .763 | .813 | .841 | .624 | .717 | .500 | .430 |
| .10 / 2 | .854 | .788 | .820 | .823 | .653 | .728 | .520 | .504 |
| .10 / 4 | .851 | .807 | .829 | .833 | .653 | .732 | .590 | .496 |

“Nearby” means <=4 prompted-turn positions for every setting; “distant” means
>4. Nearby FPR uses the 100 unrelated nearby pairs; distant recall uses the
135 related distant pairs. These subsets differ in composition, so their values
are not an estimate of unconditional production error rates.

The .10/window-4 setting has TP=498, FP=87, FN=119 overall, and
TP=205, FP=41, FN=109 on long histories. Its development F1 gain over the
unchanged anchor is .031; the paired 2,000-resample thread-bootstrap 95% gain
interval is .002–.060. This interval is exploratory, unadjusted for selection
and repeated development reuse. It does not override the failed quality gate.

Nearby unrelated false positives rise from 42/100 to 59/100. Distant related
matches rise from 44/135 to 67/135: bounded attraction does not merely recover
adjacent pairs, because refitting changes the partition and centroid memberships.
The best-F1 setting changes 12,616 co-membership links across all development
turns, including unjudged pairs. Its p95 cosine displacement is only .00338.
This shows partition/membership sensitivity to modest vector changes; it does
not establish that all changed links are correct. Group IDs are not compared
because cluster renumbering is arbitrary.

Complete scoring takes 7.71 seconds, with measured peak RSS 1,049,016 KiB.
Maximum per-history transformation-plus-clustering p95 is 14.67 ms, including
ancestry construction. Performance gates pass; the base embedding construction
costs are unchanged from the preceding measured study.

Private artifacts are in
`/home/james/scratch/mash-artifacts/turn-temporal-smoothing-20261005`.

- Protocol SHA256: `7463c0311fa6878ec93187ece79a68820448438f4d3f7f35f0b0f8a39aa4ade9`.
- Development SHA256: `d4ab356b33f2fbcd4905fc61fcceb2cb1d5793d1735eb1d088fe69ca0fedcb48`.

No additional weights/windows will be tuned on this result. Smoothing remains
experimental. The remaining problem is accurate coverage of multiple subjects
in long histories, rather than construction speed. No algorithm is accepted yet.
