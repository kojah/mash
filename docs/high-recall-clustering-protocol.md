# High-recall turn clustering calibration

Target, specified before this run: precision >82%, overall recall >90%, and distant-turn recall >80%, simultaneously. These are point-estimate targets, not lower confidence bounds.

Use cached Voyage Code 4, 2048-dimensional, model-token-budget joint-turn matrices from the model-capacity experiment. Preserve source, smoothing (alpha .10, window 4, branch-aware), annotations, and the 104/52 calibration/development split. No API inference, new annotation, or confirmation access.

Run 256 seeded Optuna TPE calibration trials. Search BIRCH radii .40/.45/.50/.55/.60/.65/.70, absolute membership cosine .55–.85 in .0125 increments, relative-best margins 0/.025/.05/.10/.20/.30/.50/1, and pooled/chunk/union views. Enqueue the previously selected high-recall setting first. Keep groups supported by at least two distinct turns.

Optimize the minimum ratio to the three targets until all are exceeded; among feasible settings optimize precision. Freeze the highest-precision feasible calibration setting (F1 breaks ties); if none is feasible, freeze the best minimum target ratio. Only then score that one candidate on the separate development subset. Report calibration and evaluation counts, distant denominators, runtime, source/matrix/annotation hashes, and exploratory paired uncertainty versus the frozen Voyage 4 reference.

This is reused development, with blind agent-authored labels. The original confirmation remains sealed. Passing these targets is provisional evidence, not production acceptance or an independently confirmed result. If this finite search fails, report the shortfall before changing the algorithm family or opening another search.

Use Optuna and local MLflow; record aggregate metrics only in MLflow. Keep matrices, predictions and annotations private. No production or frozen-baseline changes.
