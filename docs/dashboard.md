# Navigating the dashboard

After [setup](installation.md), start the dashboard:

```sh
mash dash
```

Open **[http://127.0.0.1:8790](http://127.0.0.1:8790)** in your browser. It
opens on the overview: your latest chats and memories. Leave the terminal
running; Ctrl-C stops the dashboard. If that port is busy, use
`mash dash --listen 127.0.0.1:8791` and open port 8791 instead.

The dashboard has no login. Read [Keep access private](#keep-access-private)
before changing its listening address.

A `cargo install --git` installation does not bundle the web interface. Build
it once using [Node.js 24+](https://nodejs.org/en/download), pnpm, and Rust:

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
to `http://127.0.0.1:8790`; set `MASH_DASHBOARD_API_URL` to proxy to a
different running dashboard.

## The rail

The left rail is the way around. On a phone it opens from the menu button.

- **overview:** your latest chats and memories, and each machine and agent
  with its chat count.
- **chats:** every chat, newest first, with a search box. **viewing history**
  lists chats you opened in this browser session; each reopens where you left
  it.
- **agents**, **machines** and **projects:** one page each, with its recent
  chats and how they break down by the other two.
- **memory:** saved decisions, preferences and facts.
- **services** and **fleet manager:** process and database checks, each
  machine's agent setup, and fleet actions. The fleet manager appears only
  where fleet control is set up. See [services](services.md).
- **tool explorer:** every tool your agents can call on Mash, with its
  parameters. Run a read-only tool to see the reply as an agent receives it.
  Tools that change data are listed but cannot be run here.
- **settings:** this machine's configuration and index readiness. You can
  change which agents are indexed, how far back, whether history also goes to
  the shared server, memory capture and credential masking. Saving rewrites
  the file and keeps the previous one beside it as `config.toml.previous`;
  restart the watcher and reconnect agents to apply. Search and storage
  settings are shown but cannot be changed here. A dashboard that other
  machines can reach changes settings only when it was started with
  `--services-config`.

The button in the top right corner switches between light and dark; the
choice is remembered in that browser.

## Find and read a chat

Open **chats** and type in the search box to search what was said. Without a
query the page lists chats. Narrow either with the chips for agent, machine,
project and date. A changed chip is a draft, shown with a dashed border, until
you choose **Apply filters**. Applied filters appear as tags in the search box;
choose one to remove it.

A transcript shows your messages on the right and the agent's replies on the
left. Runs of tool calls and reasoning are folded into one line; open it to see
each step and its output. The side panel lists your prompts on the current
page, other chats from the same project and memory saved from this chat.
**Copy session ID** copies the ID to resume the chat in its agent.

Chat and memory views are read-only.

## Inspect conversation records

Open **memory** in the rail, or one of the kinds beneath it. Switch kinds with
the chips at the top, search statements with **find a statement**, and filter
with the status, review and source chips. Source chooses this machine's
records or those published to your shared group. Choose **Apply filters** to
run the search and **Next page** to continue.

Open a record to read its revision history, quoted supporting messages and
related records. **open the message** under a quote opens the cited chat
message. In a transcript, **memory from this chat** lists records that cite
the chat in any retained revision, and a quoted message shows **Saved as**
beneath it.

Lists include candidates and retired records. Check the status, expiry dates,
evidence warnings and declared conflicts before relying on a record. Reading
never confirms or changes one; ask your agent to make changes through Mash's
[record tools](records.md). If a shared record's source is unavailable, its
link produces an error rather than opening a local copy.

## Keep your place

Nothing refreshes by itself. A submitted search and its filters are kept when
you open a result and return with your browser's Back button. To load new
data:

- **reload chat list** at the foot of the rail, or **Check for updates** on
  the overview, loads new chats.
- **check for updates** reloads memory or an open record.
- In a transcript, the refresh button checks for new messages and
  **Show updated messages** applies them.

New conversations appear only after indexing: run `mash index`, or leave
`mash index --watch` running in another terminal.

If a request fails, an error panel names what could not load and offers
**Retry**. A connection error means the browser could not reach Mash, not that
chats or memories were deleted. Check the connection and that `mash dash` is
still running, then choose **Retry**.

## Save a transcript

Use the transcript's **Export** menu to download the whole chat as Markdown or
HTML, including tool output. **Export to PDF** opens a printable copy; choose
**Save as PDF** in your browser's print dialog.

For a stable copy, wait for indexing to finish first. Downloaded transcripts
may contain sensitive information.

## Export many chats at once

Open **chats**, then **export**. Narrow the list with the agent, machine,
project and time filters; every matching chat is exported, on every page,
unless you untick it. Choose **Markdown** to read or **JSON** for scripts, then
**Export**. Your browser saves one `.zip` file with a folder per project and a
file per chat.

Keep the page open until the download starts. If any chat cannot be read, the
export stops and saves nothing. Attachments are not included. The files hold
the chats in full, including anything sensitive said in them.

These exports and saved transcripts cannot be brought back into Mash. For a
copy Mash can restore, use
[a portable backup](upgrades.md#make-a-portable-backup). Chats from your other
machines appear in the dashboard through [sharing](shared-index.md).

## Open it from your phone or another computer

By default, only your machine can reach the dashboard. To use it from other
devices, give it an access token and a private network path. The steps below
use [Tailscale](https://tailscale.com), which gives the dashboard an HTTPS
address only your own devices can open.

1. On the machine that runs the dashboard, choose a token of at least 32
   characters and keep it in an environment variable:

   ```sh
   export MASH_DASH_TOKEN="$(openssl rand -hex 24)"
   echo "$MASH_DASH_TOKEN"
   ```

2. Publish the dashboard's port inside your tailnet. The command prints the
   address, such as `https://laptop.your-tailnet.ts.net`:

   ```sh
   tailscale serve --bg 8790
   ```

3. Start the dashboard with that name and the token variable:

   ```sh
   mash dash --allow-host laptop.your-tailnet.ts.net --access-token-env MASH_DASH_TOKEN
   ```

4. On the phone, with Tailscale connected, open the address and enter the
   token once. The browser stays signed in for 30 days.

On Linux, `tools/dashboard-remote.sh` in the source repository does steps 1 to
3, keeps the dashboard running as a user service, and prints a QR code to scan
instead of typing the token (`tools/dashboard-remote.sh --qr` shows it again).
The code opens `https://…/#signin=TOKEN`; the dashboard signs in and removes
the token from the address at once. Treat the code like the token itself.

The dashboard searches whatever that machine can search: its own history and,
with a [shared index](shared-index.md), your other machines' history too.

A machine with no agent history of its own, such as the server that holds the
shared index, can still host the dashboard. Give it a configuration with a
`[shared]` table and start the dashboard with `--shared-only`: chats, search
and memories then come from the shared index alone, and no local index is
needed.

`--allow-host` is the name browsers use for the dashboard, with its port when
the address shows one (`laptop.your-tailnet.ts.net:8443`); repeat it for more
than one. It works the same way behind another HTTPS reverse proxy, such as
Caddy. Changing the token signs every browser out.

## Keep access private

With `--access-token-env`, every page of history needs the sign-in. Ten wrong
tokens in a minute pause sign-in for everyone until the minute passes.

Without it the dashboard has no login. Keep it on its default address, which
only your machine can reach: `mash dash --listen` with another address serves
your history to anyone who can connect, and prints a warning when started that
way. `--allow-host` refuses to start without a token.

Prefer a private network over a public address. On the public internet the
token is the only thing protecting your full chat history, and a tunnel service
that ends HTTPS for you, such as Cloudflare Tunnel, can read what passes
through it.
