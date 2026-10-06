# Frozen turn-clustering baseline: complete-answer-v1

Frozen on 2026-10-06 by user decision. This historical local reference is
preserved alongside the [current experimental reference](turn-clustering-baseline.md).
Its original machine-readable record is
[complete-answer-baseline.toml](complete-answer-baseline.toml); the algorithm's
exact implementation is Git revision
`cdc683efb83de14908d84481dedcaecbb944c580`.

| Component | Frozen choice |
|---|---|
| Turns and context | Existing branch-aware delineation and bounded ancestor context |
| Answer coverage | All nonempty assistant segments belonging to the turn |
| Encoder | Installed quantized MiniLM, 384 dimensions, singleton inference |
| Pooled view | Context-bearing windows, equal-role pooling |
| Chunk view | Heading-aware structural boundaries and existing token budgets |
| Proximity smoothing | Original single-pass smoothing, alpha .10, window four; pooled view only |
| Clustering | BIRCH radius .60, cosine membership cutoff .75 |
| Primary cluster retention | At least two distinct turns |
| Multi-label fusion | Union with separate namespaces for the two views |
| Additional denoising / generic penalties / consensus | Disabled |

The [result report](complete-answer-baseline-results.md) records overall F1
86.05%, long-thread F1 79.61% and long-thread recall 78.34%. Freezing establishes
a stable comparison point; it does not change the existing acceptance thresholds
or declare production acceptance. The confirmation set remains sealed.

## Freeze contract

- Do not change this baseline's settings, extraction, pooling, normalization,
  insertion order, tie behavior, memberships or reported development results.
  Source revision and recorded hashes define details beyond the table.
- Stop tuning this baseline against the existing development labels. A future
  algorithm change must be a separately named candidate with an explicit
  hypothesis and evaluation procedure. Preserve this reference unchanged.
- Keep the original annotations and artifacts. The manifest pins the original
  protocol, build contract, selected/control reports and their file catalogues.
  Private reports contain individual artifact, annotation and code hashes.
- Verify hashes before reusing frozen results. Artifact-catalogue hashes describe
  the complete selected view: 160 manifests plus 160 matrices. Construction-source
  hashes refer to the six archived builder files; the live build used eight-vector
  transactions. Later cache checkpoints of 128 preserve inference batching and
  outputs, but must not rewrite the recorded timings or provenance.
- Confirmation evaluation is a separate next step. No confirmation labels are
  read as part of this freeze, and no new candidate or denoising study is launched.

This freezes the Python experimental pipeline. Production message embeddings and
search behavior stay as they were.
