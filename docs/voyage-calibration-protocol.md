# Voyage calibration and representation audit

Registered before the new API calls or calibration. The frozen
`complete-answer-v1` baseline remains unchanged. Production search is untouched.

## Questions and arms

Audit API response indices, dimensions, finiteness, normalization, text hashes,
pooling, point ownership and cached-control parity. The first Voyage study stored
only decoded int8 vectors, so raw-float comparison requires fresh calls.

Compare MiniLM's frozen int8 representation with Voyage Code 4 and Voyage 4,
each using (a) original short windows as floats, (b) those same vectors passed
through the original int8 codec, and (c) complete-turn floats. For every arm,
report fixed pooled-only, chunk-only and union results. Keep original smoothing
alpha 0.10/window 4; chunk vectors are unsmoothed. Raw floats are retained.

Complete-turn inputs contain the current lean user prompts followed by every
visible nonempty assistant message in that turn, in order, with role markers.
Do not add other threads or LLM summaries. Inputs fitting a conservative 28,000
UTF-8-byte budget are embedded once. Longer inputs are split at paragraph/line
boundaries, with a UTF-8-safe fallback for oversized lines; no source characters
are dropped. Their normalized mean is the pooled view and individual pieces
are the chunk view. This representation change is explicit: it replaces external
equal-role window pooling for this experimental arm only. Complete-turn inputs
are independently screened for credential-like patterns; flagged histories are
excluded from all arms before choosing the split.

## Calibration and evaluation

Use only previously inspected development histories, starting with the prior
156 matched histories (indices 71, 73, 74, 110 already excluded). This is a new
thread-separated development holdout, not a pristine confirmation set. Existing
agent-authored labels remain provisional. Keep the original confirmation sealed.

Sort eligible histories by SHA-256 of `voyage-calibration-v1:<thread index>`;
the first two thirds are calibration and the remainder evaluation. Freeze IDs
and input/source/code hashes before inference. No labels determine the split.

Each representation gets exactly 48 Optuna random-sampler trials (seed 20261006),
with the same parameter choices/draws: BIRCH radius {0.40,0.50,0.60,0.70},
membership cosine {0.55,0.65,0.70,0.75,0.80,0.85}, relative-best margin
{0.00,0.05,0.10,1.00}, view {pooled,chunk,union}. Enqueue the original
radius/cutoff/margin/union first. Optimize calibration F1 only; choose the earliest
trial on ties. Evaluate that selected configuration once on evaluation histories.
No winner selection or retuning based on evaluation results.

Keep primary groups supported by at least two distinct turns, derive centroids
from primary members, and retain memberships passing both absolute and
relative-best gates. Namespace union views and require at least two distinct
member turns in each surfaced group. All fitting stays within a thread.

Report precision/recall/F1, confusion counts, long-thread results (>=8 turns),
distant related-turn recall (gap>=4 in long threads), and paired 2,000-resample
thread-bootstrap F1 differences against the frozen matched MiniLM control.
Intervals are exploratory and unadjusted for multiple arms. Record trials and
aggregates in Optuna/MLflow, excluding chat text and per-thread labels.

## Execution and cost

Use `voyageai/voyage-code-4` and `voyageai/voyage-4` through OpenRouter,
1024 float dimensions, default task, no fallback, data collection denied.
Validate response model and indices; cache raw vectors by model and exact text
hash in a private SQLite database. Batch <=64 texts/100,000 UTF-8 bytes with
two concurrent requests and up to three transient retries. Limit estimated plus
reported new API charges to $3. Do not log credentials or raw HTTP bodies.
Record provider receipts, successful-call charges and unknown transport charges.
Provider model weights cannot be pinned; retain response model and request IDs.

Prior short-window MiniLM uses contextual target-token pooling where specified;
the remote API returns whole-input vectors. This unavoidable difference must be
reported, not described as identical pooling. Repeated calls may expose provider
drift; quantify parity against the earlier cached Code 4 vectors.
