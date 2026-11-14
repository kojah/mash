---
name: export
description: Save a chat from Mash as a file to read or share. Use when the person runs /mash:export, or asks to export, save or download this chat or a past chat as Markdown, HTML or PDF.
---

# Export a chat

The person wants one conversation as a file. Mash's dashboard renders
Markdown, HTML and PDF; from here you can write Markdown yourself or open
the dashboard at the right place. Hosts may prefix the tool names.

## Which chat

With nothing after the command, this session's chat (its id is in the Mash
note at the start of this session). Otherwise find the one they mean with
`list_chats` or `search`; if several match, list them and ask.

## Choose the route

- **Markdown, and the chat is of ordinary length:** write it yourself, as
  below.
- **HTML or PDF, an exact copy with all tool output, or a very long chat:**
  use the dashboard. Follow the `dashboard` skill to open
  `http://127.0.0.1:8790/#/chat/<chat id>` and tell the person to use the
  transcript's **Export** menu: Markdown, HTML, or Export to PDF.
- **Many chats at once:** the dashboard's bulk export, from the Export
  entry in its menu, which saves a `.zip` with a folder per project.

Say which route you took and why.

## Writing Markdown yourself

1. Call `help` with `tool: "view"` for the current arguments; live help is
   authoritative.
2. Read the whole chat with `view`, following its continuation until the
   end. Do not summarise or skip messages; if you cannot read all of it,
   stop and use the dashboard route.
3. Write one file: a title, then the agent, machine, project and dates,
   then each message in order under a heading that names who spoke and
   when. Keep the text as it was. Include tool calls and their output only
   if asked, since they can be very long.
4. Save it where the person said. Otherwise use a `mash-exports` folder in
   the current directory, named from the chat's date and title, and never
   overwrite an existing file without asking.

Report the path and the message count, and say if anything was left out.

## Care

An export holds the conversation in full, including anything sensitive
said in it. Mash masks recognisable credentials when reading, but that is
best effort: tell the person to look it over before sharing, and do not
commit the file or send it anywhere yourself.

This is not a backup. `mash export` and the `export_silver` tool write the
whole index for moving or restoring it; use them only when the person asks
for a backup.
