---
name: mash-performance-profiling
description: Diagnose Mash indexing and shared-publication latency, disk contention, and concurrent-publisher capacity using targeted profiling and repeatable load tests. Use for performance investigations and regressions, not routine deployment or correctness-only testing.
---

# Mash performance profiling

Identify what limits progress, change one relevant factor, and measure the effect
without weakening publication or search correctness. Keep the existing harness
when it answers the question; add k6 for controlled arrival rates and sustained
load. A long phase is evidence of elapsed time, not proof of disk saturation.

## Establish the current workload

- Recheck installed and candidate revisions, active publications, configured
  mutation capacity, node identities, corpus size, and backend limits. Historical
  reports locate hypotheses; they do not describe today's activity.
- Use `mash status --shared` and its activity fields: node, phase, operation and
  phase elapsed time, counters/units, occupied and available slots. Distinguish
  capacity waiting, receiving, node-lock waiting, SQLite application, Qdrant
  writes, optimization and search probes. Inspect live help for current flags.
- Read current code before attributing a wait. Shared SQLite files are per node
  under one server; per-node locks and a global whole-mutation limit are separate
  constraints. They still share disk and Qdrant resources.
- Check whether the run is a delta, a full snapshot, a same-revision retry, or
  replay from committed silver. Record preparation, upload and server work
  separately; a retry can repeat work before it even reaches admission.

## Select the smallest useful instrument

**Disk and process counters first.** `iostat`, `pidstat`, `/proc/PID/io`, cgroup
`io.stat`/`cpu.stat`, and `/proc/pressure/io` establish throughput, latency, queue
depth, CPU throttling and pressure. Correlate timestamped samples with Mash phases.
For example, `iostat -dx -y -t 1 60` excludes the misleading since-boot first report;
`pidstat -dru -p PID 1 60` measures a selected process. Identify the backing device
and include the API and Qdrant separately. Container CPU/memory limits matter.

**BCC/eBPF for attribution.** Use `biolatency-bpfcc` for device latency histograms,
`biosnoop-bpfcc` for individual requests and queue latency, and `biotop-bpfcc` for
I/O attribution. Check each installed tool's help and supported kernel probes.
On Debian the tools may live in `/usr/sbin`, outside the user's PATH. Examples,
after obtaining the required execution access:

```bash
sudo /usr/sbin/biolatency-bpfcc -D -m 1 30
sudo timeout 30 /usr/sbin/biosnoop-bpfcc -Q
```

Compare device-service latency with queue-inclusive latency; use filesystem or
off-CPU probes only when block-level evidence leaves the wait unexplained. Keep
captures bounded and start with aggregate histograms to limit profiling overhead.
Writeback threads can obscure which application originally dirtied a page.

Check tool availability and privileges separately. Permission to install packages
does not imply permission to attach kernel probes. If root/capabilities are missing,
report the exact limitation and continue with readable counters; do not change
kernel security settings, create privileged containers, or use package hooks as a
privilege workaround. Never claim an eBPF result from ordinary counters.

**k6 for repeatable arrival patterns.** Use constant/ramping arrival-rate executors
for offered-load tests; fixed concurrent users answer a different question and
can hide overload by slowing arrivals. Keep the existing fixture codec where
possible. Record offered and achieved rates, VUs, dropped iterations, admission
rejections, timeouts, p50/p95/p99 latency, throughput and backlog/drain time.
Use explicit thresholds for the intended acceptance workload. An overload trial
may deliberately exceed capacity: report dropped work and bounded recovery rather
than presenting only successful-request latency.

Assign independent node identities to concurrent publishers. Serialize increasing
revisions within each node, and distinguish publication from duplicate validation.
Use deterministic synthetic vectors to isolate storage from inference; identify
that limitation. Check final silver revisions/counts, current views, released slots
and representative searches. HTTP 200 alone does not prove the intended state.
Do not silently retry ambiguous mutation failures. If a harness retries explicit
pre-admission rejection, disclose that behavior and verify production clients do
the same before claiming smooth caller behavior.

**tokio-console when application waits remain unexplained.** Use it to investigate
task scheduling, semaphore or lock waits after phase/counter evidence narrows the
problem. Check required build instrumentation and measure its overhead separately;
do not compare an instrumented debug build with an optimized baseline as though
only the proposed code change differed.

Loom and Jepsen are not default profiling dependencies. Consider Loom when custom
synchronization primitives need interleaving checks, or Jepsen for an explicitly
scoped distributed failure investigation. Keep the repository's TLA+, boundary
fault tests and real-backend recovery checks for protocol changes.

## Run controlled comparisons

Use the repository's Xenia `agent-test` launcher for heavy builds and workloads.
Disposable Docker backends need explicit memory limits and disk-backed volumes;
the Docker daemon is outside the launcher's budget. Keep logs, captures and input
fixtures under `/home/james/scratch/mash-artifacts`, outside Git. Never load-test
production merely because it is reachable. Use owned disposable groups/services
and wait for workload handles to finish before deleting their collections.

Compare one and two whole-publication slots with the same data and backend limits.
Include small updates, large/large and large/small mixes, late arrivals during
optimization, duplicate retries and sustained offered load. Record binary hashes,
configurations without secrets, cache conditions, sample intervals, resource peaks
and other host activity. Alternate run order and repeat when differences may be
noise. Avoid overlapping builds, cleanup or unrelated benchmarks with measurement.

Increasing the slot count is useful only if latency/throughput improve without
unacceptable contention, memory growth, queue failures or search regressions.
Many accepted publishers do not require many simultaneous disk-heavy operations.
Bound both active work and waiting requests; test eventual queue drain and
cancellation without releasing permits still owned by detached work.

## Interpret and preserve correctness

- Process physical-write counters are affected by writeback and exclude other
  processes. Logical bytes written are not physical disk traffic. Include Qdrant
  cgroup I/O; host pressure and device statistics include unrelated workloads.
- `%util` alone is not a saturation test, especially on virtual or parallel
  storage. Combine latency, queue depth, throughput and pressure. Sampled temporary
  file sizes are lower bounds on peaks; unknown measurements are not zero.
- Qdrant optimization merges/cleans segments and builds indexes in the background.
  Mash may additionally hold publication slots until collection-wide optimization
  drains. Recheck the current policy: optimization, write durability, certification
  and search latency are distinct. One publisher can wait on work from another.
  Test cold and concurrent-write search latency before changing that policy.
- Preserve silver commit-before-views, canonical retry validation, revision/build
  freshness and recoverability. Do not skip validation just because revisions match.
  Review the relevant `protocol/` model, mappings and assumptions for semantic
  changes, and run the required protocol and boundary tests from `AGENTS.md`.

Report the measured workload, result, uncertainty and next decision. Link raw
artifacts and record follow-ups in Beads. Separate implemented, validated and
deployed states; use the repository's mash-deployment skill for an authorized
rollout rather than treating a benchmark as deployment authorization.

## References

- [BCC tutorial and tool semantics](https://github.com/iovisor/bcc/blob/master/docs/tutorial.md)
- [k6 arrival-rate and concurrency executors](https://grafana.com/docs/k6/latest/using-k6/scenarios/executors/)
- [Qdrant optimizers](https://qdrant.tech/documentation/ops-optimization/optimizer/)
- [tokio-console instrumentation](https://github.com/tokio-rs/console)

Consult current primary documentation when tool options or backend semantics are
uncertain. Do not pin a transient host permission, installed version or benchmark
result into this skill as a permanent requirement.
