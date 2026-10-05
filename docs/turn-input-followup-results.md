# Bounded input/context follow-up: completed

The [frozen protocol](turn-input-followup.md) was evaluated without changing
labels, models, thresholds or preprocessing after seeing scores. All four
representations were built for the 200 histories; only the 160 development
histories were scored. The 40-history confirmation set remains sealed.

**Decision: no eligible configuration.** All sixteen cells fail at least one
registered quality gate. Performance and provenance checks pass.

The most informative near-miss is whole-input ancestral context, BIRCH radius
0.60, cosine cutoff 0.75, centroid memberships. It is the only cell passing the
overall absolute floors and baseline gain, but fails the long-history floors.

| Configuration | Overall precision | Recall | F1 | Long precision | Recall | F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Raw, radius .75 / cutoff .65, centroid | .839 | .737 | .785 | .767 | .554 | .643 |
| Lean, radius .75 / cutoff .65, centroid | .842 | .728 | .781 | .796 | .548 | .649 |
| Context whole, radius .60 / cutoff .75, centroid | .887 | .724 | .798 | .855 | .564 | .679 |
| Context whole, radius .60 / cutoff .75, primary | .843 | .784 | .813 | .825 | .615 | .704 |
| Context current-prompt, radius .75 / cutoff .65, centroid | .816 | .814 | .815 | .758 | .688 | .721 |
| Context current-prompt, radius .75 / cutoff .65, primary | .719 | .950 | .818 | .639 | .901 | .748 |

The .798-F1 near-miss has TP=447, FP=57, FN=170 overall and
TP=177, FP=30, FN=137 on long histories. Its paired thread-bootstrap 95%
F1 interval is .758–.837; gain over chronological-four is .080–.170.
Intervals do not override the point-estimate gates. Broad primary BIRCH
assignments recover many positives but introduce false positives. Context helps
some configurations, while lean selection alone does not improve aggregate F1.
Current-prompt pooling also removes special-token states without context, so it
does not isolate the causal effect of ancestral attention.

## Coverage and performance

Every recorded follow-up slice was checked against its exact original Unicode
source range and permitted structural ancestry. The unchanged raw control
reproduced the original per-history confusion counts in both assignment modes.

Lean/context inputs supply 9,195,820 of 13,373,228 own-source characters (68.8%).
They completely exclude 16,925 of 27,440 sources, primarily intermediate
assistant segments. This is a substantial information reduction, not harmless
normalization. Context is supplied to 4,913 of 5,082 prompted turns. Character
coverage includes whitespace and is not a semantic-quality measure.

| Representation | Construction p95, seconds | Newly inferred vectors |
| --- | ---: | ---: |
| Raw separate views, original build | 14.197 | 8,760 |
| Lean | 5.402 | 3,528 |
| Context whole | 3.760 | 5,219 |
| Context current-prompt | 3.663 | 5,416 |

All timings are cache-assisted, with sequential cache reuse across views. They
are not independent cold-inference comparisons. Raw control-copy time is
excluded and its original construction costs are reported instead. The full
follow-up build took 609.48 seconds; scoring took 13.15 seconds. Maximum
development clustering p95 was 12.76 ms. Externally measured peak RSS was
1,502,240 KiB for building and 1,145,560 KiB for scoring. All registered
construction, clustering and 4-GiB memory budgets pass.

## Reproducibility and next step

Private artifacts are under
`/home/james/scratch/mash-artifacts/turn-input-followup-20261005`:

- Protocol SHA256: `9c58a5bc6d83f0fe2606d169ab688a17a9015530b98aeafd42b0eaa57f0cf6ca`.
- Development SHA256: `d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e`.
- Analysis SHA256: `d2fa16653a4175291c443a867d17bdc4018881727b45cc00f41777e17f6870a7`.

The next separately registered experiment is the user's bounded temporal
embedding smoothing proposal. It must keep the current vectors and labels
unchanged, report nearby topic-shift false positives and distant recurrence
recall, and preserve the overall and long-history quality gates. There is no
accepted algorithm or confirmation result yet.
