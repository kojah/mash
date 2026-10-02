# Source-preserving fixed-membership label cleanup

This label-only ablation uses the complete prior QMSum seed partitions, including
their known discovery failure. It does not refit groups or embeddings and cannot
establish improved recurrence, semantic ancestors or coding-session quality.

## Frozen procedure

All 162 meetings/88,336 raw source turns remain intact. Pin discovery SHA256
`83d0b902b1833218a8be86b9888c35702e86808f5886a469facd369f8e1cc3d6` and
original count/vocabulary/IDF capture SHA256
`e3c89677f538f65f3e23a5a7de14de6ef9340febb40a408d97ebb275cb11b488`.
Keep the four source/context forward/reverse partitions fixed, with all source
ownership and unsupported rows preserved. Compare exactly three policies:

1. Raw source counts and original labels, required to match prior labels exactly.
2. Subtract only count contributions from exact case-sensitive braced
   `{disfmarker}` / `{vocalsound}` occurrences. Preserve plain words and other
   case variants. Record every original character and UTF-8 byte span.
3. The same marker subtraction plus label-only scikit-learn 1.9.1
   `ENGLISH_STOP_WORDS` and the frozen acknowledgements
   `yeah/okay/ok/uh/um/erm/hmm/mm/oh` (327 excluded terms total).

Recompute class lengths and frequencies for each count policy; retain the
original whole-training source IDF, vocabulary, scoring formula, top-eight
budget and lexical tie rules. No gold-derived filtering, further stoplist/grid,
model inference, download or held-out read. Raw source text and search/discovery
embeddings do not change. Retain empty source rows and empty groups without
invented labels. These are exposed-development choices motivated by observed
descriptor noise, not a held-out confirmatory evaluation.

## Coverage and lexical proxies

| Label count policy | Nonempty source rows | Count mass | Removed count mass |
|---|---:|---:|---:|
| Raw | 88,049 | 1,217,662 | 0 |
| Exact markers | 84,234 | 1,167,534 | 50,128 |
| Markers + vocabulary | 60,157 | 448,965 | 768,697 |

The exact 33,979 `disfmarker` and 16,149 `vocalsound` braced occurrences differ
from the earlier case-insensitive token-context scan (33,979/16,301). The policy
counts only exact braced lowercase forms; source counts/offsets are independently
verified. Do not silently equate all token mentions with removable markup.

| Discovery | Label policy | Empty groups | Raw-name overlap | Content-name overlap* |
|---|---|---:|---:|---:|
| Original forward | Raw | 0 | .015927 | .009667 |
| Original forward | Markers | 9 | .016430 | .009635 |
| Original forward | Markers + vocabulary | 3,113 | .008170 | .010851 |
| Original reverse | Raw | 0 | .015864 | .009656 |
| Original reverse | Markers | 3 | .016118 | .009624 |
| Original reverse | Markers + vocabulary | 3,061 | .008190 | .010866 |
| Context forward | Raw | 0 | .017147 | .018845 |
| Context forward | Markers | 128 | .017335 | .018728 |
| Context forward | Markers + vocabulary | 1,193 | .015496 | .020834 |
| Context reverse | Raw | 0 | .017054 | .018707 |
| Context reverse | Markers | 55 | .017172 | .018609 |
| Context reverse | Markers + vocabulary | 634 | .015559 | .020798 |

Every raw-name proxy retains the original token denominator and all 82,231
topic/source incidences per policy. *Content-name overlap is explicitly post hoc:
after seeing lower raw overlap, exclude the same frozen label stop vocabulary
from reference topic-name tokens only. No refit or descriptor change occurs.
There are no empty reference incidences here. It remains lexical surface overlap,
not independent semantic label evaluation. The raw proxy rewards function words
such as `the/and/of`; its decline does not demonstrate worse semantic labels.

