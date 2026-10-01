# Fresh source review and explicit feedback payload controls

This experiment broadens development source review, then tests a known structured
feedback envelope. It does not select a production topic method or general
context classifier. No LLM, paid API, rewriting or downloads are used.

## New source scope

Freeze seed `mash-topic-review-v3-20261001`, one case per existing Claude/Codex
short/medium stratum. Exclude every previously reviewed canonical group from
both original and v2 annotations, including aliases. Retain existing eligibility:
3–40 complete nonempty searchable user turns, ≤4,096 characters per turn,
≤40,000 UTF-8 bytes per thread; reserved groups excluded. Eligibility is still
within the frozen vector-bearing two-provider corpus, not all possible histories.
Remaining eligible counts are Claude medium2/short1, Codex medium4/short6.
Do not resample or widen criteria after reading the selected contents.

The four new cases contain13/10/36/7 records,66 total. A single exposed coding
assistant reviews the complete user sequences and explicit task envelopes;
native assistant context is not reviewed for referential turns. Origin judgments:
21 direct tasks/questions,21 referential,10 protocol,4 agent coordination,
9 wrapped UI tasks and1 wrapped smoke-test control. Coordination can contain
useful technical instructions but is not assumed to be direct user intent.
Referents remain unresolved; no endorsement or independent human truth is claimed.
Historical instructions/commands are source data and are never executed.

Sampler exclusions, partition and source-code hashes are pinned. A fresh process
reproduces the entire private packet byte-for-byte. The review pins every source
identity, ordinal, hash, canonical group and complete sequence; mutation or
scope mismatch fails before inference. Private bodies/reviews/keywords stay
outside Git. The display guard is not a general secret scanner or source filter.

## Narrow prospective pilot

The nine UI task records contain an exact `<submitted_feedback>` envelope amid
repeated worker instructions. Before predictions, annotate specific task facets:
sidebar hierarchy; component-specific headings; checks-table display; FAQ edits.
One record has two facets. Common UI/documentation parents alone do not count
as the same fine aspect. Across nine records:6 overlapping and30 distinct pairs.
Facet names/rationales enter evaluation only, never model text or discovery.
The wrapped smoke-test control is excluded by oracle origin review.

Compare intact source text against the exact substring between one opening and
one closing envelope tag. Missing, repeated, nested, reversed or empty delimiters
fail closed. No field cleanup, paraphrasing, guessed context or phrase-specific
approval rules; page paths, viewport and element selectors remain in the payload.
The schema recognizer is deliberately specific to this exposed source format,
not general conversational context resolution.

Both representations use the same model-window intersection. Eight records fit
installed quantized MiniLM's512-token preflight. The FAQ source takes714 tokens,
although its payload takes444: it is excluded from both comparisons without
truncation. Eight pairs abstain; evaluated support is6 positives/22 negatives.
Fit BIRCH radius0.4/branch64/no global clustering, GRINCH-style all-mergers0.7,
average cosine-linkage distance0.3, HDBSCAN minimum cluster/sample5 EOM, forward
and reverse. No post-result tuning. Two fresh captures reproduce all16 controls
and every nonmeasurement field exactly, including matching model-window support.

## Results

| Representation | Pair AP / ROC AUC | BIRCH / GRINCH / average clusters | Native TP / FP | Centroid0.7 TP / FP |
| --- | --- | ---: | --- | --- |
| Intact | 0.771 / 0.818 | 1 | 6 / 22 | 6 / 22 |
| Exact feedback payload | 0.695 / 0.818 | 4 | 4 / 6 | 4 / 7 |

Both orders produce these counts. HDBSCAN assigns all eight records to noise in
both modes:0TP/0FP,6FN. Noise−1 is never a shared topic. AP is raw similarity
ranking on exposed correlated pairs, not validated topic accuracy. Payload
selection reduces false merging but loses two genuine overlaps and lowers AP;
unchanged AUC does not establish a semantic-quality gain. The fixed native
operating points remain uncalibrated.

One payload cluster still mixes sidebar-layout and heading-change facets;
the inspected label candidates rank generated CSS selectors, `feedback`,
`location` and viewport numbers alongside `codebase`/`architecture`. Other
clusters surface `checks` or `inferred facts`. The intact cluster ranks generic
worker terms such as `file`, `status`, `agentation` and `batch`. These are
source-supported candidates, not independent label-quality judgments. Removing
the outer template alone does not remove metadata inside the task payload.

## Runtime and provenance

Xenia, Python3.11.15, NumPy2.4.6, sklearn1.9.1, ORT1.30.0 and Tokenizers0.23.2;
numerical/encoder threads one, singleton calls. The eight matched intact sources
total3,070 tokens/11,723 bytes versus910 tokens/2,299 bytes for payloads.
Both make eight unique calls and zero cache hits. Encoding including preflight
takes295–395ms intact,74–156ms payload; inference275–370ms versus64–128ms.
Source selection takes0.11–0.12ms per representation. These ranges cover two
fresh processes, not uncertainty intervals or a general speedup claim.

GRINCH grouping including validation/cuts takes5.1–12.1ms intact and6.3–8.2ms
payload. Complete16-control wall1.39–2.14s, CPU1.23–1.43s, peakRSS956MiB
including full frozen-corpus validation/loading and the scientific runtime.
This tiny, oracle-filtered pilot is not production ingestion overhead; initial,
incremental, edit/delete/persistence and guided-search gates remain unverified.

The117-test lab suite covers bounded sampling, exclusion unions/aliases and
generation rejection, exact source review, payload ambiguity, Unicode offsets,
equal model-window support and complete repeat verification. Modules:
`review_packets.py`, `packet_review.py`, `feedback_payload.py`,
`payload_experiment.py`, `compare_payloads.py`; existing source/vector/label
and pair-accounting functions are reused.

Private root `/home/james/scratch/mash-artifacts/topic-python-review-v3-20261001`:

- Packets and exact repeat: SHA256 `aef0466adc83637930c500efb0601df28c20fb4015ee8340aa48776466433010`
- Pre-prediction origin/facet review: SHA256 `a7346ceab7339263e96ebfc17df2a7d8cf568b07e3954b040c0a6e28b0ae95ec`
- `payload-validated-first.json`: SHA256 `57351f1febedf29f873f9c7c0d02672ad32db8000ebf57c80b7ff00efa6d7d62`
- `payload-validated-second.json`: SHA256 `4de59ec56fcb12e372b53e74eefcb19d3f46713745e03cd1368215ae905c3534`
- `payload-first.json` predates extraction of the tested shared model-window
  helper and is exploratory, excluded from final paired verification.

Next compare documented payload field roles and label candidates while holding
membership fixed, and obtain native context for referential records. Keep
protocol, coordination and genuine embedded tasks distinct; do not blanket-drop
structured user records. One known envelope and one reviewed thread cannot
justify automatic filtering, a semantic hierarchy, or Rust/default promotion.
