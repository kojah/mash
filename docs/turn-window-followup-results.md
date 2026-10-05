# Retained turn windows: completed

The [prospective protocol](turn-window-followup.md) specified eight window
cells and one unchanged pooled-context anchor. All 160 development histories
were scored from read-only caches, with unchanged models and labels. Source,
role and turn provenance checks passed; the anchor reproduced its earlier
per-history predictions. No inference or confirmation scoring occurred.

**Decision: no eligible configuration.** Separate window vectors improve some
development results, but the precision/recall trade-off still fails the gates.

| Representation / family / cut | P | R | F1 | Long P | Long R | Long F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Pooled anchor / BIRCH / .75 | .887 | .724 | .798 | .855 | .564 | .679 |
| Context windows / BIRCH / .65 | .803 | .870 | .835 | .743 | .790 | .765 |
| Context windows / BIRCH / .75 | .883 | .639 | .741 | .843 | .497 | .625 |
| Context windows / average-link / .65 | .916 | .598 | .724 | .865 | .490 | .626 |
| Context windows / average-link / .75 | .946 | .340 | .501 | .933 | .268 | .416 |
| Full answers / BIRCH / .65 | .781 | .874 | .825 | .692 | .787 | .736 |
| Full answers / BIRCH / .75 | .888 | .684 | .773 | .838 | .561 | .672 |
| Full answers / average-link / .65 | .897 | .661 | .761 | .830 | .529 | .646 |
| Full answers / average-link / .75 | .925 | .361 | .520 | .895 | .299 | .449 |

The best aggregate-F1 cell, context windows/BIRCH/.65, has TP=537, FP=132,
FN=80 overall and TP=248, FP=86, FN=66 on long histories. It has nearby
unrelated FPR .600 and distant related recall .719 (the same <=4/>4 definitions
as the preceding temporal study). This recovers many relationships but is not
accurate enough to promote.

At the **matching .65 BIRCH setting**, the prior pooled whole-context result
was P=.768, R=.870, F1=.816, versus .803/.870/.835 for separate windows.
Long-history precision rises .694→.743 and recall .780→.790. The paired
thread-bootstrap F1-gain interval is -.005–.045: point estimates improve, but
uncertainty includes no gain and this is reused development data. Comparing
only against the .75 anchor would conflate representation and threshold effects.

Restoring intermediate answer segments does not consistently help. At .75
BIRCH it raises recall .639→.684, but at .65 it lowers precision .803→.781
with little recall change. Average-link offers higher precision with inadequate
recall. These are observed operating-point differences, not evidence that one
family is universally better.

Context windows supply 8,231 points for 3,396 development turns; full answers
supply 9,534. Maximum points per turn are 18 and 54 respectively. The existing
distinct-turn support rule prevents a turn's repeated windows from retaining
a cluster by themselves. Window multiplicity can still influence fit; no
unregistered reweighting was introduced.

Complete scoring takes 13.88 seconds, with measured peak RSS 1,068,432 KiB.
Maximum per-history cache preparation plus fit p95 is 64.68 ms. Performance
gates pass. These are cache-only costs, not fresh embedding construction.

Private artifacts: `/home/james/scratch/mash-artifacts/turn-window-followup-20261005`.

- Protocol SHA256: `3e981e55e9c6dc7cccaf6ae784c1b929374465e6ac51f7570d69cc8eaf09dce5`.
- Development SHA256: `126544f75c82034e0c400863311c918d00096a95878357721993d3322f404eba`.

The user-approved next comparison is source-preserving, structure-aware
segmentation at message, heading, paragraph and code boundaries. It remains a
separate prospective experiment; parameters and effects will not be silently
folded into this completed screen. Confirmation remains sealed.
