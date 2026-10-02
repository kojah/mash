# Complete admitted-text windows and coverage correction

## Correction and scope

The recent prototype and pooling reports interpreted 75 absent vectors as an
unfinished embedding cohort. That interpretation was wrong. Every one has an
explicit empty search projection (67 Claude Code, eight Codex); all 74 nonempty
projections have compatible stored vectors. The earlier
[full projection audit](2026-10-02-projection-coverage.md) already established
complete admitted-text coverage across this frozen database. Canonical provider
`user` records include tool/context payloads and are not automatically human
requests. Do not silently embed excluded canonical content. Current rules and
the earlier native corroboration do not classify every one of these 75 records.
The raw source records, exclusion decisions and original baseline are preserved.

This experiment therefore compares first-window and complete-window local
embeddings for the SAME 74 admitted texts, retaining 75 excluded empty sources
without geometry. It does not claim to repair missing indexing. The first-window
arm is a controlled 256-token representation, not proof that production truncates
at that length. Stored production vectors remain a separate historical baseline.

## Frozen procedure

Baseline SHA256:
`367d53bff53eb4186123992b3fbad84fd8ea900bce6829d701dfe58783125cc7`.
Installed quantized MiniLM model SHA256:
`afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1`;
tokenizer SHA256:
`da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0`.
The existing `Encoder` opens explicitly supplied installed assets only; no
downloads, LLM or paid calls. ONNX CPU execution uses one thread and singleton
batches. Dependencies/runtime versions and exact window token IDs/offsets are
recorded privately.

`embedding_windows.windows` retains all original represented characters and
bytes, cuts at whole-word boundaries and uses at most 256 tokens including two
special tokens. Each window receives a mask-aware float32 attended-token mean.
The first-window arm quantizes the first mean. The complete arm combines raw
means using float32 payload-token weights, then quantizes the final source
vector with signed-byte ties-away rounding and float64 normalization. The shared
inference helper also computes unused per-window quantizations; these do not
enter the full-source average. This is a window-mean representation, not full
cross-window attention or new topic-boundary evidence.

All 74 admitted texts span 63,732 UTF-8 bytes. Nine require multiple windows,
with at most 12 windows/message; 65 are single-window. The 129 actual window
inferences are shared between the two controls, not doubled. First windows cover
15,072 bytes (23.65% of admitted bytes). No text is truncated or excluded to fit
the window. Short-text vectors match exactly between controls; long-text
first/full cosine ranges down to .686061, mean .780420.

Apply the unchanged .55 bounded routing and deferred lexical labels to raw
messages and causal units in both arrival orders. Unit formation still breaks
at excluded canonical records; reverse arrival reverses preformed units rather
than each thread's native sequence. Source vectors/text remain separate evidence
for label support and deferred membership. No parameter grid or promotion gate
was adjusted from the observed output.

## Results and interpretation

| Representation | Leaves | Singleton leaves | Weak-label leaves | Forward/reverse pairs | Order pair Jaccard |
| --- | ---: | ---: | ---: | ---: | ---: |
| Stored historical baseline | 57 | 45 | 47 | 37/28 | .710526 |
| Re-embedded first window | 59 | 46 | 48 | 23/22 | .875000 |
| Re-embedded complete windows | 58 | 45 | 47 | 25/26 | .961538 |

Both new controls retain two forward multi-tag sources. Causal units remain 144
and produce exactly the same source co-membership pair sets as raw messages
within each control. Complete windows show a stability signal on this exposed
cohort, but not a fragmentation fix. Pair sets change and are small; higher
Jaccard is not semantic quality, useful ancestry, guided-search benefit or an
independent held-out success. No production topic algorithm is selected.

Two corrected-run outputs match every non-runtime field exactly. The first
repeat checker falsely rejected tuple-versus-JSON-array representation; its
failed run/log are retained. A regression fixture now compares canonical JSON
and excludes only top-level runtime/scheduler observations. The preliminary
run without rusage is also retained; final verified runs have separate names.
`topic_window_audit.py`, importing no producer/window helpers, verifies all
149 sources, 129 windows, contiguous character/byte offsets, exact source
hashes, whole-source concatenated payload-token equality, finite unit vectors,
singleton control equality and empty-source exclusion conservation. It shares
the compiled tokenizer and does not rerun inference or reconstruct routing,
label scores or semantic quality. The 633-test research suite passes in 4.859s.

Verified runs take 5.075/5.527 seconds after input loading and pins; CPU
4.905/5.340s; encoding including model setup 2.272/2.444s; actual window
inference 1.734/1.818s. These include both controls, both arrival orders, raw and
unit refreshes. Target smaps RSS maxima are 230,472/232,256 KiB; PSS
222,564/224,067 KiB; approximate status HWM 230,472/232,796 KiB.
Rusage HWM is 230,100/228,636 KiB; retain possible launcher peaks without
subtraction or workload attribution. External sampling has 271/292 samples,
one vanished read each and maximum gaps 34.00/44.77ms. Maxima are sampled lower
bounds; children/observer memory excluded and monitoring intrusive. These are
research controls, not production inference parity, complete-ingestion
performance or the <=10% throughput-loss gate.

Private final artifacts have prefix `topic-complete-windows-verified-` under
mash-artifacts; the verified audit is
`topic-complete-windows-verified-audit-20261002.json`. Next evaluate unit formation
over admitted user text with excluded canonical context retained separately,
rather than treating every excluded tool/context record as a mandatory topic
boundary. Independent coding quality and full-ingestion/selected-Rust gates
remain open.
