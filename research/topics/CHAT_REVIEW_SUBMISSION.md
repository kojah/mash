# Coding-chat review submission contract

The captured packet and blank template stay unchanged. `chat_review.py` parses
their known JSON schema into frozen typed models (`chat_review_models.py`).
The reference validator is standard-library only; it does not load model or
scientific dependencies and does not generate or fill judgments.

Run `python3 research/topics/chat_review.py --packet PRIVATE/review.json
--answers PRIVATE/completed-answers.json`. Complete every document with literal
booleans and explicit lists/notes. Blank null values are rejected, not counted
as uncertain judgments. Empty subject/task lists require explanatory notes.
Insufficient context requires notes, explicit subject uncertainty, and no
purportedly answerable navigation tasks. Multiple primary subjects are allowed;
nonempty subject lists need at least one primary. Broader-subject names are
reviewer proposals, not a verified tree or automatic ancestor assignments.

Each subject/task needs nonempty exact quotes from unredacted turns in the same
document. Turn IDs, source hash, packet hash and complete document coverage are
checked. Duplicate JSON fields/identities/evidence, unexpected keys, malformed
types/enums, missing decisions and invalid dates fail. Matching a quote proves
provenance, not that the interpretation is semantically correct. Assistant/tool
text is source evidence, not automatically an accepted user decision.

`importance` is `primary` or `secondary`. `kind` is one of:

- `specific-thread-recovery`
- `broad-subject-discovery`
- `multiple-subject-exploration`
- `rare-technical-entity`
- `scoped-lookup`

Constraints have exactly `provider`, `time_range`, `node`, each null or an
explicit requested filter. Provider/node are nonempty strings. Time range is
null or `{start, end}`, with at least one nonnull ISO timestamp including
timezone, valid dates and ordered bounds. A scoped lookup needs a filter.
The blind packet has **no authenticated provider/time/node metadata**.
Therefore `unavailable_metadata` must name every requested nonnull constraint
(`provider`, `time_range`, `node`), exactly once. Such draft tasks remain
incomplete for scored scoped retrieval. Do not interpret requests as source
facts or invent metadata to remove that gap. Corpus-wide negatives are not
accepted task kinds for this single-chat pilot.

Human status and prior-source/candidate-output exposure declarations are
required literal booleans; human attestation must be true. This is a self-report,
not automatic verification. Aggregate output never reports identities, labels,
requests, or quotes; errors avoid private source fragments. Even an accepted
submission sets independent gold/semantic quality/agent utility to unproven.
Free-text label agreement needs human alignment/adjudication; do not treat
literal name equality as semantic agreement or compute misleading kappa over
unmatched subject vocabularies. Actual independent reviews and genuinely fresh
group-separated task data remain required. Synthetic test fixtures are not
actual annotations and do not populate the private packet.
