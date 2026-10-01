# Factual payloads before topic boilerplate

Continuation of `mash-c0d.19.2`, using the
[fixed canonical partition](2026-10-01-topic-partition.md). Rust, test-only topic
experiment; no production indexing, search, CLI or MCP changes.

## Source inspection and change

Recorded non-blind coding-agent observations for all 50 previously selected
cases, spanning both providers and short, long, mixed and instruction-heavy
threads. Most judgments use sampled excerpts; two examples also include the
complete first canonical request. This is **not** a completed full-thread
precision/overlap-recall evaluation or independent human review.

Complete PR and UI examples confirm that factual subjects occur after lengthy
worker instructions. The model and bounded label extractor predominantly see
those instructions, producing generic revision/feedback-worker tags for distinct
changes. The frozen corpus contains 76 unique known PR templates and 33 UI
envelopes.

For the known PR request/header/output shape, put the factual source description
first, retaining all trailing text and moving the preamble after it. Unknown,
quoted, duplicated-marker, incomplete and oversized shapes are preserved.
For a complete UI feedback envelope, put submitted feedback first. Retain unknown
instructions and trailing requests. Only the two exact observed worker wrappers
and their known save-result sentence are omitted from derived topic evidence;
unrecognized routing variants remain. Canonical history and source identity
stay unchanged, and changed text receives fresh compatible local vectors.

An initial reorder-only UI run still produced generic worker tags: short feedback
was followed by enough routing text to dominate inference. Removing just the
known wrapper recovers distinct feedback/smoke-check subjects. Observed PR tags
now describe resource cleanup, query results and probe-handle cleanup rather than
only revision guidelines. Some labels still emphasize testing/procedure, and UI
labels still contain generic feedback terms or incidental identifiers.

## Controlled optimized comparison

Same frozen 789-thread Claude Code/Codex snapshot, local installed compatible
model, both insertion orders and radii, cleaned input and representative/whole-mean
discovery. Canonical identities, exact-copy groups and eligibility are unchanged
in all 16 reports: 778 eligible threads, 146 reserved threads, 145 eligible
reserved threads. The table reports representative discovery at radius 0.4.

| Metric | Before: full | Payload first: full | Before: reserved run | Payload first: reserved run |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible, both orders | 778/778 | 778/778 | 770/778 | 770/778 |
| Reserved assigned / eligible, both orders | — | — | 137/145 | 137/145 |
| P95 direct tags, forward / reverse | 5 / 5 | 5 / 5 | 5 / 4 | 5 / 5 |
| Maximum tags, forward / reverse | 41 / 36 | 42 / 40 | 44 / 41 | 46 / 41 |
| Active leaves, forward / reverse | 396 / 384 | 429 / 430 | 355 / 348 | 373 / 386 |
| Named leaves, forward / reverse | 395 / 383 | 428 / 429 | 354 / 347 | 372 / 384 |
| All-instance pair stability | 0.956 | 0.938 | 0.952 | 0.936 |
| Canonical-copy collapsed stability | 0.935 | 0.897 | 0.924 | 0.894 |
| Reserved-only canonical-copy stability | — | — | 0.909 | 0.855 |
| Older raw-vector collapsed stability | 0.836 | 0.793 | 0.822 | 0.796 |

Subject recovery improves in the inspected cases, but insertion-order stability
regresses and the older raw-vector measure now misses the proposed 0.8 target.
Do not select a more favorable metric to declare adoption. The canonical grouping
and older 451-vector-sequence scope measure different populations; neither is
relevance accuracy. One or two weak representative leaves remain unnamed.

The input pass projects 136 messages, embeds 134 unique replacements, omits 78
and deduplicates 15. Compared with the preceding 27 projections/26 embeddings,
the added model work is necessary to avoid reusing stale prefix vectors. No LLM,
remote call or model download is involved. The same cap of 1,024 unique rewritten
inputs and 16,384 candidate-label vectors remains in effect.

## Memory and throughput tradeoff

Long rewritten descriptions increased the held-out process peak to 1,583,332 KiB
(about 1.51 GiB) with 64-document projection waves. Existing CPU inference already
uses eight-document batches, but a wider wave activates multiple long-sequence
model workspaces. Limit projection waves to one eight-document batch. Candidate
phrase ranking keeps its existing bounded batches; ordinary index inference is
unchanged.

Peak RSS falls to 1,130,940 KiB (about 1.08 GiB, 29% less); held-out wall time rises
from 16.56 to 18.88 seconds. Full eight-run diagnostics take 20.54 seconds after
compilation. The narrower input pass takes 4.75 seconds in the full run versus
3.03 seconds with wider waves. Independent artifact comparison confirms identical
tags, labels, graph, counts and partitions in all 16 narrow/wide reports.

These are single-run Xenia CPU measurements including sources, sessions, caches,
temporary evidence, hierarchy and report writes. Memory is still higher than the
previous roughly 583 MiB baseline; this is not a production resource bound,
streaming/index throughput guarantee or Mac/CoreML result. Search requests do not
perform this test-only diagnostic.

## Validation and next work

512 regular tests pass, with 107 skipped. Tests cover payload ordering, preservation
of unknown/mixed/trailing requests, known wrapper removal and malformed/quoted/
nested/oversized inputs. Optimized full/reserved real-model runs, all-target/
all-feature Clippy, doctests, formatting and diff checks pass. A 16-report audit
verifies unchanged partitions/eligibility, exact distinct-thread/provider counts,
acyclic forest relationships and deduplicated inherited tags.

Private source judgments, predictions, intermediate runs, resource profiles and
audits remain outside Git in `mash-artifacts/topic-payload-first-20261001`. No raw
history or conversation IDs are published. Python only compares artifacts; the
projection and inference pipeline remain Rust.

Next bead `mash-c0d.19.2.2` records bounded assignment provenance for full-source
relevance review. Remaining defects include acknowledgment/context fragments,
loaded-skill and plain-header framing, path/procedural names, redundant labels and
the stability regression. Production topic namespaces, lifecycle/persistence and
scope-safe MCP navigation/tags/filters remain unfinished.
