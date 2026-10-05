# Selective semantic-gated smoothing: completed

The [registered refinement](turn-selective-smoothing.md) tested one fixed
semantic gate and shortness rule on the same 160 development histories. The
user authorized this new hypothesis after the earlier heuristic screen closed.
No parameter sweep, new embeddings or confirmation evaluation occurred.

**Decision: no improvement and no eligible configuration.** Selectivity avoids
most of uniform smoothing's additional false positives, but also loses almost
all its recall improvement.

| Treatment | P | R | F1 | Long P | Long R | Long F1 | Nearby unrelated FPR | Distant related recall |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Unsmoothed | .887 | .724 | .798 | .855 | .564 | .679 | .420 | .326 |
| Uniform alpha=.10 / window=4 | .851 | .807 | .829 | .833 | .653 | .732 | .590 | .496 |
| Selective semantic gate | .880 | .726 | .796 | .852 | .567 | .681 | .450 | .348 |

The refined treatment has TP=448, FP=61, FN=169 overall, versus 447/57/170
without smoothing. Long-history counts are 178/31/136 versus 177/30/137.
Nearby unrelated false positives rise from 42 to 45 (100 negative pairs),
while distant related matches rise from 44 to 47 (135 positive pairs).
For the user's intended local mechanism, nearby-related recall is 401/482=.832,
versus 403/482=.836 unsmoothed and 431/482=.894 with uniform smoothing.
Thus the uniform method did recover nearby links, at a false-positive cost;
this fixed refinement does not preserve that local benefit. Lack of a large
long-history gain alone would not invalidate a proximity-based mechanism,
because related turns in long histories can be far apart. The long-history
gate still applies to accepting a complete clustering algorithm.
Paired thread-bootstrap F1-gain intervals are -.016–.014 versus unsmoothed
and -.063–-.005 versus uniform smoothing. These are exploratory intervals on
reused development data; no stronger causal or generalization claim follows.

Of 3,396 development turns, 615 satisfy both the prompt-token cap of 32 and
own-token cap of 128; 598 actually move after the semantic/ancestry gates.
For long histories, 550 of 3,057 turns qualify and 539 move. For short histories,
65 of 339 qualify and 59 move. The treatment therefore affects about 18% of
turns. A short prompt with a substantial selected answer deliberately does not
qualify. We will not retune these length caps or the .55 similarity floor using
this result. Limited targeting is a plausible explanation for the small impact,
not proof that loosening the caps would help.

Cached matrix and lean-input hashes match the frozen parent. Literal source,
role and ancestry checks pass. Refitting the unsmoothed anchor reproduces its
reference predictions. Contract tests cover both length caps, long targets,
unrelated neighbours, sibling isolation and invalid target identities.

Scoring completes in 7.76s with externally measured peak RSS 1,057,324 KiB.
Actual new preparation+smoothing+fit p95 is 23.60ms/history. Control times
remain historical; this is not new embedding construction. No composite with
the structural chunk-view ensemble was evaluated. Confirmation remains sealed.

Private result:
`/home/james/scratch/mash-artifacts/turn-selective-smoothing-20261005/development.json`.

- Protocol SHA256: `2ac00207795fc987de54e4e9e0502dfe847e1d26523b0ebef2ed298c2c5ec19f`.
- Development SHA256: `daee901beef0448984386ff6f1bc0adcbd8cb6d4f72e8671ae53f4614fa4c914`.

Retire this fixed refinement. Improving long-thread recall remains unresolved;
the independently registered local affinity diagnostic is still in progress.
