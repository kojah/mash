# Grounded topic-assignment diagnostics

Continuation of `mash-c0d.19.2.2`. The Rust topic prototype now supports opt-in,
typed private evidence capture. This adds no production index, search, CLI or MCP
behavior. Topic relevance and lifecycle gates remain open.

## Evidence boundary

Set `MASH_TEST_TOPIC_EVIDENCE_THREADS` to a strict TOML file containing a `threads`
array of existing conversation IDs. The ignored `benchmark_topic_discovery` test
writes evidence into its private JSON reports, alongside the existing topic and
hierarchy results. No source content is printed or traced. Leave the variable
unset to retain the previous report shape.

The selection rejects unknown fields, duplicate/empty IDs, unknown conversations,
more than 64 threads, IDs over 256 bytes, and files over 16 KiB. Capture validates
alignment between source records and user-message vectors. Known modes carry
typed state; all-message baseline modes explicitly report unsupported evidence.

Each source retains its stored message ID, ordinal and BLAKE3 fingerprint of the
full canonical content. Cleanup preserves that identity while separately recording
the derived text fingerprint and whether the text is canonical, a stored search
projection, a topic projection, or synthetic fixture data. Source IDs used for the
canonical train/reserved partition remain unchanged.

Each retained direct topic records total cosine-weighted message support,
matching-segment/message counts, and its four strongest supporting segment ranges.
Ranges address the ordered source manifest, not the original conversation's message
ordinals. Scores are similarities and support weights, not relevance probabilities.
Discarded weak topics do not enter this packet. Source capture does not alter
assignment, label extraction, discovery order or hierarchy construction.

Limits are 64 captured topics per selected thread, four sampled segments per topic,
16,384 source records and 8 MiB of excerpt text per report. Each excerpt has at most
512 Unicode characters without splitting a UTF-8 character. Counts explicitly
report omitted sources, topics and segments. Samples distinguish complete derived
text from complete canonical references. Synthetic references cannot satisfy the
canonical-reference flag. Stored references permit loading full source text from
the original read-only database; their excerpts are not necessarily complete.

The serialized source scope is **retained embedded user messages** after filtering,
projection and deduplication. It excludes unembedded and omitted user records and
assistant messages. This is not a whole-conversation export or proof that all major
subjects have been evaluated. Full-thread overlap-recall review must also read the
canonical conversation and account for those omissions.

## Frozen-corpus audit

Used the same 50 selected cases, 789-thread Claude Code/Codex snapshot, compatible
installed local model, canonical reserved split, both insertion orders and radii,
and representative/anchored modes as the
[payload-first comparison](2026-10-01-topic-payload-first.md).

An independent artifact audit checks all 16 reports against that baseline after
excluding elapsed-time fields and the new evidence field. Topic memberships,
labels, partitions, graph relationships, inherited tags and counts are identical.
The audit resolves every captured source against read-only SQLite and checks its
conversation ownership, user role, ordinal and full canonical fingerprint. It
also validates range bounds, score weights, derived-text fingerprints where the
full text is available, sampling omissions and global counters.

Each report contains 1,912 source references and 229,947 bytes of excerpt text.
There are 139 incomplete excerpts; these must not count as complete source review.
The largest full diagnostic report is about 5.35 MB, including pre-existing topic,
label and hierarchy data. Actual memory and inference work remain subject to the
limitations of the earlier prototype measurements; output caps do not establish
a production process-memory bound.

Source-backed spot inspection confirms remaining defects:

- UI topics still emphasize generic feedback and viewport/CSS framing, sometimes
  duplicating tags for the same feedback message.
- Context-dependent acknowledgments produce direct tags such as “sounds good” or
  “sure,” despite conveying little standalone subject information.
- Plain instruction headers and attachment framing contribute policy/procedural
  tags and irrelevant label terms.

These observations concern inspected supporting sources. They are not an
independent human assessment or a completed 50-thread precision/recall review.
The earlier insertion-order stability regression remains unchanged.

## Validation and remaining work

517 regular tests pass, with 107 skipped. Tests cover canonical versus derived
fingerprints, preserved assignments, weak-topic exclusion, malformed selection,
source alignment, synthetic provenance, Unicode/text limits, topic caps and
segment omissions. Optimized full/reserved real-model diagnostics, all-target/
all-feature Clippy, doctests, formatting and diff checks pass.

Private selections, source packets and audit artifacts remain outside Git in
`mash-artifacts/topic-evidence-20261001`; no real IDs or raw histories are published.
Python is used only to audit artifacts. Capture, projection, inference and topic
assignment are Rust.

Keep the bead in progress for full-source relevance/overlap review and fixes. Before
production MCP topic tools, also establish per-index/model namespaces, stable
revisioned identities, edit/delete/restart persistence, and scope-safe navigation
and filters. Planned MCP capabilities remain tree exploration, thread topic tags
with all semantic ancestors, and topic filters composed with provider/node/time
and existing search scopes.
