---
name: cli-ux-acceptance-testing
description: Use when dogfooding or auditing a CLI through independent isolated clean-room journeys, transcript review, perceived-slowness follow-up, or ranked UX-friction synthesis.
---

# CLI UX acceptance testing

Capture what a user expects, attempts, observes, and does next. Preserve novice
signal before consulting implementation context. This workflow produces
exploratory UX evidence, not behavioral-test verification.

This workflow needs fresh subagents: concurrent agents that inherit none of
this conversation and see only the prompt you give them. Stop if the host
cannot start them or the repository profile described below is unavailable.

| Host | Start a fresh subagent with |
|---|---|
| Codex | a spawn with `fork_turns="none"` |
| Claude Code | the Agent tool and a non-fork `subagent_type` such as `general-purpose`; several calls in one message run concurrently |
| Other | any spawn that passes only the prompt you write |

When the host does not load skills into subagents, give them this skill's
path to read.

## Repository profile

For live work, read `.agents/ux-acceptance-testing/PROFILE.md` from the repository
root. It must define product identity, binary, journey selector, session
start/stop commands, validated isolation fields, disposable state, journal
directory, post-exploration discovery, and handoff boundaries. Use
[profile-template.md](assets/profile-template.md) when creating a profile.

Stop before live exploration when the profile is missing, ambiguous, or cannot
prove sessions are disposable, offline, bounded, source-free, and isolated from
user state. Transcript mode needs no live profile; mark its evidence
non-clean-room.

## Integrity rules

- Use exactly two fresh tester subagents per live journey. Spawn both directly
  and concurrently. Stop if capacity is unavailable.
- Give testers only goal, stop condition, binary, revision, session metadata,
  journal path, profile, and this skill. Hide expected commands, suspected
  findings, peer identity, and prior evidence.
- Before the exploration marker, testers must not read source, repository docs,
  tests, plans, issues, prior journals, or another tester's work.
- Give each tester a fresh validated session. Never mount repository source,
  user state, credentials, or host workspaces into it. Never mutate real user or
  production state.
- Preserve journals before cleanup. Never delete or reset outside the
  profile-reported session boundary. Redact secrets and stable personal paths
  with explicit placeholders.

## Modes

- **Live:** one tester pair, then one fresh synthesizer.
- **Batch:** all tester pairs fan out directly; each completed pair gets one
  fresh synthesizer; one final fresh meta-synthesizer receives pair syntheses.
- **Transcript:** convert supplied terminal evidence into one retrospective
  journal. Do not invent expectations, timing, or missing output.
- **Performance add-on:** record perceived latency first. Follow the profile's
  performance workflow before tracing, profiling, or making a measured claim.

## Run live journeys

1. Choose a journey through the profile selector. Paired testers share family,
   variation, goal, stop condition, binary, and revision. Exclude recent
   families when at least three alternatives remain; keep concurrent batch
   pairs distinct.
2. Express one realistic outcome, never a command script. Let testers explore
   adjacent commands naturally until success or bounded impasse.
3. Start and validate two profile sessions. Require every declared isolation
   field and distinct session identities. Failed pre-exploration isolation may
   be replaced; product failure remains evidence.
4. Copy [journal-template.md](assets/journal-template.md) twice and
   [synthesis-template.md](assets/synthesis-template.md) once into the profile's
   journal directory. Assign stable collision-free pair, tester, and session
   metadata.
5. Instantiate [tester-prompt.md](assets/tester-prompt.md) for both testers.
   Do not begin synthesis until both raw journals close.

Tester or product failure remains evidence. Replace a tester only when it
failed to start or violated isolation before product exploration began.

For a requested batch, select every journey first. Spawn all `2 × rounds`
testers directly when capacity exists; never use nested tester spawning. Start
pair synthesis in newly free slots. After all pairs terminate,
instantiate [batch-synthesis-template.md](assets/batch-synthesis-template.md)
for meta-synthesis.

## Journal evidence

Before each meaningful action, append:

1. immediate goal and expected result;
2. exact command, exit status, relevant stdout, and relevant stderr;
3. perceived latency: `instant`, `noticeable`, or `slow`, recorded before any
   timing or tracing;
4. friction and next choice in tester's own words.

Use stable `J-001` timeline IDs. Never rewrite earlier entries; append
corrections. Continue naturally after errors. End black-box exploration with:

```text
Exploration complete. Repository context may now be consulted.
```

Only afterward may each tester consolidate `F-001` findings, distinguish
observation from inference, assign `P0` silent-omission/destructive/security/
false-success, `P1` blocking/material disruption, or `P2` confusing/
undiscoverable impact, and follow profile discovery. Do not change product code
or publish issues.

## Filter evidence

Give a fresh pair synthesizer only the two immutable journals, pair metadata,
profile, synthesis template, and this skill. It must
validate matching journey, goal, stop, binary, and revision plus distinct valid
sessions. Invalid pairs publish nothing.

Preserve every candidate and classify it:

- `accepted-reproduced`: both testers independently observed it;
- `accepted-valid`: exact one-sided evidence supports user impact and the peer
  does not contradict it;
- `rejected`: unsupported inference, visible-guidance recovery, environment
  artifact, contradiction, duplication, or unsupported preference;
- `needs-more-evidence`: plausible but insufficiently supported.

Only accepted findings may become product feedback or authorized external
updates. Pair synthesis cites tester plus timeline/finding IDs. After initial
classification, follow profile discovery to mark existing duplicates without
erasing new evidence.

Batch meta-synthesis runs in a fresh subagent and receives pair syntheses only.
Exclude invalid pairs and non-accepted candidates; deduplicate while retaining
every pair citation, recurrence count, and journey breadth. Rank by evidence,
impact, priority, and cross-journey reach. Append its complete decisions; never
promote raw evidence or redo pair acceptance.

## Close and hand off

Preserve raw timelines unchanged. Record outcome, cleanup, retained artifacts,
and limitations. Run the profile stop command for every validated session.
Return journal paths, accepted ranked findings, rejected/uncertain counts, and
required user choices.

Report accepted findings as one ranked table, most severe first, before any
prose. A reader must be able to act from the table alone:

| ID | Finding | Class | Pri | Owner | Suggestion | Evidence |
|---|---|---|---|---|---|---|

`Suggestion` is the concrete change that would close the finding, short enough
to read in the row — the exact sentence, flag, or command, not "clarify this".
Where the fix needs a decision the workflow cannot make, say which decision.
`Evidence` cites the tester or reader IDs the finding rests on. Rejected and
uncertain candidates are counted, not tabled. A batch table adds `Recurrence`
and the guides or personas each finding spans.
 Follow profile handoffs for formal tests, product
changes, performance work, or external issue updates.
