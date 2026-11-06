# Connect another MCP client

Claude Code and Codex can be connected automatically with `mash init`.
See [agent setup](agents.md) for the simplest path.

Other clients that support local MCP servers can start Mash themselves.

## Connect another MCP client

After [setting up Mash](installation.md), add an entry like this to your
client's MCP configuration:

```json
{
  "mcpServers": {
    "mash": {
      "command": "/absolute/path/to/mash",
      "args": ["mcp", "--config", "/absolute/path/to/config.toml"]
    }
  }
}
```

Replace the paths with your installed executable and Mash configuration.
The exact configuration format depends on your client. For shared history,
also make your configured token environment variable available to the client.

Restart the client, then ask it to use Mash to find an earlier conversation.

## What your agent can do

Your agent can search chats, list conversations, read messages and available
attachments, and check whether history is current. It can also refresh history,
forget a chat, make backups, or download optional models when you request it.

If new chats are missing, run `mash index`. For ongoing updates, see
[keep history up to date](synchronization.md).

## Read saved attachments

Ask the agent to list attachments on a message and open the one you need.
Mash can retain attachments from Claude Code, Codex, and OpenCode; other
providers' attachments are not currently captured.

Embedded files are saved automatically when supported. To allow files referenced
from a directory on your machine, add that directory to your Mash configuration:

```toml
[attachments]
allowed_roots = ["/absolute/path/to/provider/attachments"]
share_files = false
```

Run `mash index` afterward. Missing or disallowed files may appear by name
without a readable copy. Files are not shared with your other machines by
default. Images and PDFs can contain sensitive information; review them before sharing.
