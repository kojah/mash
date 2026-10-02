# Complete-source semantic windows and quantized inference fidelity

Bead `mash-c0d.19.2.30.3.1`, prerequisite for full-corpus semantic retrieval
under `.30.3`. No semantic retrieval outcomes or production topic method are
selected here. This checks executable-reference behavior before optimizing it.

## Protocol

Use the pinned CQADupStack programmers candidate corpus and 64 development
queries. Corpus fields are original title/body; query text comes only from the
development split. Metadata and reserved query text are excluded. Preserve
source Unicode, whitespace and code rather than rewriting or decoding tokens
back into approximate source. The [MiniLM model card](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2)
documents a 256-wordpiece default and Stack Exchange training data. Unknown
pretraining overlap prevents treating this as a zero-shot evaluation.

Installed Xenova snapshot `751bff37182d3f1213fa05d7196b954e230abad9` only:
quantized ONNX `afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`,
tokenizer `da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.
No downloads or generative/paid inference. Disable saved truncation/padding
before whole-source measurement. For every nonempty field, use contiguous
original-text slices with ≤254 payload tokens plus two special tokens. Cut
only at tokenized whole-word boundaries, then re-encode and enforce ≤256.
Record original code-point and UTF-8 byte spans, source/window hashes, complete
token/type IDs and payload counts. Empty fields are explicit; none occur here.

Before inference, select four corpus windows per length stratum 2–32, 33–64,
65–128 and 129–256 tokens, by hash seed `mash-batch-window-v1-20261002`.
This is length-stratified diagnostic coverage, not a random quality sample.
Compare each new window singleton to the existing executable Python reference
exactly, then compare the same 16 windows in one mixed padded batch. Use
mask-aware float32 mean pooling, existing symmetric int8 rounding and normalized
float64 directions, ORT 1.30.0/tokenizers 0.23.2, CPU one intra/inter-op thread.
This does not test Rust inference or downstream memberships near cutoffs.

## Verified source coverage

| Field | Sources | Over 256 / 512 tokens | Maximum whole tokens | Windows / maximum per source |
| --- | --- | --- | --- | --- |
| Title | 32,176 | 0 / 0 | 54 | 32,176 / 1 |
| Body | 32,176 | 9,818 / 2,295 | 7,996 | 45,739 / 32 |
| Development query | 64 | 0 / 0 | 26 | 64 / 1 |

Body median/p95 length is 176/584 tokens. The full ledger has 77,979 windows
and 7,758,581 payload tokens. Silent 256-token truncation would affect about
30.5% of bodies. Complete token coverage is necessary evidence, not a guarantee
that word-boundary windows preserve semantic episodes or all cross-window context.

The first process was explicitly terminated after finding repeated tokenizer
offset/word-ID property-list copies inside the boundary loop. Caching those
lists preserves the policy; the restart followed that identified cost, not a
timeout. Initial fresh captures agreed but the independent source audit then
caught an initialization bug: the first title's whole-source diagnostic count
was 128 due to saved padding, instead of 9. Initialization now disables flags
before any measurement, with a regression test for saved padding/truncation.
All original window spans/input IDs are unchanged by that repair; discarded
captures remain outside Git. A self-repeat alone was insufficient validation.

Corrected first preflight costs 92.087 seconds after imports, including full
source integrity checks, tokenization, window ledgers and hashing (output writing
is outside this timer). This is offline research preparation, not a production
ingestion gate. Full incremental/lifecycle/RSS requirements remain unmeasured.

The corrected repeat matches all nontiming fields in 85.061 seconds. The
independent audit verifies all 64,416 corpus fields and 64 development queries:
exact source hashes, adjacent character/byte spans, complete source reconstruction,
re-encoded window token/type IDs and concatenated payload IDs equal to whole-source
tokenization. All 77,979 windows/7,758,581 payload tokens pass. This shares the
pinned tokenizer and source loader; it is independent validation of the window
ledger, not an independent tokenizer implementation or semantic segmentation gold.

## Batch fidelity findings

All 16 window singletons match the existing executable reference exactly.
All 16 mixed-batch directions differ; minimum singleton/batch cosine is
`.9871904276`, maximum direction-component drift `.02708835`, maximum pooled
component drift `.16068052`. There are 33 model runs/48 window occurrences,
including the reference calls. Corrected first probe singleton-plus-reference
time is 266.25 ms, mixed batch16 389.17 ms. These are different amounts of work
and padding, concurrent with other checks; they do not establish general
batching throughput. Stored pooling masks exclude padding correctly.

Changing batch shape/composition changes the observed result. Dynamic
activation quantization and kernel choices are plausible explanations, not
isolated causal findings; [ORT documentation](https://onnxruntime.ai/docs/performance/model-optimizations/quantization.html)
explains per-forward-pass dynamic quantization parameters. No relaxed numerical
tolerance is used to declare this equivalent. The first full-corpus comparison
will use singleton inference, with batching deferred to its own fidelity/quality
experiment. No selected production implementation is changed.
Fresh probe repeats agree on every nontiming field, including full pooled and
quantized vectors. The second corrected probe takes 244.81 ms for singleton plus
reference and 384.84 ms for the mixed batch. All 348 research tests pass.

## Reproduction and next step

Use `cqadup_window_preflight.py --root SOURCE_ROOT --snapshot INSTALLED_SNAPSHOT
--output FRESH_JSON`, then `--compare FIRST_JSON`. For `cqadup_batch_probe.py`,
also supply `--preflight FIRST_JSON --preflight-sha256 PIN`; repeat with a fresh
`--output` and `--compare FIRST_PROBE`. Raw public data, ledgers and probe vectors
stay outside Git under mash-artifacts. Neither fixture self-repeat nor the
source audit establishes topic assignments, semantic ancestors or Rust parity.

- Corrected first preflight SHA256: `85d35491d209b95b9a6a385d8c8b4dd2d325e305e5f7cf85bb32468c44a87d0a`
- Corrected first probe SHA256: `c962c62a4425fb6a2f8809c3a9f22702bc9e291218111d131a840b96a397b939`
- Corrected repeat preflight SHA256: `3e61b438b76fab1ad8cae05454b3395be3030855474840f08c0af5e649cde92b`
- Corrected repeat probe SHA256: `268c6dc7d936fc0cbc1d2127d33c42b0e2877c67a33a3814083898072ff3eedb`

The prepared `cqadup_semantic.py` uses all corpus windows, preserving signed
cosines and lexical original-ID ties. Its preregistered controls are title-only,
equal title/body field directions, and fixed RRF60 fusion of balanced cosine
with frozen BM25 full ranks. Within a field, payload-token-weighted unit
directions are averaged and normalized. Record complete unit/field/query vectors,
ranking matrices, build/RSS and warm evaluation costs. Fail explicitly on
canceling or absent evidence; no source omissions, outcome-based mixture tuning
or reserved scoring. Semantic retrieval results, exact full inference repeats
and independent score/metric audits are still pending.
