# Three-factor ablations: completed

The user fixed the original-smoothed pooled/heading-chunk union as the baseline
and requested ablations of **C**urrent-prompt answer conditioning,
evidence-aware **F**usion, and **G**eneric-answer confidence weighting. The
[prospective protocol](turn-three-factor-ablation.md) froze all eight cells
before scoring, with no parameter grid. All 160 development histories and
1,051 reference pairs were evaluated; 121 ambiguous pairs are excluded from
quality metrics. Confirmation remains sealed.

**Recommendation: retain the frozen baseline.** C has a small, uncertain gain;
F sacrifices too much recall; G has no effect on the judged pairs. None meets
every registered overall/long-history quality floor. Do not retune or promote
these treatments from this screen.

## All eight results

Structural boundaries, original smoothing, BIRCH radius .60 and ordinary
membership cutoff .75 remain fixed. F introduces the registered .80 threshold
for single-view evidence; G subtracts .05 from flagged answer confidence.

| C | F | G | Overall P | Overall R | Overall F1 | Long P | Long R | Long F1 |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| off | off | off | .847 | .862 | .855 | .824 | .745 | .783 |
| off | off | on | .847 | .862 | .855 | .824 | .745 | .783 |
| off | on | off | .876 | .799 | .836 | .841 | .659 | .739 |
| off | on | on | .876 | .799 | .836 | .841 | .659 | .739 |
| on | off | off | .848 | .865 | .856 | .826 | .755 | .789 |
| on | off | on | .848 | .865 | .856 | .826 | .755 | .789 |
| on | on | off | .873 | .806 | .838 | .837 | .672 | .746 |
| on | on | on | .873 | .806 | .838 | .837 | .672 | .746 |

Baseline TP/FP/FN is 532/96/85 overall and 234/50/80 long. C alone yields
534/96/83 overall and 237/50/77 long. F alone yields 493/70/124 overall and
207/39/107 long. C+F yields 497/72/120 overall and 211/41/103 long.
Adding G leaves each corresponding confusion matrix and reference prediction
unchanged. Full confusion matrices, strata and per-history predictions are in
the private typed development report.

## One-factor and leave-one-out effects

| Contrast | Overall delta F1 | Long delta F1 | Paired overall F1-gain interval |
| --- | ---: | ---: | --- |
| C only vs baseline | +.00184 | +.00608 | -.00483 to .00921 |
| F only vs baseline | -.01903 | -.04332 | -.03436 to -.00472 |
| G only vs baseline | .00000 | .00000 | .00000 to .00000 |
| Full vs removing C | +.00252 | +.00630 | -.00661 to .01131 |
| Full vs removing F | -.01834 | -.04310 | -.03217 to -.00511 |
| Full vs removing G | .00000 | .00000 | .00000 to .00000 |

C raises long recall .745 to .755 without increasing long FP count, but its
overall paired interval includes no gain. Nearby related TP rises 460 to 463
and nearby FP 64 to 65; distant related TP falls 72 to 71 and distant FP 32
to 31. It therefore does not demonstrate improved distant-topic recovery.
C jointly adds prompt conditioning and answer-only pooling; the experiment
does not isolate those mechanisms independently.

F raises overall precision .847 to .876 but loses 39 true matches while
removing 26 false matches. Long recall drops .745 to .659; long precision
still fails .85. Similar trade-offs hold when C is enabled. The tested
strong-confidence rule is not recommended for our recall-oriented baseline.

G flags 301 of 15,699 original answer points, about 1.9%; the conditioned
view also flags 301 points. The penalty is active: baseline group count changes
3,664 to 3,662, and consensus group count 6,272 to 6,241. However it changes
no judged-pair predictions. This null result concerns this fixed lexical proxy
and penalty, not every possible noise-weighting method.

Intervals use 2,000 paired history-bootstrap resamples. They are exploratory,
unadjusted for multiple contrasts and repeated development reuse; repeated
factorial cells are not independent samples. No confirmation or production
generalization claim follows from these point estimates.

## Verification and measured costs

Before treatment scoring, full baseline memberships reproduce every one of
the 1,051 frozen reference predictions and all 160 history confusion matrices.
The final scorer also checks primary-partition diagnostics. There are 3,664
supported baseline groups and 3,383/3,396 covered turns. Maximum group size is
53. Fusion retains separate strong-view groups and group intersections; it
does not merge overlapping groups transitively. Primary-partition counts in
the report remain diagnostics; F/G modify centroid-based memberships.

All selected non-whitespace body coverage and literal source/role/ancestry
checks pass. All 4,251 user points remain bit-identical. C increases total
points 19,950 to 19,992, with answer points 15,699 to 15,741. Both views have
median five vectors/turn, p95 13, maximum 49 on these development histories.
Forced-fallback points rise 71 to 93. Added prompt context reaches 3,350 turns;
236 prefixes truncate at 80 tokens. Supplied/full prompt tokens total
84,337/168,185. Those omissions affect the additional answer context, not the
unchanged user chunks or selected body coverage.

Build takes 367.78s (6m08s), including vocabulary preparation and full-source
preflight, and infers 15,722 new vectors. Per-history construction median/p95
is .472s/10.265s. Existing user vectors reuse the baseline cache, so this is
an incremental representation build, not cold construction of all artifacts.
Build peak RSS is 1,400,664 KiB. Scoring takes 17.95s and peaks at 1,091,448 KiB.
Baseline preparation+fit p95 is .141s; maximum across cells is .198s. These
include measured shared parent costs and membership construction, without
new model inference in scoring. All performance budgets pass; quality does not.

The implementation passes 122 experiment tests plus Ruff/ty checks. Procedures,
results and limitations are recorded in the LaTeX paper. No production indexes,
provider histories, embeddings configuration or baseline settings are changed.

Private artifacts:
`/home/james/scratch/mash-artifacts/turn-three-factor-ablation-20261006`.

- Protocol SHA256: `760104da726f9b3f5f2dcfc1e4b39633a1dff292dd0fea7ac278179281844be9`.
- Development SHA256: `19ae4fd9f2c25b243416ac0a27fd735f3e286d9e5115be68fcb1830b712fd099`.
- Background SHA256: `79d6b10b754233d9b2d6a2c98ae9cf3f8b33039e8665af092ee3881ee74a39d5`.
- Build analysis SHA256: `168a7a829bdd5e9e97632da20c6880ad168edbbd9db8023dd000c3742f4d411d`.
