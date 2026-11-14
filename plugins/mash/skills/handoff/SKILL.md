---
name: handoff
description: Write a brief of this session so another agent, machine or later session can continue it. Use when the person runs /mash:handoff, or asks to hand this session off or to leave notes for the next session or another agent.
---

# Hand this session on

The person is stopping here and wants the next session, possibly in another
agent or on another machine, to continue without rereading this one. Mash
indexes this conversation, so the brief goes into the conversation itself,
where `/mash:resume` finds it once the chat is indexed. Hosts may prefix the tool names.

## Write the brief

Do any saving first (see below), then reply with the brief as your final
message, under exactly this first line so it can be found later:

`Mash handoff: <project> — <one-line goal>`

Then, briefly:

- **Goal:** what this session set out to do.
- **Done:** what is finished and verified, with commits, branches or files.
- **In progress:** what is half done, and its exact state.
- **Not done:** what was planned and not started.
- **Decisions:** choices made here and why, and who made them.
- **Watch out:** failing checks, traps found, things tried that did not
  work.
- **Next step:** the first thing to do on return.

Write it for a reader with none of this context: name files, commands and
branches in full. State only what happened in this session; mark anything
unverified as unverified. Leave out credentials, tokens and other secrets:
this text is stored and may be shared across the person's machines.

## Keep the decisions

A brief is not memory. If the person stated decisions or preferences in
their own words here, name them in the brief's Decisions and offer once, in
its last lines, to save them with `/mash:remember`. Do not save your own
conclusions as their decisions.

## Make it findable

Make the brief the last thing you write in this turn, and call no tool
after it. Mash can only index a message once the turn that wrote it has
ended, so indexing from inside this turn would miss the brief.

Close the brief with one line: the next session picks it up with
`/mash:resume`, which refreshes the index first.
