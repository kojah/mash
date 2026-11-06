# Browse chats in the dashboard

After [setup](installation.md), run:

```sh
mash dash
```

Open **[http://127.0.0.1:8790](http://127.0.0.1:8790)** in your browser.
Leave the terminal running; Ctrl-C stops the dashboard. If that port is busy,
use `mash dash --listen 127.0.0.1:8791` and open port 8791 instead.

Choose a recent chat from the sidebar, or search for something you remember
and open a matching message. Expand tool activity and reasoning when you need
more context.

The dashboard is read-only. Your chats stay separate from the public Mash
documentation website.

## Keep your place

The recent-chat list refreshes automatically. An open transcript stays put
until you choose **Show updated messages**. Use **Check for updates** to see
whether there is anything new.

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
