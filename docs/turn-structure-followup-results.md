# Structure-aware chunks: completed

The [registered protocol](turn-structure-followup.md) evaluated four structural
cells against two unchanged token-window controls. All 200 histories were built;
only the same 160 development histories were scored. Model, labels, source
selection and quality thresholds remained fixed. Confirmation stays sealed.

**Decision: no eligible configuration.** Structure improves precision at the
broad cutoff but loses recall; none satisfies every overall and long-history gate.

| Inputs / cutoff | P | R | F1 | Long P | Long R | Long F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Token windows / .65 | .803 | .870 | .835 | .743 | .790 | .765 |
| Token windows / .75 | .883 | .639 | .741 | .843 | .497 | .625 |
| Plain structure / .65 | .828 | .820 | .824 | .794 | .723 | .757 |
| Plain structure / .75 | .912 | .608 | .730 | .884 | .484 | .625 |
| Heading structure / .65 | .832 | .825 | .828 | .801 | .732 | .765 |
| Heading structure / .75 | .917 | .611 | .734 | .901 | .490 | .635 |

Heading-aware structure at .65 has TP=509, FP=103, FN=108 overall and
TP=230, FP=57, FN=84 on long histories. Its precision improvement does not
meet the .85 floor. Its paired thread-bootstrap F1-gain interval versus the
matching window control is -.031–.017; plain structure gives -.034–.012.
These exploratory intervals include no gain. Nearby unrelated FPR is .600
and distant related recall .600 for heading/.65; stricter heading/.75 reduces
nearby FPR to .280 but distant recall falls to .207. No stronger coherence
claim is justified simply because boundaries look more structured.

## How many vectors?

The full 200-history cohort contains 5,082 prompted turns. Each structural
variant has **median five vectors per turn**, p95 14. Plain structure produces
31,657 vectors and heading context 31,661; the maximum is 148/150 in exceptional
large turns. This is about 2.5 times the original 12,627 context-window points.
It is not automatically one vector per paragraph, but the multiplicity increase
is substantial and must be considered before deployment.

Input-token medians are 98/99, and p95 203/205. They include ancestor and heading
context, so they do not establish body-only lengths. Inputs under 16 tokens
number 1,291/1,172 (4.1%/3.7%). Plain structure has 28,890 prose, 2,721 code and
46 standalone-heading points; heading context has 28,892/2,723/46. Only 79/81
points require the oversized-atom token fallback. All input budgets, source
selection and literal ancestry checks pass. Each vector remains 384 dimensions.

## Interrupted build and costs

The first build stopped at history 85 because the coverage guard compared
against old token-offset spans, which omitted a leading U+E200 citation marker.
The structural input preserved the marker. No content was lost. The guard was
corrected to check the agreed selected source text directly, retain old covered
characters and reject excluded answers. Boundary rules and thresholds did not
change. A regression test and a full no-inference coverage preflight passed
both treatments on all 200 histories before restarting.

The failed run is retained under `artifacts` and took 445.24 seconds. A fresh
`artifacts-restart1` directory reused its durable vector cache; that successful
build took 181.78 seconds. Total build-attempt wall time is 627.02 seconds,
excluding the separately performed preflight. The first run published 170
artifacts with 23,084 newly inferred vectors. Restart inferred another 6,123
plain and 975 heading vectors; it must not be called a cold build.

Restart construction p95 is 2.763s/0.937s, assisted by the earlier cache and
sequential reuse. Scoring took 18.54s; maximum preparation+fit p95 was 118.33ms.
Externally measured successful-build/scoring peak RSS is 1,463,852/1,039,756 KiB.
The failed run peaked at 1,392,188 KiB. All performance budgets pass, but quality
does not. Production message embeddings and silver remain unchanged.

Private artifacts: `/home/james/scratch/mash-artifacts/turn-structure-followup-20261005`.

- Protocol SHA256: `b9a53097c17110256831f4de6715d1939d5d524daa687779d9e18e9c96c33c9f`.
- Development SHA256: `50d7ef826e4d21196dc4e3cef5c33d59a5f2884561c26cb2e2ec723d3ca28555`.
- Analysis SHA256: `b84c7ff03af7680632d2729b515875e214a49fcd281973c3d2df6a890fb097ab`.

No structural algorithm is accepted or promoted, and no parameters will be
retuned from this result. The remaining gap is joint precision and recall on
long histories, not parsing correctness or construction speed.
