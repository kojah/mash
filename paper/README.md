# Living research manuscript

Working title: **Multi-topic Modelling of LLM Chat Sessions for Guided Agent
Search**. Track manuscript work in `mash-afc`. Topic research remains P0.

Update `main.tex` and `references.bib` alongside every substantive experiment:
describe procedures, input selection, source/model/software/hardware provenance,
parameters, metrics, results, analysis, limitations and the next decision.
Separate planned work, exploratory development and held-out evaluation explicitly.
Do not turn diagnostic metrics into quality claims, infer statistical significance
from single runs, or claim novelty before reviewing prior art. Record negative
results as well as improvements. Authorship and submission venue remain open.

The draft contains twelve Python geometry controls, the initial user-only boundary
pilot, an expanded eight-conversation review and a bounded preceding-context audit.
It is a research record, not a completed
paper or evidence of production topic quality. Full model/hardware provenance,
independent annotations, quality/online lifecycle gates and guided-agent retrieval
evaluation remain incomplete. Raw histories, annotation IDs and individual topic
assignments stay in private artifacts outside Git.

Compile with local TeX Live (`pdflatex` and `bibtex`), without shell escape or
downloads, keeping all output outside the repository:

```bash
python3 paper/build.py \
  --output-directory /home/james/scratch/mash-artifacts/paper-build
```

The build checks for unresolved citations/references. Inspect the PDF after
changes that affect layout. Experiment code/protocol is in
[`research/topics`](../research/topics/README.md); public aggregate research
records are in [`benchmarks`](../benchmarks/2026-10-01-topic-quality-research.md).
