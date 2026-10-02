# Online average-link source-block control

Freeze before outcomes, 2026-10-02. FISHDBC lexical return .5806/.5799 loses to
offline lexical average .6222, even with exact neighbors. Test whether average
linkage can retain that evidence online, rather than change compression or density
thresholds. This is a bounded exploratory control, not production selection.

Keep immutable 162 QMSum training meetings/8,196 source-only blocks, original
18,316dim sparse sublinearTF-IDF and unchanged384dim dense directions. Include
unknown/multi sources in geometry, original singleton-known source-weighted
all/far/crossspan LCA scopes and same-block exclusion. Full training IDF and
per-meeting frames are offline; no future-dependent projection, new inference,
model download/LLM, parameter/cut grid or pristine heldout.

Reuse the existing exact-neighbor rotate/graft/restructure implementation in
online_tree.py without editing historical pinned helpers. A sparse adapter
implements CSR row normalization, subtree sums and UNNORMALIZED means, sparse
dot products/exact nearest lookup, insertion storage and aggregate validation.
Mean(A).mean(B) equals mean pairwise cosine of original unit leaf vectors;
normalizing internal means would silently restore centroid linkage and is banned.
Concrete scientific adapter inheritance reuses all unchanged tree operations;
this is not a provider abstraction or application interface.

Three controls: greedy, rotate, GRINCH-style graft; two fixed views and both
original forward/reverse arrival orders. Max2000leaves/20000lexicaldimensions;
no ANN/operation caps, strict comparisons and existing stable arrival-order ties.
Dense controls use unchanged Tree(Linkage.AVERAGE). Sparse adapter must match
dense original prototype clades/identities/statistics/aggregates/scores exactly
on non-boundary fixtures and numerical tolerance1e-12; real near-ties remain
reported, not silently rounded. No assertion that this is author GRINCH.

Capture complete final source trees, every leaf mapping and identity, root sums,
linkage/state fingerprints and operation counts; validate after each insertion
on fixtures and final every corpus case. Audit subtree means against original
unit vectors, direct pair-average scores and original-source LCAs. Fresh whole
repeat must match every non-timing field. Source/runtime/helper/protocol pins
and full original support required.

Separate feature prep, insertion wall/CPU/p95, validation, snapshot/evaluation
timers and externally sampled RSS/PSS/rusage. Additional numeric tree storage is
distinct from full process peak and complete persistent state; disclose excluded
Python metadata/lists/temporaries/features. No complete-ingestion speed claim from
bounded per-meeting prototype or unbounded exact-neighbor grafting. No intentional
scientific benchmark overlap, uncontrolled host load, observer/children excluded,
intrusive sampled lower bounds/status approximate/launcher high water possible,
no subtraction.

Necessary usefulness target: recurring-topic loss<=.02 vs lexical average in
BOTH orders; report all/far/root/chronological baselines and order discrepancies.
No goal redefinition from geometric trees: independently assessed coding labels,
multi-tags/semantic ancestors/guided search, globally stable node-group/provider
feature frame, bounded lifecycle/persistence/edits/deletes/restart, full ingestion
<=10percent loss and selected Python/Rust differential CI remain gates. A useful
unbounded control only motivates a separately frozen bounded implementation.
