# Message clustering paper

`message-clustering.tex` is a living experimental paper. Update setup, procedures,
negative findings, results and limitations as measurements arrive; never present
planned experiments as completed. Aggregate results are public; raw histories,
judgments, vectors and research artifacts stay outside Git.

Compile from the repository root, keeping generated files outside Git:

```bash
latexmk -pdf -interaction=nonstopmode -halt-on-error \
  -outdir=/home/james/scratch/mash-artifacts/message-clustering-alternatives-20261005/paper \
  docs/paper/message-clustering.tex
```
