# Bounded local ranking cost control

On Xenia, fusion avoided reranker work and was cheaper than contextual reranking
on a fixed real-session sample. This measures cost, not relevance; contextual
reranking remains the default until held-out quality evidence supports a change.

## Setup

The optimized LLVM release binary was frozen from commit
`ec2de7e1e451854b4d64cced6aae97a01ac4dfeb`, before the shared topology-pagination
change. Its SHA-256 is
`809c28db5da219e48f6807be40c4087924aef1116e7f2e6bb2988ba962b01361`.
This is an optimized runtime measurement, not a Cranelift build comparison.
The host was Linux x86_64, kernel `6.12.101+deb13-amd64`, glibc 2.41.
Inference used installed quantized All-MiniLM-L6-v2 embeddings and
`jinaai/jina-reranker-v1-turbo-en`; no models were downloaded.

The certified SQLite copy contains 25 threads, 6,137 canonical messages and
1,061 searchable messages/vectors. It is the two-provider convenience sample
described in [the local gold cost record](local-gold-cost-2026-10-03.md), not
the complete node history. All searches requested local-only related discussion,
thread results, limit three and diagnostics. No shared API or inference API was
used, and no indexing was performed.

Six development queries were used: `cbioportal`, `how was cbioportal configured`,
`shared index`, `embedding models`, `search filters`, and `deployment`.
Fresh-process searches used `--no-worker`, interleaving the two rankings and
alternating their pair order. The resident sequence primed fusion, ran its six
queries, then primed contextual reranking and ran the same six queries. Both
resident sequences used one isolated worker, whose PID was verified from trace
replies and whose executable matched the frozen binary. Priming is excluded
from the table. Resident measurements include repeated query inputs; they do
not isolate model reuse from other caches or estimate unseen-query throughput.

## Results

Each row has six measured searches. Wall time spans CLI process launch through
exit, including JSON output, diagnostics, tracing and external monitoring.

| Ranking | Execution | Median CLI wall | Maximum CLI wall | Median observed RSS |
| --- | --- | ---: | ---: | ---: |
| Fusion | Fresh process | 250 ms | 314 ms | 103 MiB |
| Contextual rerank | Fresh process | 1,378 ms | 1,551 ms | 295 MiB |
| Fusion | Resident worker | 30 ms | 40 ms | 84 MiB |
| Contextual rerank | Resident worker | 539 ms | 695 ms | 329 MiB |

Fresh-process RSS is the median of per-query, 20 ms sampled maxima for the CLI's
current executable image; sampled peaks are lower bounds. Resident RSS is the
median worker RSS read immediately after each reply, **not a peak measurement**,
and excludes the short-lived CLI. The two kinds of memory observations should
not be compared as equivalent peaks. Allocator retention and the fixed warm
policy order can affect resident memory. Worker CPU was not measured; CLI
`wait4` CPU for resident requests measures only the client, not inference.
Fresh processes were not filesystem-cold.

Diagnostics show zero reranker calls for all fusion queries. Resident contextual
queries ran two reranker calls each, processing 28–47 total inputs per query.
Median core-reported search time was 19.5 ms for resident fusion and 530 ms for
resident contextual reranking. This supports the interpretation that contextual
inference is a substantial recurring cost, even after process/model startup is
reused. It does not establish that removing it preserves search quality.

All requests completed local retrieval without fallback. The before/after
canonical, provenance and checkpoint table digest remained
`dd22a1958591301fc20038a6a2714d6873a332eb029f33d81645202ffb735935`.
The frozen binary and installed-model inventory marker hashes were unchanged.
Model file contents were not independently hashed before and after this run.

## Captures and remaining work

Private captures are retained outside Git under
`/home/james/scratch/mash-artifacts/ranking-local-cost-20261003`:
`capture.json`, `records.json`, `before.json`, the frozen binary and SQLite copy,
and per-query stdout, trace and measurement files. The executable procedure is
`/home/james/scratch/mash-artifacts/ranking-local-cost-control.py`, SHA-256
`a6af3871f35d448280346c339c71ccec327c0d59c514ff01ed59d7f7f5e12b8e`;
its measurement helper is `tools/benchmark-gold.py`, SHA-256
`b8957333deded682d40f5313375c5e29510f4e1a42458ed80b744d59c84a642c`.
The script creates an exclusive new output directory; reproductions need their
own output and worker temporary directory rather than overwriting this capture.

This is one pass on one node and a small development sample. It supplies no
p95, fleet, concurrent-load or held-out relevance claim. An earlier broader
control stopped at incomplete shared context and remains preserved separately
in `ranking-real-cost-20261003`. Shared/combined ranking costs and deployment
verification remain open in `mash-q3e.4.2.4`; this local control does not close them.
