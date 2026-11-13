---
name: remember
description: Save something to Mash memory on request. Use when the person runs /mash:remember, or says "remember this", "save this to Mash", "make a note that", or asks you to keep a decision, preference, correction or fact for later sessions.
---

# Save to Mash memory

The person wants one thing kept for later sessions. Save it now with Mash's
`record` tool; hosts may prefix the tool names.

## What to save

Whatever follows the command. With nothing after it, save the most recent
thing the person decided, stated as a preference, corrected, or settled as a
fact in this conversation, and say which one you chose. If nothing in the
conversation qualifies, ask what to remember instead of inventing something.

Write it as one statement that will make sense alone in a later session:
name the project, file or tool it concerns, and say what was chosen, not that
a discussion happened.

## How to save it

1. Call `help` with `tool: "record"` for the current arguments. The loaded
   skill may describe another release; live help is authoritative.
2. Call `find_record_matches` with the same scope and body you are about to
   save. If a record already says
   it, tell the person and stop. If one says something that this replaces,
   supersede that record instead of adding a second one.
3. Call `record` with:
   - the scope and `session_id` from the Mash note at the start of this
     session (repository scope inside a repository);
   - a kind that fits: a decision, a preference, or a fact;
   - `evidence`: the person's exact words from this conversation as the
     quote, with purpose `endorsement`. The quote is checked against this
     session's transcript, so copy it exactly, typos included.
   - review `established`, because the person asked for it directly.
4. If the save fails, say so with the error. Retry once on `index-busy`.
   Never report something as saved that was not.

## Afterwards

Confirm in one line: the statement as saved and its scope. Do not paste IDs
unless asked. Do not save anything else the person did not ask for.

Never save credentials, tokens or other secrets, even on request: say why and
offer to save a note about where the secret is kept instead.
