# Topic experiment memory accounting

2026-10-01; `mash-c0d.19.2.27`. This corrects memory attribution, not algorithm
results, and does not establish the complete-ingestion gate.

## Cause and reproduction

Direct launches repeatedly reported `ru_maxrss = 978884 KiB` (955.94 MiB), even
for eight records' label-only work whose simultaneous VmHWM was about 132 MiB.
The observed launcher ancestor's VmHWM was also exactly 978884 KiB. Linux preserves
resource-usage statistics across `execve` while replacing memory mappings, so
pre-exec peaks can contaminate attribution. [getrusage manual](https://www.man7.org/linux/man-pages/man2/getrusage.2.html),
[execve manual](https://www.man7.org/linux/man-pages/man2/execve.2.html).

`memory_exec_control.py` uses a single-threaded forced-fork driver to isolate
children from the outer launcher peak, comparing clean Python with Python reached
after allocating/touching 64 MiB and executing again. Two final captures: clean
rusage 14.48–14.63 MiB and VmHWM 14.54–14.63 MiB; seeded rusage 76.76–76.98 MiB,
but post-exec VmHWM 14.42–14.48 MiB. This establishes the mechanism. Ancestor equality
and disagreeing workload counters support the diagnosis for our launches; the
launcher implementation itself has not been instrumented.

## External audit

`memory_audit.py --output PRIVATE.json -- PYTHON research/topics/RUNNER.py ...`
launches without a shell and samples only the target PID every 20 ms. It preserves
exit status, all samples, source hash and invocation. Address-space teardown before
zombie state is treated as a vanished sample; malformed live counters fail.
VmRSS/VmHWM are approximate; smaps_rollup RSS/PSS snapshots are more precise but
cost more. Sampled maxima are lower bounds, not continuous peaks. Observer reads
may perturb timing. Children and observer memory are excluded. [Linux procfs documentation](https://kernel.org/doc/html/v6.15/filesystems/proc.html).

Two fresh runs per workload, unchanged inputs/code/parameters:

| Workload | Samples | Sampled RSS maximum (MiB) | Sampled VmHWM maximum (MiB) | Target rusage maximum (MiB) |
| --- | ---: | ---: | ---: | ---: |
| Field labels, eight records/nine controls | 55 / 53 | 132.71 / 133.74 | 132.71 / 132.81 | 132.58 / 132.56 |
| Feedback payload pilot, 16 controls | 117 / 110 | 307.76 / 304.85 | 307.76 / 304.85 | 307.59 / 304.79 |

Both include imports; the payload workload also includes frozen-corpus validation
and local inference. Smaps and status counters differ slightly. PSS varies with
shared mappings/concurrent processes; it is recorded, not treated as private
allocation. Independent runs occasionally overlap, so timings are not promoted
as new speed results. No RSS totals are summed across processes.

All nine field-label and 16 payload controls match the original captures and their
fresh repeats in every nonmeasurement field, including scores, sources, labels,
memberships, encoder calls and window support. Historical captures remain immutable.
956 MiB is a historical rusage reading, not a demonstrated workload-image peak.
Matched-unit and fragment workloads have not been remeasured and require audit
before memory gating. The <=10% complete-ingestion target remains unverified.

## Evidence

128 lab tests pass: malformed units/missing/duplicate counters, address-space
teardown before zombie state, a real touched allocation and nonzero exit preservation.
No Rust or production changes. Scripts use only the standard library.

Private root `topic-python-review-v3-20261001` contains
`memory-exec-control-confirmed-{first,second}.json`,
`{field,payload}-memory-{audit,capture}-confirmed-{first,second}.json`, and
`memory-confirmed-summary.json`. Earlier target captures whose observer failed at
exit, and captures preceding the race fix, are excluded from final audits.
Sampler SHA256 `2cdee7bf1fdd4bcf6309f0ab3ecd8c1c0a9f5d031022fa95bff0f74c2657e538`.
Field audit SHA256:
`c651be2676f5bdbdcc714f8e1141d5c1b7e3ccaff02b877177fc00683b1a9271`,
`d75febf3fdd7108ed512aa43652efa9eaabeee3072b0da2d7deb447705eb856a`;
payload audit SHA256:
`a5e763a3723a5a84d588bda6a4dc59a5c1fe68142397ddb0b7d33a9a86b26af3`,
`d1112234ee0d21d87e0d337336f0cce1a969a54325590b2b34a04d0e6572c6c6`.

Future complete-ingestion gates must define child/process-group scope, monitor
each program image, pin sampling/workload phases and retain uncertainty about
missed transient peaks. Do not subtract pre-exec peaks or compare inherited rusage
as engine memory. Initial/incremental edits/deletes, persistence, CPU/p95 and the
throughput target remain unverified in `mash-c0d.19.2.22`.
