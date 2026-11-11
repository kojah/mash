# Browse chats and records in the dashboard

After [setup](installation.md), run the command below. For a Cargo source
installation, build the web interface using the instructions below first.

```sh
mash dash
```

Open **[http://127.0.0.1:8790](http://127.0.0.1:8790)** in your browser.
Leave the terminal running; Ctrl-C stops the dashboard. If that port is busy,
use `mash dash --listen 127.0.0.1:8791` and open port 8791 instead.

If you installed with `cargo install --git`, the web interface is not bundled.
Build it once using [Node.js 24+](https://nodejs.org/en/download), pnpm, and Rust:

```sh
git clone https://github.com/kojah/mash.git
cd mash
npm install --global pnpm@11.28.5
pnpm --dir dashboard install --frozen-lockfile
pnpm --dir dashboard run build
cargo +stable install --locked --path .
mash dash
```

Use the same repository revision as your existing Mash installation to avoid a
schema mismatch.

For frontend development, `pnpm --dir dashboard run dev` forwards API requests
to `http://127.0.0.1:8790`. Set `MASH_DASHBOARD_API_URL` to use a different running
dashboard backend. This changes the development proxy, not Mash configuration.

The left rail is the way around. On a phone it opens from the menu button.

- **overview:** your latest chats and memories, and each machine and agent
  with its chat count.
- **chats:** every chat, newest first. Typing in the search box at the top
  searches the same list. **viewing history** lists chats you opened in this
  browser session; each reopens where you left it.
- **agents**, **machines** and **projects:** one page each, with its recent
  chats and how they break down by the other two.
- **memory:** saved decisions, preferences, facts and open questions.
- **services** and **manage fleet:** process and database checks, each
  machine's agent setup, and fleet actions where they are configured. See
  [services](services.md).
- **mcp console:** every tool your agents can call on Mash, listed from the
  server's own description: what each does, its parameters and their types.
  Fill in a read-only tool and run it to see the reply exactly as an agent
  receives it. Tools that change data are listed for reference but cannot be
  run from the dashboard.
- **settings:** what the configuration file says, with index readiness. It is
  read-only: edit the file to change a setting.

## Find and read a chat

The search box searches what was said in your chats. Without a query the page
lists chats. Narrow either with the chips for agent, machine, project and date.
A chip you change is a draft, shown with a dashed border, until you choose
**Apply filters**. Results never change underneath you. Applied filters appear
as tags in the search box; choose one to remove it.

A transcript shows your messages on the right and the agent's replies on the
left. Runs of tool calls and reasoning are folded into one line: open it to see
each step and its output. The panel on the left lists your prompts on the
current page, other chats from the same project and any memory saved from this
chat. **Copy session ID** copies the ID to resume the chat in its agent.
Browser Find searches the current page.

Chat and Memory views are read-only. Your chats stay separate from the public
Mash documentation website.

## Inspect conversation records

Open **memory** in the rail, or one of the kinds beneath it. Switch kinds with
the chips at the top, search statements with **find a statement**, and use the
status, review and source chips to filter. Source chooses this machine's
records or those published to your shared group. Choose **Apply filters** to
run the search; use **Next page** to continue.

Open a record to read its revision history, quoted supporting messages and
related records. **open the message** under a quote takes you to the cited chat
message. In a transcript, **memory from this chat** lists records citing that
chat in any retained revision, including records whose latest revision cites
another chat. A message a record quotes shows **Saved as** beneath it.

Browsing includes candidates and resolved or retired records. Review the status,
expiry dates, evidence warnings and declared conflicts before relying on a
conclusion. Reading never confirms or changes a record; ask your agent to make
changes through Mash's [record tools](records.md).

Record lists stay fixed until you apply filters again, and an open record until
you choose **check for updates**. Revision history and relationships have separate page
controls. Shared records keep their source node identity when you follow links;
an unavailable shared source produces an error rather than opening a local copy.

## Keep your place

Mash does not refresh the chat list or transcript automatically. Choose
**reload chat list** at the foot of the rail, or **Check for updates** on the
overview, to load new chats. Memory has its own **check for updates**. In a
transcript, the refresh button checks for new messages and **Show updated
messages** applies them.
Returning to the browser tab or reconnecting does not trigger a refresh.

Submitted searches and their filters are kept while you open a result and return
with your browser’s Back button. The menu and chat links navigate without reloading
the page. App links update browser history directly, without requesting even a
fragment navigation from the browser. A shared navigation boundary prevents internal links and ordinary
form submissions from replacing the document. Automatic query refresh is off
by default throughout the app. Pull-to-refresh is suppressed in browsers that
support overscroll control; the browser's Reload command and recovery of a
discarded tab remain outside the app's control.

If a request fails, an error panel names what could not load and offers **Retry**.
Retry fetches that data without reloading the page. A connection error means the
browser could not reach Mash; check the connection and that `mash dash` is still
running. It does not mean your indexed chats or memories were deleted.
Opening a view while offline reports the failed request instead of leaving it
queued behind a loading message. Once connected again, choose **Retry**.

To make new conversations available, run `mash index`, or leave
`mash index --watch` running in another terminal.

## Save a transcript

Use the transcript's **Export** menu to download the whole chat as Markdown or
HTML, including tool output. **Export to PDF** opens a printable copy; choose
**Save as PDF** in your browser's print dialog.

Wait for any indexing to finish before exporting if you need a stable copy.
Downloaded transcripts may contain sensitive information. To back up Mash
for a later restore, use [a portable backup](upgrades.md#make-a-portable-backup).

## Keep access private

By default, only your machine can reach the dashboard. It has no login, so
changing its listening address can expose your history to others. Keep it on
your own machine or a trusted private network.
