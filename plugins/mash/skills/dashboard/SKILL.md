---
name: dashboard
description: Open the Mash dashboard in the person's browser. Use when the person runs /mash:dashboard, or asks to open, show or start the Mash dashboard, or to see this chat or their past chats or memories in a browser.
---

# Open the Mash dashboard

The person wants Mash's web dashboard in front of them. Start it if it is not
running, then open it and give them the address.

## Steps

1. Check whether it is already running: request `http://127.0.0.1:8790/`
   with a two-second limit (`curl -fsS -m 2 -o /dev/null http://127.0.0.1:8790/`).
   A success means a dashboard is serving; do not start another.
2. If nothing answers, start it so it outlives this command:
   `mash dash` as a background process with its output sent to a log file
   in the system's temporary directory. Then repeat the check for up to ten
   seconds. If it never answers, show the last lines of the log and stop.
   If the log says the port is in use by something else, start it with
   `mash dash --listen 127.0.0.1:8791` and use that port below.
3. Pick the page:
   - with nothing after the command, the overview: `http://127.0.0.1:8790/`
   - when the person asks for "this chat", the current session's chat:
     `http://127.0.0.1:8790/#/chat/<session id>`, with the session id from
     the Mash note at the start of this session. Use it only if you have
     that id; otherwise open the overview and say so.
4. Open it in the person's browser: `open` on macOS, `xdg-open` on Linux,
   `start` on Windows. If that fails or there is no desktop, skip it.
5. Reply in one or two lines: the address as a link, and whether you started
   the dashboard or found it running.

## When the person is not at this machine

The dashboard listens on this machine only. In a remote or SSH session, say
that the address works only from this machine's own browser, and that
`mash dash remote` keeps the dashboard reachable from their other devices
behind a sign-in. Run `mash dash remote` only if they ask for it.

## Limits

Never pass `--listen` with an address other than loopback, and never add
`--allow-host` or an access token on your own: those expose history beyond
this machine and are the person's decision. Do not stop a dashboard you did
not start. History in the dashboard is read-only; nothing here changes the
index.
