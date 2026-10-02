# Fixed incremental NMF coding-chat baseline

Freeze before results: K=16/32, one/five passes, canonical forward/reverse arrivals,
128-row batches, random seed718, random initialization, Frobenius loss,
forget_factor1, no regularization/restarts, transform_max_iter100/tol1e-4.
Use installed sklearn1.9.1 MiniBatchNMF.partial_fit; package/module hashes recorded.
Prior art: [official online API](https://scikit-learn.org/stable/modules/generated/sklearn.decomposition.MiniBatchNMF.html)
and [official text-topic example](https://scikit-learn.org/stable/auto_examples/applications/plot_topics_extraction_with_nmf_lda.html).
This is a conventional baseline, not novelty or selection.

Keep exact same frozen eight-thread/68-record exposed reviewed inputs and separate
128-thread/1850-record corpus. Names/review decisions do not enter model fitting
(oracle source admission remains). One vote per distinct exact text per thread;
retain all original evidence identities. Canonical unit order is thread/text/id.
Fit the 8192-feature vocabulary on all unique units, using existing original
source-adjacent terms. Drop >40%-thread-frequency features after three-thread
warmup, retain alphabetic features; frequency rank then lexical tie. Sublinear TF
times thread IDF, row L2 normalize. This vocabulary/IDF is transductive and fixed:
only factor updates are online. Zero lexical rows remain explicitly unassigned.

At final model, transform all original rows. Correct coefficient scale by
multiplying W by component L2 norms; normalize corrected coefficients to unit
mass. Select shares >=.25 AND >=.75best, at most three/source, index ties. No
forced best assignment. Per-thread choose at most eight components using fixed
half maximum-share/half newly covered distinct-input utility, index ties. Retain
original source evidence only for selected thread tags. Topic label candidates
must occur in at least one final assigned source; rank component weights with
existing containment pruning, top eight. Display first term, preserve other
keywords. IDs are model-generation/index scoped, not stable semantic identities.

Report source support and distinct-subject matching, name-hit proxy, unassigned/
multi-source counts, topic/thread counts, referential-only evidence and local
signature redundancy. Compare canonical-order variants via permutation-invariant
dominant-membership ARI, with unassigned as one category; this is not multi-label
or semantic agreement. Record source/vocab/matrix/model hashes, step counts,
warnings, stage/whole Python wall/CPU and external RSS/PSS. Two exact non-timing
repeats; deterministic source/scale/threshold/duplicate fixtures. Preserve failed
logs and original baselines. No output-dependent threshold/model-count tuning.

Unknown topic number/vocabulary growth, order drift, generational identity,
edits/deletion recovery, rare subjects and literal-source semantics are potential
failures, not solved by partial_fit. No new annotation, generative LLM, model
download or paid call. Exposed review is not independent quality gold. No
promotion/Rust integration until independent semantic utility, complete-ingestion
budget and selected executable reference parity pass.
