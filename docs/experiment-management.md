# Experiment management

Use Optuna for new experiment trials, following the user's decision. Existing
frozen historical reports remain unchanged. `experiment_study.py` provides a
single-process, bounded ledger for the upcoming context/boilerplate comparisons.
Declare every setting before scoring; enqueue those exact settings rather than
allowing an adaptive sampler to search beyond the protocol. Record protocol,
cohort and encoder identities in the persisted study contract. Completed trials
resume without recomputation; changed identities fail instead of mixing results.
An interrupted running trial requires explicit recovery. Failed trials can be
retried by an explicit rerun. Keep the SQLite study ledger private outside Git.

Report development F1 as the objective while retaining precision, recall,
long-history results, performance and acceptance gates in the experiment's typed
report. Optuna's best trial does not establish acceptance or authorize opening
confirmation. Blind-label validation, source provenance, thread-isolation tests
and the paper remain part of the experimental procedure.
