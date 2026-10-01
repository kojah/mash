# Canonical records, searchable projections and vector coverage

## Why this audit was needed

The [stored-vector experiment](2026-10-02-stored-vector-reuse.md) reported 1,850
stored user-message occurrences out of 8,888 canonical user-role records (20.8%).
That ratio is correct, but interpreting it as incomplete embedding coverage was
unjustified. Provider `user` records can contain tool results or injected context.
Measure coverage against admitted text separately from canonical record counts.
This audit changes the next action: do not generate thousands of unnecessary
embeddings to increase a denominator that includes excluded provider records.

## Full frozen SQLite ledger

`projection_coverage.py` opens the original frozen SQLite snapshot read-only,
revalidates its generation, fingerprint and canonical partition and requires the
exact 128-thread selection capture. Walk every message in canonical ID/ordinal
order. Hash complete original content, stored projection, identities, role/provider,
vector digest/metadata and pending state into a length-delimited source ledger.
No source strings are printed or written to the audit capture. Bounds are 500,000
records, 64 MiB per serialized record and 2 GiB total ledger input; exceed them
with an explicit failure, not a partial result. Preserve the source database.

Distinguish absent overrides (use canonical text), explicit empty, explicit
whitespace, identical and changed projections. SQL admission uses `text != ''`,
while nonblank uses `text.strip()`; do not conflate them. Check every present
signed-byte vector's dimensions, norm and generation. Missing searchable vectors,
excluded records without vectors, unexpected excluded vectors, incompatible
vectors and pending work have separate states. Vector coverage and readiness are
independent; never set a ready marker from this audit.

| Stored role | Canonical records | Nonempty projections | Compatible vectors | Explicit empty projections |
| --- | ---: | ---: | ---: | ---: |
| User | 35,848 | 6,714 | 6,714 | 29,134 |
| Assistant | 130,883 | 22,739 | 22,739 | 108,144 |
| Developer | 2,400 | 0 | 0 | 2,400 |
| Tool | 56,136 | 0 | 0 | 56,136 |
| Total | 225,267 | 29,453 | 29,453 | 195,814 |

There are no missing/incompatible/unexpected/orphaned vectors, pending rows or
whitespace-only projections. All 29,453 nonempty projections have compatible
vectors. Packed semantic chunks also contain 29,453 vectors. However, the ready
generation marker is absent; `search_dirty` is false. Neither matching counts nor
this frozen research artifact proves a successful initial index or operational
search readiness. Do not repair the marker, skip malformed sources or present
the snapshot as a completed production index.

The 128 selected threads have 1,850 searchable user occurrences, all embedded;
7,038 explicit empty overrides; zero pending or missing vectors. Of the searchable
occurrences, 1,795 have no projection override, 40 have changed projections and
15 explicit identical projections. The previous 1,812 distinct vectors plus 38
within-thread duplicates account exactly for the 1,850 stored occurrences.

## Native provenance corroboration

Code discovery shows current `Content::projected` excludes context block kinds,
`search_text::project` removes recognized injected frames, and storage queues all
nonempty search projections. Current rules alone do not establish why historical
records were excluded. `native_projection_audit.py` separately reads the current
native files for the selected Claude empty-projection records.

Use the frozen builder's final native identity contract from commit `d62a597`:
first 24 hex characters of BLAKE3(`claude-message` + NUL + thread + NUL + UUID).
Require exact canonical reconstruction/hash equality before assigning a structural
category. Unknown reconstruction is unsupported. Hash each complete current
source and reject its corroboration if device/inode/size/mtime changes during
reading. Record malformed lines, mismatches, missing sources and ambiguous categories.
Bounds are 64 MiB/line, one million lines/file and 1 GiB total reads.

All 38 selected current Claude files are stable, totaling 113,265,716 bytes, with
no malformed lines or target content mismatches. All 6,771 selected Claude empty
records are corroborated: 6,744 tool-result-only, nine text-block-only and 18 plain
text. The 6,744 tools explain 95.8% of all 7,038 selected excluded user-role records;
the other 267 excluded records are Codex and are outside this native audit.
Do not classify the remaining 27 Claude text-shaped records as human requests or
as semantically useless without further review. No claim follows about every
excluded record in the full corpus or about historical full-parser equivalence.

The first native probe used preliminary path-based message identities and matched
none. Historical source inspection established that normalization replaces those
IDs with native thread/UUID IDs. Preserve the first capture; use the verified final
contract without fallback aliases. The corrected final captures are separate.
No embeddings, source histories, parser behavior, search projections or readiness
state were changed.

Canonical text remains available for context resolution. Tool results can explain
referential requests even when they are unsuitable as standalone user-topic units.
Full topic recall requires a defined evidence policy and independently reviewed
primary/secondary aspects, not embeddings for every canonical provider record.

## Reproducibility and limits

Full-ledger runs take 7.200/5.633 seconds after imports, including original dataset
validation and hashing. Native final runs take 1.030/1.060 seconds. These are audits,
not model, ingestion, search or memory benchmarks. Both repeat every non-timing
field. Independent SQL/source-ledger reconstruction checks all 225,267 records,
29,453 raw vectors, provider/role counters and all selected thread counts. The
research suite passes 244 tests, including null versus empty overrides, whitespace
admission, vector-generation/malformed-state checks and native tool/text identities.
No Rust parity or production topic/ingestion gate is established.

Private captures remain under `topic-stored-vector-reuse-20261002` outside Git:

- Complete source ledger: `58d5aa1b3cc97b6988a88da8a9ccbf8ef9c397194def25aa0fc9fd02a1493663` (467,686,898 serialized input bytes).
- Coverage first: `340b97c70f492c7b3294a5a82259bc56e018dadd04a032e23f2b96b3603c0a34`.
- Coverage repeat: `74a70c37c400bb6b0af65ea21f5b6ba9a84b4d736ea51ed89f9cc7fe334950c2`.
- Native final first: `adc87420ec8208bcd151cd0ff87d0947818cea3433ee6c55dbfa8b95be620322`.

Next validate reference/context use and actual aspect/label quality, preserving
explicit exclusions. Complete indexing/readiness and lifecycle/performance gates
remain separate requirements.
