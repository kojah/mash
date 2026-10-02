# Bounded phrase publication during captured arrivals

`mash-c0d.19.2.38.7.2`. The [protocol](../research/topics/PUBLICATION_PROTOCOL.md)
was committed as `c45e4d7` before implementation or execution. This predefined
publication check changes no topic algorithm, labels, cutoff or source policy.
It uses the same exposed 128-thread/1850-source capture, existing phrase baseline
and `SOURCE_COVERAGE` reference. Original text hashes and all three frozen input
hashes are verified before execution and rechecked at completion. No new
inference, LLM, paid call or semantic annotations.

## Procedure

One parent-owned `PhraseStore`, one spawned process computing a snapshot, and
one coalesced pending revision. Arrivals mutate the ledger; they do not enqueue
per-source jobs or copy pending checkpoints. On completion publish the captured
historical revision, then submit the latest checkpoint if dirty. The snapshot
is self-consistent; it is not presented as the newest admitted data. This avoids
waiting for every arrival to finish before any publication becomes visible.
Errors retain the last published snapshot, stop automatic submissions and
require explicit retry. Completion drains the final revision.

Admit the frozen checkpoint order at 100 sources/second, polling every 5ms.
The ledger contains 1,527,917 text bytes, within the reference's existing
2048-source/8MiB limits. Snapshots still recompute corpus lexical statistics.
Run once and repeat the same fixed control, with exact final discovery/checkpoint
agreement. Real timing changes intermediate captured revisions; do not require
their sequences to match. CPython3.11.15, NumPy2.4.6/SciPy1.17.1/sklearn1.9.1,
one OpenBLAS/OMP/MKL numerical worker, Xenia. No other owned benchmark ran
concurrently; external host load and power state were uncontrolled.

## Observations

| Measurement | First | Repeat |
| --- | ---: | ---: |
| Admission duration, seconds | 18.497 | 18.491 |
| Final drain after arrivals, seconds | 4.781 | 4.544 |
| Complete control, seconds | 25.163 | 25.006 |
| Parent CPU, seconds | 7.323 | 7.206 |
| Exited child CPU, seconds | 23.039 | 22.579 |
| Source publication lag p95, seconds | 5.887 | 6.005 |
| Maximum source publication lag, seconds | 6.437 | 6.900 |
| Admission deadline drift p95 / max, milliseconds | 8.785 / 32.448 | 8.619 / 32.773 |
| Maximum unreflected source revisions | 607 | 531 |
| Snapshots published | 11 | 11 |
| In-flight jobs / pending revision markers, maximum | 1 / 1 | 1 / 1 |

The first snapshot contains two phrases at revision 1, visible after 1.277/1.197
seconds while 128/120 sources have already arrived. This establishes publication
before complete admission, not useful semantic quality of those phrases. Final
revision 1850 contains the same 442 topics and all source memberships as the
unchanged baseline. Every source revision is accounted for in the publication
lag calculation; p95 uses the nearest-rank `ceil(.95*n)` definition.
Total control includes frozen JSON loading/validation, process startup, arrivals,
all publications, final draining/shutdown, source rechecks and memory sampling,
before output serialization. Source loading takes 1.507/1.540 seconds. Parent CPU
and exited-child CPU are separate process scopes; the latter includes worker
startup/shutdown. No time is credited merely for queuing work.

Publication revisions:

- First: 1,128,196,284,432,612,820,1076,1379,1684,1850.
- Repeat: 1,120,179,254,412,601,835,1081,1345,1612,1850.

Exact repeated non-runtime fields include final snapshot, checkpoint, code/input
pins and checks. Timing and intermediate sequences belong to runtime evidence.
The queue bound does **not** bound freshness to one source: many source revisions
can await the next snapshot. This finite 18.5-second arrival burst does not prove
indefinitely sustainable throughput, larger-corpus behavior or bounded lag at
every arrival rate. The store's capacity limit must not be mistaken for a
production streaming design.

## Memory scope and limitations

During streaming, procfs sample maxima for the parent are RSS195480/194144KiB
and PSS184493/179861KiB; worker RSS163196/163232KiB and PSS139282/138630KiB.
The parent samples its own counters and externally samples the worker at 50ms
intervals after the first worker result supplies its PID. Early worker startup
is not externally sampled. Sampled maxima are lower bounds; simultaneous process
peaks are not inferred or added.

The repeat additionally runs under the independent `memory_audit.py` observer,
977 samples at its default 20ms interval. Its parent-only sampled RSS 399288KiB,
PSS 391766KiB and approximate VmHWM 403992KiB include the earlier full frozen JSON
load. It excludes the worker and observer itself. This extra observer is a
measurement difference between controls and may perturb runtime; two timings
are not a latency distribution or controlled speed comparison.

Parent rusage peaks 404900/403992KiB include raw capture loading, not a minimal
publisher process. Worker rusage reports the **same** 404900/403992KiB despite
worker VmHWM 163196/163232KiB, consistent with a retained pre-exec launcher
high-water mark. Do not attribute that counter to topic computation or
subtract one peak from another. Procfs evidence, scope and limitations are
retained; no complete production ingestion memory gate passes here.

## Validation and disposition

Six fixtures exercise coalescing/unchanged in-flight checkpoints, edits/deletes
and new subjects, idempotence, failed jobs, revision/scope mismatch, explicit
retry, empty state and an actual spawned process. Failed snapshots never replace
the last publication. The final captured snapshot matches the existing baseline
exactly after separating arrival counters; those counters independently match
every current source version and final revision. The full research suite is
passed: 689 tests in 12.044s with the two author references supplied. The paper
built with resolved references; new-section pages 148--149 were visually checked.
These are local research checks, not a passing GitHub CI or deployment claim.

Retain this research publication mechanism for subsequent predefined checks.
It establishes bounded scheduled work and correct final state on this captured
cohort. It does not establish independent topic quality, persistent local/shared
atomicity, edits during the *real* replay, search isolation in mash, native
parsing/embedding/storage costs, supported MacBook performance, indefinite
arrival stability or selected Rust parity. Keep the broader online/cost and
selection gates open. Ordinary search behavior is unchanged.

Private artifacts under `/home/james/scratch/mash-artifacts`:
`topic-publication-{first,repeat}-20261003.json` and logs,
`topic-publication-repeat-parent-memory-20261003.json`, focused/full test logs.
Individual source text, memberships and checkpoints remain outside Git.
