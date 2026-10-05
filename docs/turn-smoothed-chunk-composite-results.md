# Original smoothing + structural chunks: completed

The user requested this exact combination after selecting original uniform
smoothing. The [prospective protocol](turn-smoothed-chunk-composite.md) pinned
both parent reports, their radius .60/cutoff .75 and three cells. We scored the
same 160 development histories from frozen predictions. No model inference,
new cutoff, altered chunk construction or confirmation evaluation occurred.

**Decision: recall improves, but precision still prevents acceptance.** The
composite clears the long-history recall and F1 floors; long precision is .824.

| Method | P | R | F1 | Long P | Long R | Long F1 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Original smoothed pooled view | .851 | .807 | .829 | .833 | .653 | .732 |
| Heading-aware chunk view | .917 | .611 | .734 | .901 | .490 | .635 |
| Namespaced union | .847 | .862 | .855 | .824 | .745 | .783 |

Union has TP=532, FP=96, FN=85 overall and TP=234, FP=50, FN=80 long.
Compared with the previously published unsmoothed pooled/chunk union,
long recall rises .666 to .745 and F1 .746 to .783; precision falls .850 to
.824. This is a useful recall/precision trade-off, not a passing method.
Overall precision .847 also narrowly fails .85. No gates are relaxed.

Nearby-related recall is 460/482=.954 and nearby unrelated FPR is 64/100=.640.
The smoothed pooled parent alone gives 431/482=.894 and 59/100=.590.
Distant-related recall rises 67/135=.496 to 72/135=.533 when adding chunks.
Relative to the unsmoothed pooled/chunk union, nearby recall rises .919 to .954,
nearby FPR .500 to .640, and distant recall .378 to .533. Changes to fitted
groups can affect distant links even though smoothing uses a bounded window;
these results do not establish direct attraction between distant turn vectors.

Union's paired thread-bootstrap F1 interval is .822–.884, with gain over the
chronological-four baseline .139–.225. These are exploratory intervals with
reused development judgments. Both parents reproduce their reference counts.
The shared scorer refactor also reproduces all 800 rows, predictions and gates
from the original four-cell ensemble report, excluding timing fluctuations.
The experiment test suite passes all 117 tests.

Scoring takes 5.06s with externally measured peak RSS 1,031,168 KiB. Historical
summed parent preparation/fit p95 is .136s/history; this is not an actual
end-to-end composite build benchmark. Full combined memberships, group counts
and coverage were not recomputed because quality gates fail. The membership
union preserves view namespaces and does not merge overlapping groups
transitively. Confirmation remains sealed.

For the current chunk construction, current-turn prompts and the selected
answer have separate chunks. Previous-turn context is repeated on user chunks;
answer chunks receive bounded section headings, not current-turn prompt text.
Changing that representation would require its own prospective comparison.

Private result:
`/home/james/scratch/mash-artifacts/turn-smoothed-chunk-composite-20261006/development.json`.

- Protocol SHA256: `9cf602650b75507f718d7d7abebbdf6798f926bdc490e8dbd7b566707d5402fe`.
- Development SHA256: `9835b7cd6549b07ebb4e3c55ee4190ac7294854d5a9d6dacd968b08ddfb083ce`.

Keep original smoothing as the user's preferred proximity component. Do not
retune this composite from its results or claim production acceptance.