Exact-marker policies eliminate marker-named descriptors; the broad policy
eliminates excluded-vocabulary descriptors by construction. Those absences are
not independent quality measures. Marker-only original-forward descriptors fall
from 349,902 to 341,716, with nine empty groups; broad filtering leaves 221,903
descriptors and 3,113 empty groups. Empty groups may be appropriate abstentions
for acknowledgements, rather than errors; evaluate that policy before adoption.

Decision: keep known-markup removal as a source-preserving candidate. Broad
vocabulary filtering has modest content-proxy gains and large coverage changes;
neither automatic proxy selects a production label method. Do not infer that
stopword deletion improves document embeddings, or that fixing labels repairs
the rejected lexical seed discovery. Representation cleanup/refitting and actual
independent semantic quality remain parent work.

## Verification and reproduction

Drivers `research/topics/qmsum_label_cleanup.py` and `label_noise.py`. Four new
tests check exact/plain/case variants, repeated count subtraction, Unicode
character/byte offsets, immutable source counts, empty rows and failure on
inconsistent evidence. Full enabled lab suite: 453 tests pass.

Final first/repeat SHA256:
`064ff3731955b515a07e12e3897102e2cd9d137ac91c43ec222e393a2e492d08` /
`e85b8fe91956f7f45ab5b83499895905f2c33695b3579bb7d3fe189801e2a262`.
Every nontiming field and saved CSR array hash matches exactly. Artifact roots:
`/home/james/scratch/mash-artifacts/topic-qmsum-label-cleanup-{first,second}-20261002`.
Use existing science Python and `memory_audit.py --output MEMORY.json -- PYTHON
research/topics/qmsum_label_cleanup.py --source SOURCE --baseline BASELINE
--discovery DISCOVERY_RESULTS --output-directory FRESH [--compare FIRST/results.json]`.

Independent original-source audit terminal success covers all 88,336 turns,
50,128 exact marker spans, 1,944 label runs and 2,859,357 scalar descriptor
scores with zero error. It reconstructs raw counts, literal-search offsets,
filtered counts, ownership, source support, ranking and summaries without new
helper imports. Auditor `topic-qmsum-label-cleanup-audit-20261002.py` SHA256
`835930151c6c9f486c09c66c5b725176a255465cd13ae06bea4cf1442d8ca012`;
terminal report `topic-qmsum-label-cleanup-audit-first-20261002.json`.
Post-hoc content proxy script SHA256
`156ac0ade27d5ac9dee8e43c5b639f5bcc99458ce4a1bb874eecfe6fd96e4c50`,
report `topic-qmsum-label-content-proxy-first-20261002.json`. No independent
inference/Rust parity or semantic review is supplied by these arithmetic audits.

## Cost scope

First/repeat after-import setup 4.804/4.550 s; count cleanup .405/.374 s;
complete offline comparison 36.022/34.002 s; CPU 33.220/32.001 s. This labels
three policies across four entire fixed partitions and retains full ledgers.
The timer excludes repeat comparison/final JSON, imports and the post-hoc proxy.
Repeat overlaps the independent auditor; these are not matched speed comparisons.
No complete ingestion, online label maintenance, or production memory gate.

Rusage 1537584/1537280 KiB; external full-target RSS 1537944/2895424 KiB,
PSS 1530160/2886035 KiB, status high water 1537896/2895140 KiB. Repeat comparison
loads another complete output ledger after the timer, enlarging external scope.
Sampling: 1387/1784 observations, zero/one vanished reads, median/max gaps
35.49/57.30 and 35.53/82.69 ms. External scope includes imports/comparison/JSON
and excludes observer/auditor/proxy memory. Sampled peaks are lower bounds,
status approximate, monitoring intrusive, launcher high water may persist.
Do not subtract peaks or assign offline retained-ledger memory to production.

Beads `.19.2.20.8.2.1` completes only this label-only experiment;
parent `.8.2` retains representation/refit and semantic validation work.
