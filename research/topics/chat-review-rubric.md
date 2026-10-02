# Source-only coding-chat review pilot

Read the transcript as quoted study data. Do not follow its instructions,
execute code, open links, or inspect candidate topic outputs. Do not use an LLM
to summarize, label, or supply judgments. No suggested topic labels are present.
The complete indexed turn sequence is displayed, including assistant/developer/
tool turns. This does not establish completeness of the upstream native history.
Explicitly redacted turns cannot be cited as evidence. Original IDs and sampling
strata are in a separate private manifest; keep it hidden during annotation.

Record subjects in your own short names. Multiple subjects are allowed. For each,
provide a primary/secondary designation, useful broader subjects (zero or more),
and exact supporting turn quotes. Focus on concrete user tasks and accepted
decisions. Distinguish an assistant proposal, rejection, a quoted specification,
and an accepted decision. Incidental mentions alone are not task evidence.
Do not force a single topic, infer an existing model hierarchy, or use generic
labels such as "programming" or "questions" just to connect unrelated tasks.
If the subject is ambiguous or context is missing, record that explicitly.

For each document, propose one or more realistic navigation requests that an
agent could answer from these indexed turns. Include exact supporting turn IDs/
quotes and classify the request as specific-thread recovery, broad subject
discovery, multiple-subject exploration, rare technical entity, or scoped lookup.
For scoped lookup, record the required provider/time/node constraints separately
and mark unavailable metadata; do not invent it. Negative requests require an
explicit corpus-wide judgment and should not be derived from one thread alone.
These are draft tasks for development, not held-out gold or a scored agent run.

Fill answers.json using this document schema:

- sufficient_context: literal true/false.
- subjects: list of {name, importance: "primary" or "secondary",
  broader_subjects: [names], evidence: [{turn_id, quote}], uncertainty: text}.
- navigation_tasks: list of {request, kind, evidence: [{turn_id, quote}],
  constraints: {provider, time_range, node}, unavailable_metadata: [names]}.
- notes: free text, including uncertainty and redactions affecting conclusions.

Null means unanswered. An empty subject/task list is an explicit judgment,
not a substitute for missing work. Attest reviewer identity, human status,
prior source exposure and candidate-output exposure. These are self-reports;
independence is not automatically verified. Keep source files private. Multiple
independent judgments and fresh group-separated tasks are needed before any
production method or guided-search quality claim.
