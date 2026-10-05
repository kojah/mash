# Message clustering paper

`message-clustering.tex` is a living experimental paper. Update setup, procedures,
negative findings, results and limitations as measurements arrive; never present
planned experiments as completed. Aggregate results are public; raw histories,
judgments, vectors and research artifacts stay outside Git.

Compile from the repository root, keeping generated files outside Git:

```bash
latexmk -pdf -interaction=nonstopmode -halt-on-error \
  -outdir=/home/james/scratch/mash-artifacts/turn-embeddings-20261005/paper \
  docs/paper/message-clustering.tex
```

The latest experiment compares four turn representations and retries nine
clustering families. Its combined bundle/cluster diagnostic improves macro F1
modestly but misses precision and baseline-gain requirements. See
[`turn-embedding-experiment.md`](../turn-embedding-experiment.md) for the frozen
protocol, negative decision, timings and private reproduction ledger.
