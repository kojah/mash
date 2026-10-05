# Selective semantic-gated smoothing: prospective refinement

The user explicitly requested this new refinement after the closed heuristic
screen. It is a new bounded hypothesis, not a reopening of old weight/window
tuning. Freeze the definition before scoring; confirmation remains sealed.

Use the same 160 development histories, immutable whole-context turn vectors,
BIRCH radius .60/cutoff .75 and centroid memberships. Reuse the published
unsmoothed and alpha=.10/window=4 controls from temporal report SHA256
`d4ab356b33f2fbcd4905fc61fcceb2cb1d5793d1735eb1d088fe69ca0fedcb48`.
Input follow-up report SHA256 remains
`d95bb0cb317a40fcd107db4ac6ff4074f66c76a21b87301d22a802f1f2b6bf1e`.

Run exactly one new treatment, with no threshold grid:

- Only target turns with <=32 summed prompt-input tokens and <=128 summed
  own-input tokens in the frozen `lean_separate` inventory are eligible.
  These counts include each input's special tokens and overlap, making the
  length check conservative. The own-token cap prevents a brief prompt with
  a substantial selected answer from being treated as a short turn.
- Neighbours are prompted turns within four positions, restricted to the
  existing resolved ancestor/descendant relation; never siblings or self.
  Neighbours need not themselves be short, and their vectors remain unchanged
  during this single simultaneous pass.
- Weight neighbour j by `exp(-abs(i-j)/2) * max(0, cosine(x_i,x_j)-.55)`.
  Normalize the weighted neighbour mean and blend alpha=.10 into the original
  vector; normalize the result. No positive weights or mean norm <=1e-12 means
  retain the original vector. Ineligible targets are bit-identical.
- No new embeddings, inference, source cleaning, answer restoration or changes
  to BIRCH/membership thresholds. Validate cached vector and lean-input hashes
  against the frozen parent, including source/role/ancestry provenance.

Report overall and long-history P/R/F1, fixed nearby (<=4) unrelated FPR and
distant (>4) related recall, eligibility/movement counts, provider/node/length/
sampling slices and paired thread-bootstrap gain intervals against both
controls. Control timing is historical; measure actual new input preparation,
smoothing and fit plus external peak RSS. Do not claim a composite with the
chunk-view ensemble has been tested in this experiment.

Acceptance remains overall P>=.85/R>=.70/F1>=.75, F1 gain>=.10 over
chronological-four, and the same absolute floors for long histories. Also
require preparation+fit p95<=10s and measured peak RSS<=4GiB. A useful trade-off
is not an accepted algorithm if these gates fail. No retuning length caps,
similarity floor, exponential scale, alpha or window after seeing scores.
Only an eligible final choice could advance to the still-unopened confirmation
set, once, after full contract validation.
