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

The dashboard follows history as Mash updates it, on this machine and on the
machines that publish to your shared index; what you are reading never moves
by itself.

- **Lists of chats and memories** (the overview, the rail, recent chats, the
  session list beside a transcript, the agent, machine and project pages, and
  the memory pages) show what is new within a few seconds.
- **An open transcript** gains new messages at the end while you are at the
  bottom of the chat, and the view follows them. Anywhere else in the chat, an
  **N new messages** button appears instead; choose it to go to them. Text
  already shown never changes by itself: if Mash re-reads an earlier message,
  **Show updated messages** applies it.
- **A submitted search**, of chats or of memories, keeps its results and
  filters until you submit again, including when you open a result and return
  with your browser's Back button.
- **An open memory record** changes only when you choose **check for
  updates**.

Chats and memories from other machines arrive as each machine publishes them.
That needs a shared server at least as new as this dashboard. With an older
one, or with more than 16 dashboard tabs open at once, load them yourself:
**reload chat list** at the foot of the rail, and the refresh button in a
transcript.

If the connection drops, in the browser or between the dashboard and the
shared server, it reconnects by itself and then catches up.

New conversations appear only after indexing: run `mash index`, or leave
`mash index --watch` running in another terminal. The dashboard notices either
one; it does not index by itself.

If a request fails, an error panel names what could not load and offers
**Retry**. A connection error means the browser could not reach Mash, not that
chats or memories were deleted. Check the connection and that `mash dash` is
still running, then choose **Retry**.

## Ask Mashy

If you have [configured a model](configuration.md#the-dashboard-assistant),
a circle with the Mash mascot sits in the bottom-right corner of every page.
Choose it to open a chat with Mashy, and ask about your history in plain
words:

- "What did I work on yesterday?"
- "What did we decide about the restart test?"
- On a chat's page: "Summarise this chat."

Mashy searches your chats and memories, answers briefly, and lists the chats
and memories it drew on; choose one to open it in the dashboard behind. If it
finds nothing, it says so. Use the stop button to cut an answer short.

Mashy is a chat, not an agent. It can read your history and nothing else: it
cannot index, forget, save memories, run commands or change settings. The
conversation lasts as long as the browser tab and is not added to your
history.

Each question sends the text Mashy reads to the model you configured. Without
that configuration there is no circle and nothing is sent.

If Mashy fails or answers badly, start the dashboard with `MASH_TRACE=1` and
ask again. Its log then records each step of a question: which lookups ran and
how much they returned, how long the model took, how many tokens it used and
why it stopped. Questions, answers and history text are never logged.

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

By default, only your machine can reach the dashboard. On Linux or macOS with
[Tailscale](https://tailscale.com) connected, one command makes it reachable
from your other devices, and only from them:

```sh
mash dash remote
```

It keeps the dashboard running as a user service behind a sign-in (a systemd
user service on Linux, a LaunchAgent on macOS), gives it an HTTPS address
inside your tailnet, and prints the address, a one-time code and a QR code:

```json
{
  "dashboard": "https://laptop.your-tailnet.ts.net:8790/",
  "service": "mash-dash.service",
  "code": "K7M-2QX",
  "code_minutes": 10
}
```

On the phone, with Tailscale connected, scan the QR code. Or open the address
and type the code. Either way the browser stays signed in for 30 days.

| To | Run |
| --- | --- |
| Sign another device in | `mash dash remote --code` |
| Show the QR code again | `mash dash remote --qr` |
| Use another port | `mash dash remote --port 8791` (with every command) |
| Host it on a machine with no history of its own | `mash dash remote --shared-only` |
| Reach it without Tailscale on the device | `mash dash remote --public --port 8443` |
| Stop serving | `mash dash remote --remove` |

A code works once, within ten minutes. The QR code holds the long-lived token
itself, after `#` in the address, where browsers never send it to a server;
the dashboard signs in and removes it from the address at once. Treat a picture
of it like a password.

The dashboard searches whatever that machine can search: its own history and,
with a [shared index](shared-index.md), your other machines' history too. For
the shared index the service needs the node token: run `mash dash remote` in a
shell where that variable is set, and it is saved for the service in
`~/.mash/dashboard.env`, readable only by you. On macOS the LaunchAgent's
property list (`~/Library/LaunchAgents/dev.mash.dash.plist`) holds the same
tokens and is just as private; its output goes to `~/.mash/dashboard.log`.

A machine with no agent history of its own, such as the server that holds the
shared index, can still host the dashboard: with `--shared-only`, chats, search
and memories come from the shared index alone.

`--public` publishes the address on the internet with
[Tailscale Funnel](https://tailscale.com/kb/1223/funnel), so a phone or
borrowed computer that is not on your tailnet can open it. Tailscale must allow
Funnel for the machine, and only ports 443, 8443 and 10000 can be published.
Anyone can then reach the sign-in page: the token and one-time codes are all
that protect your full chat history, and ten wrong attempts in a minute pause
sign-in for everyone, you included. Prefer the default unless you need this.

### Set it up by hand

On Windows, or behind another HTTPS reverse proxy such as Caddy:

1. Choose a token of at least 32 characters and put it in an environment
   variable, for example `MASH_DASH_TOKEN`.
2. Give the dashboard an HTTPS address. With Tailscale:
   `tailscale serve --bg 8790`.
3. Start the dashboard with that name and the token variable:

   ```sh
   mash dash --allow-host laptop.your-tailnet.ts.net --access-token-env MASH_DASH_TOKEN
   ```

`--allow-host` is the name browsers use for the dashboard, with its port when
the address shows one; repeat it for more than one. Changing the token signs
every browser out.

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
