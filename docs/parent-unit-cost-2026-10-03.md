# Local message versus parent-unit costs

On Xenia, 2026-10-03, `tools/benchmark-retrieval-units.py` copied an already
certified SQLite corpus using SQLite's read-only backup API. It rebuilt gold
without provider reads or remote inference, then searched each active recipe.
The corpus contains 25 Codex/Claude threads, 6,137 canonical messages and 1,061
visible dense anchors. It is the bounded size-stratified corpus described in
`local-gold-cost-2026-10-03.md`, not the live database or a fleet-wide sample.
Of those anchors, 194 have a retained visible parent under the selected unit rule.

The binary was the debug Cranelift development build, with explicitly installed
local MiniLM assets. Exact searches used `exact-mention`; related searches used
`related-discussion` with fusion and no worker. Six queries per intent were run
once each: cbioportal, rust, qdrant, search, embedding and configuration.
Processes started fresh; filesystem and model pages could remain cached.
No compiler or validation suite ran concurrently with this capture.

| Active unit | Build wall time | Model inferences | Occupied SQLite | Exact search median | Related search median |
| --- | ---: | ---: | ---: | ---: | ---: |
| Message, already current | 0.039 s | 0 | 20.555 MiB | 31.8 ms | 346.3 ms |
| Parent and message, replacement | 19.443 s | 1,046 | 20.629 MiB | 32.7 ms | 355.5 ms |
| Message, replacement | 12.684 s | 1,045 | 20.555 MiB | 33.4 ms | 369.5 ms |

Both replacements stored 1,061 vectors. The prepared-text deduplication groups
changed slightly with context. The first stage was a readiness check of existing
gold, not a 39 ms dense rebuild. The parent replacement took about 53% longer
than the following message replacement in this single ordered run. It increased
occupied pages by approximately 76 KiB; longer generation identities and page
allocation contribute even though no canonical body is copied into durable gold.

All ten canonical/provenance tables retained the same SHA-256 across all three
stages and all searches. Hash:
`dd22a1958591301fc20038a6a2714d6873a332eb029f33d81645202ffb735935`.
The binary SHA-256 was
`e657083a5539b71b8c190acb1bf55ed237ef157427f49a14e99dc3b71bbdafaf`.
The private capture, full result payloads and per-process measurements are outside
Git at `/home/james/scratch/mash-artifacts/parent-unit-cost-final-20261003/`.

Current-image sampled RSS maxima were approximately 1.37 GiB for the parent
replacement and 1.39 GiB for the message replacement. These are 20 ms sampled
lower bounds, not proof that parent units use less peak memory. The capture
retains RSS/PSS/status counters separately from `wait4` rusage; launcher peaks
must not be attributed to the workload or subtracted. Monitor overhead is included
in wall time. Parent preparation for shared streams has additional temporary
SQLite storage, which this local-only capture does not measure.

There is no p95 estimate from these six single trials, no cold-disk or resident
worker comparison, and no semantic relevance judgment. Longer prepared input
also remains subject to the embedding model's tokenizer limit, so appended
context may be truncated. This result supports keeping parent units opt-in;
held-out quality, shared/fleet costs, edit/delete behavior and multi-passage
source mappings remain acceptance work in `mash-q3e.3.9.3`.
