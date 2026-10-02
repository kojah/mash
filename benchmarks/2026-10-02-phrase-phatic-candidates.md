# Candidate-only phatic filtering: small effect, no topic-quality promotion

## Fixed comparison

Reuse the original eight-thread exposed review (68 oracle-admitted records,
nine primary/nine secondary subjects) and separate frozen 128-thread,
1,850-source cohort. The familiar single-assistant review is not independent
human gold. Both previous arms, complete snapshots, support summaries and
lifecycle checks must match the pinned streamed allocation baseline exactly.
No reselection, embedding inference, LLM, download or paid call occurs.

Add one explicit enum-selected research arm. Before source-unit shortlisting,
exclude any candidate phrase containing an exact atom from this fixed set:
`yes no thanks thank please hello hi hey greetings bye sorry actually basically
anyway anyways lol lmao alright okok kk uhh umm hmmm`. The existing scanner's
English stopwords and acknowledgement exclusions remain unchanged. This is a
small candidate-only lexical control, not general content classification or
context resolution. The list is fixed before results; no output-driven tuning.

Retain all original source texts/identities, vocabulary and scoring statistics,
the top-two source shortlist, half weight/coverage utility, containment, ties,
eight-tag bound and literal evidence. Do not remove short records or impose a
minimum token count. A surviving technical singleton remains eligible. Exact
atom matching preserves identifiers such as `no_std`, `hello-world` and
`yes_client`; it does not substring-match them. It can still remove a meaningful
standalone word, and uncatalogued conversational terms remain eligible.
Candidate exclusion does not reconstruct bigrams or reinterpret references.

## Results

| Diagnostic | Whole-thread | Source coverage | Phatic-filtered coverage |
| --- | ---: | ---: | ---: |
| Primary evidence recall | .928571 | .986111 | .986111 |
| Secondary evidence recall | .481481 | .796296 | .796296 |
| Primary best single-phrase support F1 | .734512 | .734512 | .734512 |
| Secondary best single-phrase support F1 | .385714 | .559788 | .559788 |
| Primary / secondary name-atom hits | 5 / 3 | 3 / 5 | 3 / 5 |
| Secondary subjects without evidence / 9 | 4 | 1 | 1 |
| Reviewed referential-only topics | 4 | 10 | 9 |
| Reviewed topics / tags | 63 / 63 | 48 / 48 | 48 / 48 |
| Broad topics / tags | 618 / 1,007 | 442 / 720 | 442 / 718 |
| Broad recurring / provisional topics | 309 / 309 | 291 / 151 | 288 / 154 |
| Broad sources without evidence / 1,850 | 1,276 | 769 | 776 |
| Broad multi-tag sources | 326 | 413 | 411 |

Filtering changes three of eight reviewed thread tag lists and nine of 128
broad lists compared with unfiltered coverage. All threads remain tagged.
Broad literal coverage slightly decreases, 1,081 to 1,074 sources. Primary
and secondary evidence metrics remain exactly unchanged. Referential-only
topics fall by only one; that count is a risk proxy, not confirmed errors.
Meaningful contextual references can have exclusively referential evidence.
The result does not demonstrate semantic label quality, membership precision
or improved agent search. Any phrase on a multi-topic support record can count
toward recall; best support F1 and name overlap are also proxies.

This fixed list does not solve the remaining problem. Preserve the negative/
limited result rather than expanding the list to fit these conversations.
Next evaluate general-background lexical specificity on the frozen candidates,
with technical-term preservation and independent label usefulness gates. Pure
referential context still needs justified resolution or abstention. No method,
Rust port, MCP integration or production default is selected.

## Verification

Two complete runs match all non-runtime fields. Both original controls and
their lifecycle summaries match the prior baseline. The new arm passes refreshed
checkpoint/restart, one exact duplicate per thread (8/128), invariant lexical
votes and original tags/support with expanded evidence identities, cleanup,
actual source deletion/absence, deleted restart, reinsertion and reverse final
semantics. This is producer lifecycle verification, not independent replay.

The no-producer-import auditor independently reads the immutable SQLite source
and original review packet. It verifies baseline identity and unchanged arms;
recomputes term counts, weights, exclusions, shortlists, coverage votes,
utilities/ties, ranks, phrase identities, provider/thread/source unions and
review metrics for all six snapshots. It checks 2,604 selected scores/ranks
with zero error, plus 18 subject support sets per reviewed arm with maximum
F1 error 1.11e-16. Static English stoplist, declared phatic contract and input
sources are shared dependencies; independent native parsing, semantic gold and
lifecycle replay are outside the audit.

The full 657-test suite passes in 4.251s. New fixtures cover phatic abstention,
brief technical sources and exact identifier boundaries. The corruption fixture
now checks all three policies. An initial identifier fixture incorrectly expected
three candidates from one source despite the two-candidate bound; its failed
capture remains. The corrected boundary fixture isolates one identifier per
source without changing the algorithm or scientific outputs.

## Cost and artifacts

Reviewed refresh/evaluation takes 16.61/16.41ms whole-thread, 15.01/14.97ms
coverage and 18.67/16.71ms filtered coverage. Broad refresh takes
3,141.97/2,989.24ms, 3,303.51/3,215.32ms and 3,076.04/3,299.59ms respectively.
Whole experiment after reviewed loading, including broad loading and both
alternatives' duplicate/delete/restart/reverse checks, is 57.464/56.724s wall,
57.218/56.437s CPU. Imports, initial reviewed loading, serialization and repeat
comparison are excluded; this is not full ingestion or isolated update latency.

Sampled target RSS/status HWM is 189,252/189,532 KiB, PSS
179,143/179,522 KiB; pre-serialization rusage 189,108/188,988 KiB. There are
2,252/2,206 samples, one vanished read each and maximum gaps 44.52/44.82ms.
Sample maxima are lower bounds, status approximate, observer intrusive and
children/observer excluded; launcher history can affect rusage. Do not subtract
peaks or attribute the whole process to an isolated topic worker. The <=10%
complete-ingestion overhead target remains untested.

Private result SHA256:

- First: `76180b4fbb509a45bb2529fa9d3b09bf1bce4b5ada347caf799594215fa1e9a5`.
- Repeat: `b605c5c71bdc63070d8166534c9595037b73c34f669d44c833cf55388a312ada`.

Artifacts use `phrase-phatic-{first,repeat}-20261002`, memory sidecars/logs,
`phrase-phatic-audit-20261002.log` and final test log under mash-artifacts.
Frozen input/code/baseline identities are embedded in results; raw sources,
review labels and credentials remain outside Git.
