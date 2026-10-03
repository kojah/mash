# Shared and combined ranking cost control

The bounded control completed 52 successful thread-search requests on Xenia:
six development queries, shared-only and combined scopes, fusion and contextual
ranking, fresh CLI processes and one isolated resident worker. Four worker primes
are excluded from the 48 measured requests. All requested sources completed;
no request reported a shared or context error. Contextual reranking remains the
default: this measures cost, not relevance or a reason to change that default.

| Scope | Ranking | Fresh median / max | Resident median / max |
| --- | --- | ---: | ---: |
| Shared | Fusion | 231 / 330 ms | 94 / 148 ms |
| Shared | Contextual rerank | 997 / 1,025 ms | 497 / 617 ms |
| Combined | Fusion | 243 / 309 ms | 103 / 150 ms |
| Combined | Contextual rerank | 1,014 / 1,154 ms | 491 / 762 ms |

Each cell summarizes six queries: `cbioportal`, `how was cbioportal configured`,
`shared index`, `embedding models`, `search filters`, and `deployment`. Fresh
ranking pairs alternate order. Resident runs prime each policy before its query
sequence, in scope/policy order. CLI timings include diagnostic/trace and
measurement overhead. Fresh processes are not filesystem-cold, and prior
requests warm the API/backend. One pass is not a p95 or fleet measurement.

The previously failing shared contextual query `how was cbioportal configured`
completed with a 27.059 ms context stage in the fresh-process sample and
19.918 ms in the resident sample; end-to-end times were 704 and 456 ms.
Combined context took 19.778 and 12.866 ms, with total times 1,072 and 440 ms.
These observations verify that case and the bounded query set. They do not
guarantee every thread will meet the deadline: child fan-out still affects
small tree reads. They also do not establish a controlled causal speedup over
the archived timeout under different cache/load/optimizer conditions.

## Setup and preservation

Both CLI and staged API used optimized LLVM release source
`6e8f2bb6c5f580aa25e3cb54e40cd0abe4c2402c`, binary SHA-256
`f82bf0bf52a931a3f91899fbf890ead30625f1f0edb7d681b731b2866ce7674c`.
Qdrant 1.19.1 retained the completed native dev-Mac publication: 3,018 threads,
434,486 messages and 68,731 vectors. The local convenience sample contains
25 threads, 6,137 messages and 1,061 vectors. Compare ranking within each scope;
these different corpora do not support a local-versus-shared speed comparison.
Inference used explicitly installed local models, with no downloads or paid API.

Shared freshness stayed identical across deployment and across the control.
The copied local ten-table silver digest stayed
`dd22a1958591301fc20038a6a2714d6873a332eb029f33d81645202ffb735935`.
Binary and installed-model inventory marker were unchanged. The marker does not
hash all model weight files. No indexing, new embeddings or node publication
was performed. Installed node configs and the legacy service were unchanged.

The single observed worker PID was 3970785. Fresh-process sampled RSS medians
were roughly 97–98 MiB for fusion and 294–295 MiB for contextual ranking.
Resident post-reply RSS medians were 84 / 329 MiB for shared fusion/contextual
and 278 / 407 MiB for combined fusion/contextual. The same worker retains models
and state across preceding policies/scopes; those resident values are not
isolated per-policy memory requirements. Fresh samples are lower bounds and
resident samples are not peaks. CLI sampling excludes worker/API/Qdrant memory;
no worker CPU, complete fleet RSS or durability conclusion follows.

## Captures and failed attempts

Private captures are in
`/home/james/scratch/mash-artifacts/ranking-shared-neighborhood-worker-20261004`:
`capture.json`, `records.json`, `before.json`, per-query result/trace/measurement
files and the copied local database. The external script is
`ranking-shared-neighborhood-worker-control.py`, SHA-256
`9bdd2c8e416f3f397985ea053956142c6b275ea25785bf8f555ec70abab34d4b`.
The checked-in `tools/benchmark-gold.py` measurement helper SHA-256 was
`b8957333deded682d40f5313375c5e29510f4e1a42458ed80b744d59c84a642c`.
The model inventory marker SHA-256 was
`d0a88e5bb68f64e9f4d28eca417a7ab94781f3bd539e3680e416ea66db7c713d`.

The first post-deployment attempt completed 24 direct-process requests, then
aborted because its 110-byte Unix socket path prevented worker startup; the
CLI fell back to direct execution, which the harness correctly rejected as a
resident measurement. Its captures remain in `ranking-shared-neighborhood-20261004`.
The corrected run used a separate root and an 81-byte socket path under
`/home/james/scratch/achi-tmp/rpn04`. Earlier context-timeout and backend-test
setup failures remain documented; none are counted as completed controls.

See [local ranking costs](local-ranking-cost-2026-10-03.md) for the separate
local-only experiment and [Qdrant transition](qdrant-only-transition.md) for
deployment, context-validation scope and outstanding fleet migration.
