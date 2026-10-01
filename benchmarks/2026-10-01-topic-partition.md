# Fixed topic evaluation partition

Implements `mash-c0d.19.2.1`, following
[discourse filtering](2026-10-01-topic-discourse.md). Test-only Rust diagnostics;
production CLI, MCP, search, indexing and backend behavior remain unchanged.

## Canonical source identity

Capture every canonical user record for each evaluated conversation before topic
omission, search projection or model work, including unembedded messages. Hash
ordered, exact-deduplicated UTF-8 text with a versioned domain and length framing.
Provider, conversation/message IDs, embedding generation/vectors and derived
topic text do not determine identity. Exact copied sequences share the partition
across providers; different text is not merged merely because quantized embeddings
match. Assistant-only conversations have no user-source identity.

The first digest byte modulo five selects the reserved set. Derived cleaning
preserves that identity, even when it removes every eligible message. Source edits
change identity; this is a fixed-snapshot evaluation split, not production topic
identity or revision lifecycle. Partial/shared-prefix histories and near copies
are not treated as exact copies and can still cross the split.

Private reports contain a typed, versioned partition summary and source-ID map,
separate eligible/assigned counts, and reserved eligible/assigned counts. No source
text or digest maps are printed to stdout/traces. Discovery-sample selection is
factored into its existing representative module; assignment semantics are unchanged.

## Frozen corpus and independent audit

Same 789-thread Claude Code/Codex snapshot, installed compatible local model,
cleaned input, both orders/radii and representative versus whole-mean discovery.
Canonical text produces 665 exact source groups across 788 conversations, with
one assistant-only conversation. Three conversations previously appeared to be
assistant-only when considering embedded records alone; their unembedded user
messages now contribute to source identity.

There are 778 eligible conversations. Representatives assign all 778 in the
full-corpus run; 11 remaining conversations have no retained user evidence,
rather than being assignment failures. The fixed partition reserves 146 threads,
of which 145 are eligible. A read-only SQL audit verifies exact grouping from all
canonical user text, equal identity maps in all 16 reports, held-out/count bounds,
and unchanged full-corpus direct memberships and hierarchy against the prior run.

The following table reports radius 0.4. Pairwise Jaccard asks whether threads
share at least one direct topic, independently of topic IDs; it is not relevance
accuracy. The reserved-only comparison excludes training threads.

| Metric | Representatives, full | Whole means, full | Representatives, reserved run | Whole means, reserved run |
| --- | ---: | ---: | ---: | ---: |
| Assigned / eligible threads, forward / reverse | 778/778 / 778/778 | 748/778 / 749/778 | 770/778 / 770/778 | 740/778 / 741/778 |
| Reserved assigned / eligible, both orders | — | — | 137/145 | 135/145 |
| P95 direct tags, forward / reverse | 5 / 5 | 3 / 2 | 5 / 4 | 3 / 3 |
| Maximum direct tags, forward / reverse | 41 / 36 | 22 / 20 | 44 / 41 | 16 / 13 |
| All-instance stability | 0.956 | 0.933 | 0.952 | 0.895 |
| Canonical exact-copy collapsed stability | 0.935 | 0.893 | 0.924 | 0.841 |
| Reserved-only canonical-copy stability | — | — | 0.909 | 0.792 |
| Older raw-vector sequence collapsed stability | 0.836 | 0.762 | 0.822 | 0.750 |

The older metric uses the same 451 nonempty ordered raw user-vector sequences
as before. Canonical text grouping is a different scope; its larger score does
not prove an algorithm improvement. The fixed reserved set also differs from the
prior filtered-vector split, so the previous 0.756 and current 0.822 are not a
controlled before/after quality comparison. This run establishes a versioned
baseline for future preprocessing experiments. Radius 0.6 still underperforms:
representative raw-vector collapsed Jaccard is 0.525 full and 0.598 reserved-run.

Representative radius 0.4 clears the proposed 0.8 stability target in this split,
including the older stricter vector scope. That does not establish robustness
across multiple seeds/partitions, eliminate near-copy leakage, or establish label
precision and overlap recall. One weak leaf remains unnamed in each representative
order; generic/context-dependent tags and path-heavy labels remain. The broader
50-thread source-based relevance review, stable topic namespace/revision,
lifecycle/persistence and scope-safe MCP tools are unfinished.

## Validation and resources

509 regular tests pass, with 107 skipped. The final focused topic suite also
passes after sample-selection extraction. Regression tests cover source ordering,
length boundaries, exact deduplication, distinct text, absent user sources,
cross-provider copied partitions, omissions leaving identities unchanged,
eligibility versus baseline coverage and canonical unembedded records. Optimized
full/held-out real-model diagnostics, all-target/all-feature Clippy, doctests,
formatting and diff checks pass. No external backend changes.

Eight full configurations take 13.61 seconds after compilation; the reserved
process takes 13.07 seconds and peaks at 597,544 KiB (583.5 MiB). These include
source loading, sessions, caches, hierarchy and private report writes; they do
not prove production streaming throughput or a memory bound. Raw canonical text
is read only to compute identity, then released by the loader.

Private reports, metrics, SQL audit and logs are outside Git in
`mash-artifacts/topic-partition-20261001`. Python only audits/computes artifact
comparisons; canonical identity and experiment partition are implemented in Rust.
