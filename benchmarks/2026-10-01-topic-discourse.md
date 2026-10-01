# Topic discourse and delivery filtering

Continuation of `mash-c0d.19.2`, after
[parent naming](2026-10-01-topic-parent-labels.md). Rust, test-only topic
diagnostic; no changes to production indexing, CLI, search or MCP behavior.

## Evidence and change

Coding-agent reinspection of the first 15 previously selected review cases found
topic tags for acknowledgments, continuations, tool-use interruptions, command
output and handback delivery. These are non-blind judgments from sampled source
excerpts, not complete-thread review or measured precision/overlap recall. Long
threads and truncated factual briefs require more source evidence.

Replace the short exact acknowledgment list with a bounded, compositional phrase
recognizer: every normalized token must belong to a complete known acknowledgment
or continuation. Punctuation/case normalization remains limited to messages of
at most 256 characters. Unknown tokens, technical subjects, option identifiers
and mixed requests remain evidence. For example, “sure, sounds good, let's keep
going” is omitted, while “sure, sounds good, let's add PostgreSQL” is retained.

Derived input cleaning also omits complete standalone local-command caveat/stdout
envelopes, the known hook-prompt framing and the exact handback delivery notice.
Quoted, nested, malformed, unknown-attribute and trailing-human-request variants
are preserved. Existing notification filtering and canonical history/vector
preservation remain intact. No model downloads, LLM or remote calls.

## Optimized frozen-corpus results

Same 789-thread Claude Code/Codex snapshot and installed compatible local model,
representative discovery versus whole means, two radii and insertion orders.
The table reports radius 0.4, forward/reverse order. Pairwise stability compares
whether threads share at least one direct topic, independently of topic IDs.

| Metric | Representatives, full | Whole means, full | Representatives, held-out run | Whole means, held-out run |
| --- | ---: | ---: | ---: | ---: |
| Assigned threads / 789 | 778 / 778 | 748 / 749 | 768 / 768 | 735 / 736 |
| P95 direct tags | 5 / 5 | 3 / 2 | 5 / 5 | 3 / 3 |
| Maximum direct tags | 41 / 36 | 22 / 20 | 30 / 34 | 17 / 15 |
| Active leaves | 396 / 384 | 127 / 123 | 328 / 334 | 101 / 107 |
| Named active leaves | 395 / 383 | 127 / 123 | 328 / 334 | 101 / 107 |
| All-instance stability | 0.956 | 0.933 | 0.918 | 0.921 |
| Copy-collapsed stability | 0.836 | 0.762 | 0.756 | 0.743 |

Copy collapse selects one thread per exact nonempty ordered **raw**, deduplicated
user-vector sequence before topic filtering: 451 sequences. The read-only analysis
reproduces all four prior copy-collapsed metrics exactly. Assistant-only threads
have no sequence and are excluded from that scope, but remain in all-instance
coverage. Empty pair unions would be undefined rather than a perfect score.

Full representative coverage is unchanged from the preceding experiment, while
maximum tags falls from 44/40 to 41/36 and copy-collapsed stability rises from
0.817 to 0.836. There are 107 additional topic-input omissions across the corpus;
derived cleaning omits 78 messages, projects 27, deduplicates 15 and embeds 26
unique replacements. Full representative parent groups number 97/88, all named.

The held-out partition now contains 172 threads rather than 174; representatives
assign 151/172 in both orders. The current partition hash uses filtered/projected
vectors, so changing preprocessing changes the split. **Held-out comparisons
across these iterations are not controlled.** The new run still fails the proposed
0.8 copy-collapsed stability target. That metric covers all evaluated threads in
the held-out-trained run, not only reserved threads. Freeze source identity and
train/test partition before further algorithm comparisons.

## Review and limits

Reinspection removes the tool-interruption tag from the outreach/concurrency
cases, the handback-delivery tag from the performance case and the exact
continuation tag from the PR-revision case. Relevant tracing, PR outreach,
performance and website/design tags remain. These observations do not establish
full-thread recall or causal improvements in relevance.

Broader acknowledgment clusters, generic context-dependent requests, agent-message
framing, path-heavy labels and duplicate audit labels remain. Do not extend the
filter by deleting arbitrary user words or silently interpret unresolved options.
The 50-thread representative quality review is unfinished; production topic tags,
stable identities, lifecycle/persistence and MCP tree tools remain unshipped.

The eight-run full diagnostic takes 14.30 seconds after compilation. The held-out
process takes 13.72 seconds and peaks at 592,900 KiB (579.0 MiB), including source
strings, model sessions, evidence, label cache, hierarchy and report writes.
These are single-run diagnostic costs, not production throughput/RSS guarantees.

## Validation

507 regular tests pass, with 107 skipped. The focused topic suite, optimized
real-model full/held-out runs, all-target/all-feature Clippy, doctests, formatting
and diff checks pass. Regression cases cover compositional continuations and
preservation of technical, mixed, quoted, malformed and unknown framing. Regular
validation excludes external services; no backend changes.

Private sampled judgments, predictions, metrics and baseline-reproduction evidence
remain outside Git in `mash-artifacts/topic-discourse-20261001`. Python only
compares artifacts; topic filtering and inference stay in Rust.
