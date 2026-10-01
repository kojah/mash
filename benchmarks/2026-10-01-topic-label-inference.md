# Topic label inference stability

The previous shortlist experiment changed the semantic score of identical
phrases despite unchanged topic discovery inputs. This experiment isolates
raw model inference, batch composition, worker routing, storage quantization,
and the label cache before further c-TF-IDF tuning.

## Controlled installed-model experiment

Xenia CPU, explicitly installed AllMiniLML6V2Q assets, generation
`ca9c7766f955402eb34db3c324da24a512e1118de44e3d9aa9a0faeeac0da5f4`.
No model downloads or source/index mutations. Synthetic phrases and neighbors
contain no private conversation data.

Six phrases are encoded alone and at every position in batches of two and eight
with short and long neighbors. Each batch is repeated; each phrase is then
encoded alone on all eight workers after the intervening batch shapes. A fixed
singleton embedding supplies the comparison center. The report separates raw
f32 output from the application's subsequent per-vector i8 quantization.

All 300 observation metrics reproduced exactly between Cranelift development
and optimized builds. Repeated identical batches agreed within the asserted
`1e-5` component tolerance; singleton outputs across workers were identical.
Every output in a separate 64-message scheduler wave
matched sequential inference with the same batch composition exactly.

| Phrase | Maximum raw component change | Minimum raw cosine to singleton | Range of raw cosine to fixed center | Range after i8 storage |
| --- | ---: | ---: | ---: | ---: |
| deadlock | 0.02730 | 0.98793 | 0.01088 | 0.01120 |
| runtime | 0.02348 | 0.98628 | 0.01876 | 0.01836 |
| runtime confirmation | 0.02740 | 0.98855 | 0.00590 | 0.00627 |
| current uncommitted planning | 0.02424 | 0.98950 | 0.01699 | 0.01641 |
| evidence boundaries | 0.02390 | 0.98602 | 0.01504 | 0.01529 |
| hey | 0.04171 | 0.96599 | 0.01007 | 0.01049 |

Batch composition affects the raw model result before our storage quantization.
The installed FastEmbed dynamic-quantization implementation explicitly warns
about batch-dependent data ranges; this experiment demonstrates the effect in
the actual installed model, without attributing every difference to an
individual ONNX operator. Worker routing/reordering does not explain it.

## Bounded singleton label candidate

The offline topic prototype has an opt-in CPU label inference profile,
`MASH_TEST_TOPIC_SINGLETON_LABELS=1`. It runs singleton model calls in parallel
through the existing eight-worker scheduler, retaining output order and tracing.
The production document embedding path still uses batches of eight; contextual
document projection also retains its existing batch behavior. No index
generation or persisted document embedding format changes.

The label path accepts at most 64 phrases per call and retains the existing
16,384-entry generation-scoped cache bound. Its inference profile is fixed when
the labeler is constructed and recorded in reports. It rejects remote/Core ML
backends rather than claiming untested stability there.

Two 64-phrase waves in opposite orders and a three-phrase subset produced exact
stored singleton reference vectors. Over-cap input fails, and empty input
returns no vectors. These 131 embeddings took 60.9 ms in the optimized
controlled run; that small repeated-phrase test is not a corpus throughput
or production indexing benchmark.

## Frozen corpus comparison

The same 789-thread snapshot is evaluated with both inference profiles, full
and reserved-data discovery, both insertion orders, representative and anchored
discovery, and radii 0.4/0.6: 32 corpus reports plus eight focused label reviews.
Controls reproduce the previous experiment excluding timing/new metadata.
Partitions, eligible/source identities, assignments, ancestor relationships,
provider/thread counts and retained source evidence are unchanged. The audit
independently recomputes counts and inherited memberships.

For the focused reviewed topics, four control label lists differ between the
standalone diagnostic and the complete benchmark, which performs intervening
ancestor-label inference using the same cache. Singleton inference eliminates
all four discrepancies. This is bounded evidence for these reviewed topics,
not an exhaustive review of corpus relevance.

Primary representative/radius-0.4 results:

| Discovery | Order | Batched label time | Singleton label time | Changed label sets | Named leaves, both profiles |
| --- | --- | ---: | ---: | ---: | ---: |
| Full | Forward | 2,317 ms | 3,557 ms | 118 | 409 |
| Full | Reverse, warm cache | 450 ms | 711 ms | 115 | 408 |
| Reserved | Forward | 2,231 ms | 3,204 ms | 99 | 373 |
| Reserved | Reverse, warm cache | 503 ms | 715 ms | 104 | 370 |

The forward runs encode 8,977/8,285 unique candidates respectively; reverse
runs reuse the cache and encode 1,435/1,548 missing phrases. Warm timings are
therefore not directly comparable with cold vocabulary runs. All-eight-config
full runs took 20.27 s batched and 21.26 s singleton; reserved runs took 19.93 s
and 21.90 s. Peak RSS across runs was roughly 1.05–1.10 GiB. These are single
contended offline runs, not a production speed guarantee.

The domain-shortlist omission remains: the forward research-pipeline topic
still excludes concrete domain terms. Singleton inference also consistently
admits a greeting in one reviewed topic. The stable profile is retained as an
explicit experiment, with no claim of improved relevance or automatic adoption
as the default. It supplies a reproducible baseline for the accepted support
and filler changes.

## Reproduction and limits

Ignored real-model checks:

- `semantic::embeddings::cpu::sensitivity::benchmark_label_embedding_sensitivity`
- `semantic::embeddings::cpu::tests::cpu_scheduler_preserves_quantized_batches_and_message_order`
- `topics::evaluation::review::benchmark_label_candidates`
- `topics::evaluation::benchmark_topic_discovery`

The controlled diagnostic requires `MASH_TEST_MODELS_DIR` and an explicit
`MASH_TEST_TOPIC_REPORT_DIR`; it creates its report exclusively and refuses to
overwrite a previous capture. Corpus diagnostics additionally require the frozen
SQLite snapshot, selected thread TOML and probe TOML. Private captures remain
outside Git under `mash-artifacts/topic-label-sensitivity-20261001`.

Validation: 548 regular tests, 77 focused topic-related tests, both installed
CPU model checks, all 32 corpus reports and eight reviews, all-target/all-feature
Clippy, doctests, formatting and diff checks passed. No shared backend behavior
or production topic API is introduced by this experiment.

Stable label vectors are a prerequisite for trustworthy ranking comparisons,
not proof of label relevance or coherent topics. The accepted next quality work
combines c-TF-IDF distinctiveness, support across threads, semantic relevance and
conversational filler filtering. Broad source-grounded precision and secondary
subject recall, native Core ML validation, lifecycle/persistence and production
MCP topic tools remain incomplete.
