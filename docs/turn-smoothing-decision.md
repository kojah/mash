# Preferred proximity component

The user selected the original uniform smoothing after the selective refinement
failed to preserve its nearby-recall improvement. Keep alpha=.10 and window=4,
using the original distance weights `5 - abs(i-j)` and resolved ancestor/
descendant restriction. This is the linear-weight treatment already evaluated,
not the selective refinement's exponential weighting or semantic gate.

Use original vectors in one simultaneous pass; exclude self and sibling
branches, normalize the neighbour mean, and retain the original vector when
there is no valid mean. Recompute the visible thread when its data changes.
Do not tune new smoothing parameters. Retire the selective shortness/gating
variant described in [its results](turn-selective-smoothing-results.md).

The chosen component increases nearby-related recall from .836 to .894, with
nearby unrelated false positives increasing from 42 to 59. Overall P/R/F1 is
.851/.807/.829 and long-history P/R/F1 is .833/.653/.732. See the
[original temporal results](turn-temporal-smoothing-results.md).

This is a user-selected research/component decision, not evidence that the
complete algorithm passes the registered quality gates. In particular, a
composite of this smoothed view and the structural chunk view has not yet been
evaluated. Preserve the existing acceptance criteria and sealed confirmation;
register and evaluate that exact composite before claiming its performance.
