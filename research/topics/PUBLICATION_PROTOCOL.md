# Frozen phrase publication control

2026-10-03, `mash-c0d.19.2.38.7.2`. This is a predefined online/publication
check, not a topic algorithm round or semantic evaluation. Keep the existing
`SOURCE_COVERAGE` discovery, source validation, lexical scores and identities
unchanged. No ensemble cutoff tuning, new inference or annotations.

One mutable bounded `PhraseStore`, one spawned snapshot process, and one pending
latest-ledger revision. Do not retain per-arrival jobs or checkpoint copies while
the worker is busy. When a snapshot finishes, publish its captured revision and
submit the latest complete checkpoint if dirty. Continuous arrivals must not
starve completed publications. Failure preserves the previous publication and
requires explicit retry; final completion includes draining the latest revision.
This is a research scheduler, not a database publication or production API.

Use the frozen 128-thread/1850-source input in recorded checkpoint order at
**100 source arrivals/second**, with a 5ms polling interval. Deadlines start at
the first source; record actual admission drift rather than assuming exact rate.
Reuse original texts and namespaces. Source/provenance checks must precede work.
No vectors or embedding generation are needed by the phrase arm.

Frozen inputs:

- `topic-128-thread-first-20261002/checkpoint.json`:
  `3ab25cafacab7ef1c6c0682189740aa23c620e4eab994db36c6d36f5e0d6d1f2`.
- Its `results.json` provenance capture:
  `8d06df371de287a4e12ab92376b276af111a63c7ffaeb58f447377afc51005aa`.
- `phrase-allocation-stream-first-20261002.json` baseline:
  `e1e425b2d7f635df16e07f8f840e44f3da4f108899d4de233095325901a246c5`.

Compare the drained final discovery semantics to the prior exact baseline;
arrival revision/version counters are tested separately. Repeat the same control
once with exact final checkpoint/snapshot agreement. Real scheduling determines
intermediate publication revisions, so timing and intermediate revision sequences
must be disclosed, not required to match or misrepresented as deterministic.
Fixtures separately check coalescing, idempotence, edits/deletes/new subjects,
monotonic revisions, failed jobs and explicit retry using controlled futures.

Report actual admission/end-to-drain time, worker wall/CPU totals, publication
count and revisions, update publication lag/p95, source-revision backlog and
admission drift. Instrument parent and worker process scopes separately with
Linux `memory_audit.sample`; sampled RSS/PSS maxima are lower bounds, kernel
high-water counters are approximate, and sampling perturbs the run. Never add
noncontemporaneous process peaks or call one process the complete job. Keep
complete-workload wall/CPU and rusage too, with their scope identified.

This finite captured replay can verify bounded queued jobs and final correctness;
it cannot prove indefinitely sustained throughput, independent semantic utility,
native parsing/embedding/index write cost, search isolation in the application,
multi-node behavior, persisted shared publication or production selection. The
reference still recomputes corpus lexical statistics for snapshots. Actual
complete-ingestion and selection gates remain open regardless of this result.
