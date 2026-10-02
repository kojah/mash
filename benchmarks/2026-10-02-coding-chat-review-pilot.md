# Actual coding-chat source review pilot

P0 `mash-afc.1.1`, preparing evidence for `mash-afc.1`. This moves review from
public programming questions to actual Claude Code/Codex histories. It does
**not** provide semantic labels, guided-agent success, or a production selection.
No LLM, model inference, summarization, label suggestion, or external sharing.

## Procedure and sample

Read the frozen original index snapshot in read-only immutable mode:
`/home/james/scratch/mash-artifacts/search-performance-d62a597/index.sqlite3`,
SHA256 `251795191c9df51352f4a703faa2928017c99196596cd6ec16ec83803a40d463`.
There are789 conversations (515 Claude Code/274 Codex) and225,267 indexed
messages. The snapshot already appeared in previous research; **not held out**.
Complete indexed sequence is not proof of upstream native history completeness.

Before inspecting selected text or candidate outputs, fix seed
`mash-coding-chat-source-pilot-v1-20261002`, two requested documents per
provider/length stratum,3–40 nonempty original user messages, short<=10,
<=512 indexed turns and<=262,144 original UTF-8 content bytes per thread.
No embedding availability, topic output, keyword, label, or retrieval-score
selection. Native indexed content is used, not a rewritten search projection.
Group exact ordered nonempty user sequences by SHA256 with domain separation
and UTF-8 length framing. Collapse aliases to lexicographically first eligible
thread before sampling; rank groups by seed/group hash. This does not detect
semantic near duplicates, prefixes, or all canonical native relationships.

Exclude12 previously reviewed threads/12 exact sequence groups from supplied
`topic-python-boundaries-20261001/annotations.toml` and
`topic-python-review-v2-20261001/annotations.toml`, including matching aliases.
Other historical exposure is not exhaustively excluded.466 threads meet raw
eligibility bounds; after grouping/exclusions, strata have109 Claude-short,
199 Claude-medium,59 Codex-short and1 Codex-medium eligible groups.

Select2/2/2/1 respectively: **7 documents,281 complete indexed turns**.
Report Codex-medium shortfall; do not expand bounds or replace cases after
seeing outcomes. Include all indexed roles and original ordinal order; no
turn truncation or user-only summaries. Sampling limits exclude large/long
histories; this bounded development pilot is not corpus prevalence evidence.

## Review boundary

`review.json`/escaped offline `review.html` have opaque source/turn IDs and
original turn roles/text, without provider/selection strata, suggested subjects,
candidate outputs or canonical IDs. Intrinsic text can reveal context; this is
not a guarantee against reviewer recognition. A separate private
`unblinded-originals.json` retains exact original IDs/text/hash/provenance.
The existing conservative display guard redacts standalone credential-like
values with an explicit marker; zero selected turns trigger it here. It is
not a general secret scanner. Keep every artifact private, outside Git;
directory mode0700/files0600. Rendering escapes source HTML and uses a
restrictive CSP; embedded instructions/code are quoted data and not executed.

Blank `answers.json` and [rubric](../research/topics/chat-review-rubric.md)
request concrete primary/secondary subjects, useful broader subjects, exact
turn evidence, uncertainty and realistic navigation tasks. Draft task kinds
cover thread recovery, broad discovery, multiple subjects, rare technical
entities and scoped lookup. Missing provider/time/node metadata is explicit;
do not invent constraints or derive corpus-wide negatives from one chat.
All fields remain null/unanswered; no assistant-generated human judgments.
Reviewer identity/human/prior-source/candidate-output declarations are self-reports,
not proof of independence. Submission validation, actual independent reviews,
agreement, fresh group-separated tasks and scored agent runs remain to do.

## Repeat and audit

Full file repeat is byte-identical, excluding provenance timing only. Source
database and supplied exclusion pins are checked before/after capture. Four
fixtures check length-framed/order-sensitive sequences, alias exclusion and
order-independent selection, literal schemas/sample caps/shortfall reporting,
complete mixed-role context, exact text hashes, explicit redaction and safe
HTML escaping. Full enabled research suite597 tests passes.

Independent auditor imports no producer and reconstructs database counts,
ordered user sequences/groups, exclusions, candidate/alias selection, all281
original turn IDs/ordinals/roles/text/hash/display records, provenance pins and
blank answers. SQLite/JSON/hash primitives are shared; native completeness,
human semantics, search utility and Rust parity are not proven.

Artifacts under `/home/james/scratch/mash-artifacts/`:

- `coding-chat-review-first-20261002/provenance.json`, SHA256
  `2b526d18425879147579e2e57b33d74c85e1ff6ad3e73ae01ede0c1283dbab5e`.
- Repeat `coding-chat-review-second-20261002/provenance.json`, SHA256
  `6dd886cd53f43ad3cb0ba42ea46b8049fcbadb17f33e0382c78bd5b61945272e`.
- Shared `review.json` SHA256
  `cb8c2aa9e737127c48f90fada40a1a8383dcea94dac17c276287f2820780e682`.
- Auditor code SHA256
  `70c125de3d59b3e28bd7e8529dbf1a5301f768889de6a95610d7e1fdbdeef3e1`;
  output `coding-chat-review-audit-final-20261002.json` records coverage and pins.
  An earlier audit also passed; the final audit additionally asserts literal
  protocol/schema fields and names its source/selection conclusion precisely.

Run `chat_review_packet.py --database SNAPSHOT --exclude-annotations FIRST_TOML
--exclude-annotations SECOND_TOML --output-directory FIRST`, then fresh repeat
with `--compare FIRST`. Audit `audit_chat_review.py --database SNAPSHOT
--packet-directory FIRST --output AUDIT`. Private inputs/output outside Git.
Preparation after imports4.723/2.921s includes snapshot hashing, source reads,
selection and rendering. No controlled performance comparison, external memory
gate, full ingestion overhead or application latency claim. No Rust/app change.

Next: validate source-backed review submissions and obtain credible independent
coding-chat judgments. Separate development interpretation from held-out claims.
Compare simple thread/episode lexical profiles against topic discovery, with
browsing/tagging measured separately from automatic rank guidance. No new model
method or taxonomy has been selected just because this packet is reproducible.
