# The Mash plugin for Claude Code and Codex

The plugin is what makes an agent use Mash without being told to. It adds
eleven skills, their slash commands, and hooks that bring earlier work into a session
and keep the index current. `mash init` installs it for each agent it
connects; this page says what you get and how to control it.

The plugin sits on top of the [MCP server](mcp.md), which gives the agent the
Mash tools themselves. Without the plugin the tools still work when you ask
for them by name.

## Skills and commands

| Skill | In Claude Code | In Codex | What it does |
| --- | --- | --- | --- |
| `remember` | `/mash:remember what to keep` | pick the `remember` skill, or say "remember this" | Saves a decision, preference or fact to [memory](records.md), with your words as the evidence. With nothing after it, saves the latest decision in the conversation. |
| `resume` | `/mash:resume` | pick the `resume` skill, or ask "where did we leave off?" | Reads this project's most recent chats, from every agent and machine, and tells you what was in progress, what was left open and what was decided. |
| `handoff` | `/mash:handoff` | pick the `handoff` skill, or say "hand this off" | Writes a brief of the session into the conversation: goal, what is done, what is half done, decisions and the next step. `resume` finds it from any agent or machine once the chat is indexed. |
| `recall` | `/mash:recall a topic` | pick the `recall` skill, or ask "what does Mash remember about…?" | Shows the [memory](records.md) records for a topic or this project, with where each came from. It tells you when records need review. |
| `review` | `/mash:review` | pick the `review` skill, or say "review Mash memory" | Goes through the memory records that need a decision from you, a few at a time: ones an agent saved without your confirmation, stale ones and conflicting pairs. You keep, fix, withdraw, remove or skip each. |
| `forget` | `/mash:forget what to remove` | pick the `forget` skill, or ask explicitly to forget or delete something Mash stored | Finds one memory record or indexed chat, shows exactly what would go, and removes it only after you say yes. Offers to correct a wrong record instead of deleting it. |
| `standup` | `/mash:standup`, or `/mash:standup this week` | pick the `standup` skill, or ask "what did I work on yesterday?" | Summarises your work over a period, grouped by project, across agents and machines. With nothing after it, covers the previous working day. |
| `export` | `/mash:export`, or `/mash:export a chat` | pick the `export` skill, or say "export this chat" | Opens the [dashboard](dashboard.md#save-a-transcript) on the chat, where the Export menu saves it as Markdown, HTML or PDF. Can write a Markdown file itself if you ask for that. |
| `status` | `/mash:status` | pick the `status` skill, or ask "is Mash working?" | A plain-language check of the index, search, agent connections and your other machines, with the one next step when something is off. |
| `dashboard` | `/mash:dashboard` | pick the `dashboard` skill, or say "open the Mash dashboard" | Finds the running [dashboard](dashboard.md), or starts one, and opens it in your browser. Ask for "this chat" to land on the current conversation. |
| `mash-history` | `/mash:mash-history`, or just ask | pick the `mash-history` skill, or just ask | Searches your past chats and answers with the messages behind the result. The agent also uses it unprompted when you refer to earlier work. |

You do not need the commands for everyday use. Ask in plain words:

- "What did we decide about the database migration?"
- "Find the chat where we set up deployment."
- "Remember that we deploy from `main` only."
- "Open the Mash dashboard."
- "Where did we leave off on this project?"
- "What does Mash remember about deployment?"
- "That memory about the staging URL is wrong, fix it."

After saving, the agent confirms the statement it saved. It refuses to save
credentials and tokens. Removing is permanent: `forget` handles one record or
chat at a time and always shows it and asks first.

## What happens without asking

| When | What the plugin does | Turn it off |
| --- | --- | --- |
| A session starts | Shows the agent this project's saved records and recent chats, and starts a background `mash index` at most once every ten minutes | `MASH_AUTO_INDEX=off` stops the indexing; `MASH_RECORD_RECALL=off` stops the record context |
| You send a message that looks back at earlier work, or names something the conversation has not mentioned | Reminds the agent to search Mash before guessing | `MASH_PROMPT_RECALL=off` |
| You state a lasting decision or preference | Reminds the agent how to save it. A session that runs long without saving anything gets one reminder at most every fifteen minutes | `MASH_RECORD_CAPTURE=off` |
| The agent reads a file in the project | Shows records and earlier chats about that file, once per file and session | `MASH_FILE_RECALL=off` |

Set a switch in the environment the agent starts in. Every hook does nothing,
silently, when Mash is missing, not set up, or slow to answer, so it never
blocks the agent. Nothing here sends your history anywhere: the hooks call the
`mash` program on your machine. [Record what you worked out](records.md) has
the detail on saving and recall.

## Requirements

- Python 3.9 or later for the hooks (`python3`, `python`, or the Windows `py`
  launcher).
- Network access to install or update the plugin; the agent downloads it from
  the Mash repository.

## Install, update and remove

`mash init` installs the plugin with the agent's own commands and prints the
files it edited. To do it yourself:

```sh
claude plugin marketplace add mashium/mash && claude plugin install mash@mash
codex plugin marketplace add mashium/mash && codex plugin add mash@mash
```

Restart the agent afterwards. The plugin and the `mash` program are released
together: after upgrading Mash, `mash status --installation` reports a plugin
that no longer matches it, and running the install command again refreshes it.
To remove it:

```sh
claude plugin uninstall mash@mash
codex plugin remove mash@mash
```

## Check that it is working

```sh
mash status --installation
```

Each agent's `connection` is `full` when the tools and the plugin are both in
place, and `mcp-only` when the plugin is missing. `next_action` gives the
command that repairs it. See
[Use Mash with your agent](agents.md#confirm-the-connection).
