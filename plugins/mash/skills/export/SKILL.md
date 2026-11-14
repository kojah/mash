---
name: export
description: Save a chat from Mash as a file to read or share. Use when the person runs /mash:export, or asks to export, save or download this chat or a past chat as Markdown, HTML or PDF.
---

# Export a chat

The person wants one conversation as a file. Mash's dashboard does this
properly; open it on the right chat. Hosts may prefix the tool names.

## Which chat

Find the chat they mean with `list_chats` or `search`; if several match,
list them and ask. With nothing after the command they mean this session:
use the session id from the Mash note at the start of this session if it
has one, otherwise find this chat by its latest messages. The current
session's last few minutes are usually not indexed yet, so offer to run
the `index` tool first and say that the export ends where the index does.

## The dashboard does the exporting

Mash renders a chat as Markdown, HTML or PDF in its dashboard, exactly and
with tool output. So the normal route is:

1. Follow the `dashboard` skill to open
   `http://127.0.0.1:8790/#/chat/<chat id>`.
2. Tell the person to use the transcript's **Export** menu: Markdown, HTML,
   or Export to PDF. The browser saves the file.

In a remote or SSH session the dashboard address works only on that
machine: say so, and either point to `mash dash remote` or offer to write
the Markdown file here instead.

For many chats at once, point to the dashboard's bulk export: open
**chats**, then **export**. It saves a `.zip` with a folder per project.

## Writing a Markdown file here instead

Only when the person asks for the file to be written from this session, or
the dashboard cannot be opened. Say first that this is your transcription
of the chat, so it is slower than the dashboard and may differ from it in
small ways; for a long chat, recommend the dashboard.

1. Call `help` with `tool: "view"` for the current arguments; live help is
   authoritative.
2. Read the whole chat with `view`. It reads around one message at a
   time, so start from the chat's first message and keep viewing from the
   last message returned until you reach the end. Do not summarise or skip
   messages. If you cannot read all of it, stop and say so.
3. Write one file: a title, then the agent, machine, project and dates,
   then each message in order under a heading that names who spoke and
   when. Include tool calls and their output only if asked.
4. Save it where the person says. If they name no place, ask; do not pick
   a folder inside a repository, and never overwrite a file without asking.

Report the path and the message count, and say if anything was left out.

## Care

An export holds the conversation in full, including anything sensitive
said in it. Mash masks recognisable credentials when reading, but that is
best effort: tell the person to look it over before sharing, and do not
commit the file or send it anywhere yourself.

This is not a backup. `mash export` and the `export_silver` tool write the
whole index for moving or restoring it; use them only when the person asks
for a backup.
