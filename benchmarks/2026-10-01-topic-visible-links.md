# Visible Markdown text for topic labels

Focused `mash-c0d.19.2.9` follow-up to the
[grounded context experiment](2026-10-01-topic-context.md). Rust test-only;
production indexing, CLI, search and MCP behavior are unchanged.

## Change and boundaries

The reviewed pipeline thread's labels include words from a hidden filesystem
destination, rather than just the visible recall-audit link text. Use
`pulldown-cmark` 0.13.4, with default features disabled, as a development dependency
to distinguish resolved Markdown links and their visible text. The parser's
[source offsets and reference definitions](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/struct.Parser.html)
avoid maintaining a partial Markdown recognizer.

Opt in with `MASH_TEST_TOPIC_VISIBLE_LINKS=1`; any present value enables the
experiment. Unset it for the raw control. Reports serialize typed `label_text`
metadata. Single-word and phrase label extraction operate on at most 4,096 source
characters in the candidate. Existing phrase extraction still stops at 256 tokens.
Mask parser-confirmed link/image syntax and destinations with punctuation, then
restore visible source text/code spans. Resolved reference definitions are masked
as well. This prevents phrases spanning an omitted destination.

Visible technical names and image alt text remain available. Literal inline or
fenced code, unresolved/malformed syntax, plain autolinks and email links remain
unchanged. Bare filesystem paths and HTML attributes are outside this change.
This is label-term extraction only: do not modify source text, source hashes,
document embeddings, canonical identity, partition or confidence votes. The
development-only parser does not add a production binary dependency.

## Frozen-corpus comparison

Same 789-thread corpus, generation, canonical reserved split, context/conserved
support/exact-routing candidate and 53 selected evidence threads. Run 16 configurations
per variant: both modes/radii/orders and full/reserved discovery models.

All 16 raw controls reproduce the preceding context candidate except timings and
the new `label_text` field. All 16 visible-link candidates preserve every structural
report field exactly, excluding label content/scores, label-model accounting and
timings. Captured source evidence remains byte-for-byte equal. Independent audits
also validate distinct thread/provider counts and semantic forest relationships.

Primary representative radius 0.4:

| Scope / order | Label orders changed | Label word sets changed | Unique candidate phrases, raw → visible | Named active topics |
| --- | ---: | ---: | ---: | ---: |
| Full / forward | 157 | 103 | 8,982 → 8,977 | 409, unchanged |
| Full / reverse | 156 | 99 | 9,099 → 9,086 | 408, unchanged |
| Reserved model / forward | 150 | 92 | 8,293 → 8,285 | 373, unchanged |
| Reserved model / reverse | 145 | 87 | 8,321 → 8,317 | 370, unchanged |

These counts compare label text/order, not merely floating-point score changes.
Source term changes alter global c-TF-IDF weights, shortlists and selection; effects
are wider than the inspected thread. Unchanged memberships and named-topic counts
do not prove improved label precision.

In the reviewed full-forward pipeline example, hidden-path phrases disappear.
Visible recall-audit, confirmation and evaluation subjects remain, with evidence
boundaries newly surfaced. Generic wording about current uncommitted planning also
enters the shortlist. The specific deadlock domain is still missing in that variant;
the other three primary variants retain deadlock/harness subjects. Bare-path names
in other variants remain. This is a targeted nonblind review, not a completed
50-thread precision/secondary-subject recall gate.

Retain visible-link extraction as an opt-in candidate for the next source-based
review. Prioritize domain-aware shortlist/ranking diagnosis, generic phrase noise,
and major/secondary-subject recall before production persistence and MCP topic
tools. The focused destination-exclusion issue is complete; overall topic quality
and contextual inference remain open.

## Validation and cost

544 regular tests pass, with 107 skipped; all-target/all-feature Clippy, doctests,
formatting and diff checks pass. New fixtures cover visible technical names,
destination/title exclusion, nested formatting, inline code labels, image alt
text, resolved reference definitions, malformed/unresolved syntax, literal code,
autolinks, Unicode and phrase boundaries.

Eight-configuration diagnostic time: full 21.40 s raw versus 22.01 s visible;
reserved 20.91 s versus 21.01 s. Peak RSS is approximately 1.09–1.10 GiB.
Other jobs share this host, and these single pairs cannot establish production
overhead or search latency. Parser allocation is bounded by the source prefix;
phrase/vector reuse keeps the existing 16,384-entry model-scoped label cache.

Private reports and audits live in `mash-artifacts/topic-links-20261001`. No real
identifiers or raw conversation history are checked in. Runtime processing is
Rust; Python performs offline artifact comparisons only.
