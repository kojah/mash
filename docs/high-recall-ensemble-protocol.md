# Convex embedding ensemble follow-up

The 256-trial Code 4 settings search did not meet the simultaneous targets. Its frozen candidate scored 77.83% precision, 89.12% recall and 77.78% distant recall on the reused development evaluation subset. Calibration precision was 81.47%, below the >82% requirement.

Test a genuine clustering representation ensemble, without pair-level filtering: concatenate sqrt(w) times normalized Code 4 2048-dimensional embeddings and sqrt(1-w) times normalized Voyage 4 1024-dimensional embeddings. Its cosine is the convex combination of model cosines. Unit ownership must match exactly. Preserve joint-turn source, token budgets, structural cutoffs, .10/window-4 branch-aware pooled smoothing, and distinct-turn support.

Before running, register 512 Optuna TPE trials with seed 20261007, weights 0/.25/.50/.75/1, and the same radius/cutoff/margin/view search and simultaneous strict targets as the [first sweep](high-recall-clustering-protocol.md). Enqueue the prior Code high-recall setting. Select by calibration precision among feasible candidates, otherwise minimum target ratio, with F1 and trial number as tie breaks. Freeze one selection before evaluation.

Use the same 104/52 development split, cached vectors only, local MLflow aggregate logging and persisted Optuna trials. Confirmation stays sealed. This is an adaptive development follow-up, not independent validation. Do not sweep evaluation outcomes or promote a new baseline merely because the point estimates pass. Report the result and limitations in the paper.
