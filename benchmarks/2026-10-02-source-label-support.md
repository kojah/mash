# Original-message label support on unchanged lexical groups

P0 `mash-c0d.19.2.20.8.3.3.3.9.1`, Python-only controlled research.
[Frozen protocol](../research/topics/SOURCE_LABEL_SUPPORT_PROTOCOL.md).
No production topic method selected; no Rust/application/deployment change.

## Question and control

The preceding audited label experiment found 516 single-physical-block groups,
all empty under a >=2 member-block gate; 359 contain multiple original ready
messages. Does support need to be separate from clustering granularity?

Hold the 162 training meetings, 88,336 source IDs, 84,434 ready sources,
77,892 singleton-known sources, 8,196 physical blocks, lexical-average trees,
fixed-eight display groups, original word counts and static stoplist unchanged.
Both arms use identical c-TF-IDF times corpus block IDF scores, letter rule,
<=40% global block DF, top eight and lexical ties. Change only the >=2 distinct
member-block gate to >=2 distinct original-message IDs. Corpus background,
class lengths and TF/IDF denominators remain physical-block based. Global
background is an initial snapshot, not implemented causal online maintenance.

`source_label_support.py` rejects duplicate source identities, invalid ownership
and incomplete group partitions. Count source presence, not token repetitions
or duplicated context. Raw-fallback blocks grant no label evidence. Source IDs
and the first two original supporting IDs accompany each label. These are
support examples, not representatives or independent human judgments.

No inference, model/download/LLM, custom stopword, parameter sweep, gold input,
held-out source or membership change. Native topic names enter only after ranks
are fixed. Exact token overlap is a weak conditional proxy: paraphrases can
miss and generic words can falsely hit. Unknown/mixed sources still supply
word evidence; 6,542 remain outside conditional native-name scoring. No empty
native-name token sets. Meeting annotations are not agent-session multitag gold.

## Results

| Support gate | Hits / 77,892 | Name overlap | Empty / 1,296 | <2 terms | Terms |
|---|---:|---:|---:|---:|---:|
| Physical blocks | 21,680 | .278334 | 588 | 693 | 4,183 |
| Original messages | 21,478 | .275741 | 360 | 482 | 5,398 |

Message support recovers 228 empty groups (207 single-block groups) and moves
211 groups from fewer than two terms to at least two. Label term lists change
in 580 groups. Remaining empty groups include all 157 one-source groups and
203 multiple-source groups. 2,250 selected message-supported terms have exactly
two source supporters. Repeated words in a single source cannot pass the gate.

Availability improves, but the weak name proxy declines slightly; no
significance or semantic inferiority claim. Examples newly eligible in meeting
2 include `informal/range/institutions/information/things/help/quite/use` and
`specific/year/yes`. Meeting 3 includes `mike/ear/plug/huh/good` and
`german/spanish/dutch/native/nsa/non/ones`. Other recovered proposals include
`gap`, `didn/got`, and `know`. Not all recovered labels are useful topics.
Do not promote message support as a semantic-quality solution or weaken other
gates. Keep explicit provenance/weak cases in subsequent label evaluation.

## Reproducibility and independent audit

Artifacts outside Git, prefix `/home/james/scratch/mash-artifacts/`:

- `topic-qmsum-source-support-first-20261002.json`, SHA256
  `c32f7abd20ff9950097a4ad3f2d95cf23839cf91f7cab1e19ba673eb6b04e5e9`.
- `topic-qmsum-source-support-second-20261002.json`, SHA256
  `c04e3f7c0f7521db83b3e7a0e88d7dc369572590f8d8ddd7b5a6315aaf92a486`.
  Every non-timing field repeats exactly; begin/end code pins are unchanged.
  Block control exactly replays all previous groups/states/ranks/scores/DF.
- `topic-qmsum-source-support-audit-20261002.json`, SHA256
  `012c2613324ecf58d5a0a25db251d4c712aaa4a2c7a73a92a2bd49330c6739eb`:
  independent original-row
  presence/count accumulation, scalar score/DF/rank, fixed-cut reconstruction,
  source-support IDs and native annotation/name-token evaluation. Checks 9,581
  terms, 9,581 examples and 168,868 repeated source-proxy contributions.
  Maximum numeric error 8.88e-16 within frozen relative/absolute 1e-12.
  Auditor source SHA256
  `018b4893b412186f6cca26cf537548372f02aa7eff683c8372777a5f020e639a`.
  Shares pinned source count ledger/NumPy/SciPy/sklearn static stoplist; original
  tokenizer previously audited. Implementation consistency, not semantics.

Four new fixtures cover original-message recovery, fixed-score/rank replay,
duplicate context/incomplete ownership rejection, repeated-token abstention,
unready sources and fallback. Full enabled research suite 556 tests.

## Measured scope

First/repeat after-import wall 43.857/32.690s; CPU 27.748/26.811s;
setup 27.408/17.145s. Background DF 30.472/66.315ms. Across 162 meetings,
block-gated label totals 2.135/1.997s; message-gated 2.156/1.920s. Each arm
rebuilds physical counts and validates unique ownership; these are comparison
costs, not an optimized production label path. Large validation inputs include
unused raw embeddings/context/IDF ledgers. Proxy, provenance and capture work
is in the full target. Main timer excludes imports, final JSON/repeat compare;
external sampler includes them. Host load uncontrolled; no speed-win claim.

Rusage peaks 2,084,736/2,084,872KiB. External RSS/PSS maxima
2,086,088/2,073,272 and 2,085,864/2,072,794KiB; approximate status HWM
2,086,088/2,085,864KiB. Samples 949/864, vanished 1/0;
median/max gaps 41.559/200.721 and 38.660/130.489ms.
One target excludes children and observer; monitoring intrusive, sampled maxima
lower bounds, status counters approximate, launcher rusage may retain high water.
No subtraction or attribution to a production worker. Scientific runs sequential;
brief first-capture inspection overlaps repeat setup.

Independent coding/semantic labels, multitag membership, semantic ancestors,
guided-search gains, shared/global online lifecycle, edit/delete/restart,
complete ingestion <=10% overhead and selected executable Python/Rust parity
remain open. More populated labels are not evidence of production readiness.
