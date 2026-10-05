# User-requested smoothed pooled + structural chunk composite

Freeze this exact combination before computing its predictions. The user
selected original uniform smoothing and explicitly requested combining it with
the chunk view and fixed structured clustering cutoffs. This authorizes one new
composite, not another weight/window/cutoff sweep. Confirmation remains sealed.

Parent A is the published whole-context pooled turn view with original uniform
smoothing alpha=.10/window=4 (linear distance weights and branch restriction),
BIRCH radius .60/cutoff .75 and centroid memberships. Pin temporal report SHA256
`d4ab356b33f2fbcd4905fc61fcceb2cb1d5793d1735eb1d088fe69ca0fedcb48`;
row name `temporal:0.10:w4`.

Parent B is the published heading-aware structural chunk view, with BIRCH radius
.60/cutoff .75, centroid memberships and turn-level union of chunk memberships.
Pin structural report SHA256
`50d7ef826e4d21196dc4e3cef5c33d59a5f2884561c26cb2e2ec723d3ca28555`;
row name `structure_heading:birch:0.75`.

Evaluate exactly three cells on the existing 160 development histories:
unchanged A, unchanged B, and their namespaced membership union. No transitive
group merging, intersection, parameter tuning, new inference or label changes.
The cutoffs are the existing fixed clustering/membership thresholds; structural
chunk boundaries themselves remain unchanged. Reproduce parent pair counts.

Use the existing pair-operator scorer with parent/cohort/model/annotation hashes
validated. Report overall/long P/R/F1, nearby related recall and unrelated FPR,
distant related recall, all existing provider/node/length/sampling slices and
paired thread-bootstrap uncertainty versus chronological-four. Historical
summed parent costs are estimates, not measured composite construction costs.
Any quality-eligible composite must be fully recomputed from cached vectors
with exact membership/pair parity and measured resource budgets before it could
advance to confirmation.

Preserve overall P>=.85/R>=.70/F1>=.75 and F1 gain>=.10 over chronological-four,
plus the same absolute quality floors on long histories. Require actual
preparation+fit p95<=10s and peak RSS<=4GiB for promotion. At most one final
eligible method may enter the unopened confirmation set, once. A failed
composite is not promoted or retuned; the selected smoothing component can
remain the user's preference without asserting full-method acceptance.
