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

Start with search or a recent conversation. The navigation menu opens machines,
agents, recent chats and Memory. **Continue reading** keeps positions for chats
opened in the current browser session. The reader has an outline of the messages
on the current page; phones put Outline, Memory and Export in a bottom toolbar.
Expand tool activity and reasoning when you need more context.

The dashboard is read-only. Your chats stay separate from the public Mash
documentation website.

## Inspect conversation records

Below the chat sections, the front page has a **Memory** section with icons for **Decisions**, **Preferences**,
**Facts** and **Questions**. Choose a kind to browse it directly, or use the
**Memory** section title to browse all kinds. Search their statements,
switch kinds using the category tabs. Open **Filters** to filter by status or review, and choose
local records or records published to your configured shared group. Choose
**Apply filters** to run the search; use **Next page** to continue.

Open a record to read its revision history, quoted supporting messages and
related records. **Open source message** takes you to the cited chat message.
From a transcript, **Browse chat memory** (or **Memory** in the phone toolbar) finds records citing that chat in any
retained revision, including records whose latest revision cites another chat.

Browsing includes candidates and resolved or retired records. Review the status,
expiry dates, evidence warnings and declared conflicts before relying on a
conclusion. Reading never confirms or changes a record; ask your agent to make
changes through Mash's [record tools](records.md).

Record lists and open records stay fixed until you choose **Refresh memory**
or **Refresh record**. Revision history and relationships have separate page
controls. Shared records keep their source node identity when you follow links;
an unavailable shared source produces an error rather than opening a local copy.

## Keep your place

Mash does not refresh the chat list or transcript automatically. Choose
**Check for updates** to check for new content, then **Show updates** to update
the recent-chat list or **Show updated messages** to update an open transcript.
Returning to the browser tab or reconnecting does not trigger a refresh.

Submitted searches and their filters are kept while you open a result and return
with your browser’s Back button. The menu and chat links navigate without reloading
the page.

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
