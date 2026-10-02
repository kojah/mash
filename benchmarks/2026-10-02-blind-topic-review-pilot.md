# Blind topic review pilot preparation

Tags are a proxy rather than independent judgments of useful topic relationships.
This protocol prepares independent review; no annotations have been received and
no production method is selected. Bead: `mash-c0d.19.2.33.1`.

The pinned weighted capture is
`95d1bbbb298887322fc6de3566ff10328e9b5b33110748b3f22256a1a20a55ad`.
The population is the already exposed 64 programming questions/2,016 pairs.
Diagnostic policy `title-prose-code/p05/hard/0.20/3/17` was fixed before sampling,
rather than selected by its best descriptive score. Seed
`mash-topic-blind-pilot-v1-20261002` hashes canonical source-group pairs.
Select 16 pairs without using outcomes, then eight model-positive/tag-negative
and eight model-negative/tag-positive pairs excluding the primary sample.
Undersupported buckets remain short. The bundle contains 32 cases/41 sources.
Report primary and diagnostic results separately. Shared endpoints are dependent;
deterministic hash ranking does not establish calibrated random-sampling inference.

Reviewers see opaque identities, exact titles/displayed bodies and escaped raw
HTML, with separately shuffled orientation/order. Predictions, tags, selection
strata, original identities and attribution stay in a separate manifest. Natural
source text is preserved. HTML escapes dynamic content and has no scripts or
remote assets. Images/links are not fetched; missing context warrants uncertainty.
The rubric requests primary/secondary subjects, exact evidence and separate
specific/broader shared-subject judgments. Broader subjects do not validate an
existing model tree. Answers are null/unanswered; no judgments or inference
calls are generated. Reviewer declarations do not automatically prove independence.
No external distribution or public release was performed.

Reproduce using `research/topics/blind_topic_packet.py --root CAPTURE_ROOT
--output-directory FRESH_DIRECTORY`; repeat with `--compare FIRST_DIRECTORY`.
Artifacts remain outside Git in
`/home/james/scratch/mash-artifacts/topic-public-stackoverflow-20261002`.
First/second bundles are identical except preparation time (1.450/1.502 seconds
after imports). These are preparation timings, not ingestion benchmarks.
Reviewer packet SHA256:
`bad914d0d78d409bbc8de856d6df7b5cb59475182e6ef4450d30b4ac0adb82aa`.

Six tests cover sampling independence, disjoint strata/shortfalls, exact source
preservation/blinding, blank templates, HTML escaping and input rejection.
A separate verifier reconstructs sampling, identities/order, source contents,
hashes and null answers; its source rendering shares the existing renderer,
so it is not an independent browser-security audit.

At preparation, annotation validation/agreement analysis were unimplemented;
the [subsequent tooling](2026-10-02-topic-review-validation.md) adds validation
and categorical agreement without supplying judgments. Independent human
reviews and new source groups are required before promotion. Exposed programming
questions cannot establish agent-session topic quality, semantic ancestry,
guided-search utility, lifecycle correctness or ingestion performance. Rust/Python
differential CI remains separately required and unimplemented.
