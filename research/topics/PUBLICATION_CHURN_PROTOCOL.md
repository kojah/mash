# Frozen sustained captured-source churn control

2026-10-03, `mash-c0d.19.2.38.7.3`. Scheduler/lifecycle/cost validation of the
unchanged phrase SOURCE_COVERAGE reference, not an algorithm or quality round.
Reuse the three exact input pins and provenance checks in PUBLICATION_PROTOCOL.
No model calls, label changes, corpus selection, threshold tuning or new arms.

One fixed schedule, one exact repeat on Xenia:

1. Admit all 1850 original sources at 100 events/second in checkpoint order.
2. Continue at 100 events/second for 6000 mutations (60 seconds). For zero-based
   mutation k, select original source `(k // 2) % 1850`; even k deletes it, odd k
   reinserts its unchanged original identity, provider, thread and text. Every
   event must change the ledger. Final state contains all 1850 original sources.
3. Poll every 5ms, one spawned snapshot worker and one pending latest revision.
   Publish completed captured revisions despite continued arrivals; drain to
   revision 7850 and shut down. Deadline 240 seconds from first admission is a
   failure, never permission to duplicate a still-live worker.

For every publication validate captured revision, namespace and complete source
versions against its submitted checkpoint. Keep at most one submitted checkpoint;
diagnostic expected-version/last-event metadata may temporarily include the newly
submitted and just-completed jobs. Final discovery semantics must equal the
existing baseline (arrival/version counters separately audited); final checkpoint
and all non-runtime result fields must match the repeat exactly.

Measure per-event *ledger advancement lag*: time until a published revision first
reaches/passes that event. This is not visibility of every intermediate mutation.
Classify each event at its first advancement as latest-for-source or superseded,
using the captured last mutation for its source. Deletions followed by reinsertion
may be coalesced without ever publishing the transient absence. A latest upsert
must have its exact version present; a latest deletion must be absent. Report the
classification instead of calling all revisions independently visible updates.

Report cold admission separately and six fixed 1000-event churn windows:
admission drift, nearest-rank p95/max advancement lag, peak/end revision backlog,
publication revisions/counts, represented/superseded counts, final drain and
complete elapsed/CPU time. Instrument parent and worker RSS/PSS separately, as
in the previous control; run both under the external parent memory auditor to
include initial JSON load. Sampled maxima are lower bounds, monitoring perturbs
timing, rusage may retain launcher peaks; do not add/subtract process peaks.
Diagnostic event histories and instrumentation are measured research overhead,
not a production queue design. Code/source drift or failed validation rejects
the capture without parameter changes.

This finite 78.5-second captured replay tests bounded scheduled work at this
cohort/capacity and rate. It cannot prove indefinite stability, scaling beyond
2048-source/8MiB limits, source quality, real edits/native ingestion/model/index
write costs, application search isolation, MacBook throughput, durable shared
publication or Rust parity. Snapshots still recompute lexical corpus statistics.
Acceptance/selection and complete-ingestion gates stay open regardless of results.
