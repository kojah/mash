# Held-out source readiness and physical units

P0 `mash-c0d.19.2.20.8.3.3.3.9.4.5`, still in progress.
Frozen GUIDANCE_VALIDATION_PROTOCOL.md unchanged. Source-only prerequisite,
not held-out query/semantic/label quality or a production method selection.

Reuse the installed pinned tokenizer, unpadded/untruncated singleton calls;
no embedding inference, LLM/API or model download. Raw and equal-length
exact-marker-masked variants both must be nonempty and <=512 tokens. Retain
every original identity, variant input hash/token/status and explicit gap.
Physical units use16 contiguous ready turns/512 total raw-model tokens; source
end/gap/turn/window flush precedence matches the original pool. Separating
boundaries from normalized vector pooling preserves ready lexical-zero sources
without fabricating vectors or discarding them. No queries/labels/features used.

All20,123 original sources captured;18,916 common ready,1,207 unsupported;
2,073 blocks. Raw statuses20,090 ready/16 over-window/17 empty; masked18,921
ready/11 over-window/1,191 empty. Source references are not yet evaluated;
these are support counts, not a relevance denominator or quality claim.
Complete non-timing repeat and begin/end source/tokenizer/code pins pass.
Independent audit reconstructs native text, regex masking, tokenizer counts,
readiness and greedy boundaries/ownership for every source/block without
producer imports. Tokenizer engine/assets shared; not inference/Rust parity.

Three new fixtures include100 deterministic generated original-pool comparisons
with cancellation/gap/window/turn limits, zero-independent ownership, empty
inputs and invalid-support rejection. Full enabled suite591 tests passes.
No production app/Rust/deployment change.

Artifacts outside Git under `/home/james/scratch/mash-artifacts/`:

- `qmsum-validation-units-first-20261002.json`, SHA256
  `6192cf134f0d49c54fc9b367a9c80745edfee1735f404984dcf7312bfb665c1c`.
- `qmsum-validation-units-second-20261002.json`, SHA256
  `d0c0df87cae917c45979a6dbc158405b7f43794031a990899185bc230af32498`.
- `qmsum-validation-units-audit-20261002.json`, SHA256
  `6c55174ec476d254e451a1326413d41c8978b39d053f1adc2cc0ccd12ecb110e`;
 20,123 sources/2,073 blocks pass, auditor SHA256
 `b46af9ab2639536e906115455fcceb74b008c2ca6358e0e8ae076ffa95829c09`.

First/repeat after-import wall2.594/3.075s, CPU2.523/2.825s. Main excludes
imports/final serialization/repeat comparison, external target includes them.
Rusage217,064/217,024KiB; external RSS/PSS217,160/209,596 and
237,624/230,045KiB; approximate HWM217,160/237,624KiB. Samples161/182,
vanished1/1, median/max gaps25.646/30.747 and26.680/43.867ms. One target,
children/observer excluded, monitoring intrusive, sampled maxima lower bounds,
status approximate, launcher high water may survive exec. Repeat comparison
retains another result ledger outside main timing; no worker-memory attribution,
subtraction or full-ingestion/performance-gate claim. Scientific targets
sequential; fixture work around the repeat and host effects uncontrolled.

Next: frozen training vocabulary/IDF source counts, source-only hierarchy/groups,
full query comparison/meeting bootstrap, repeat/independent audits and costs.
Original unsupported/OOV/zero cases and all reference denominators must remain.
Coding semantic/multitag/ancestor/agent utility, shared online lifecycle,
complete ingestion and selected executable Python/Rust parity remain gates.
