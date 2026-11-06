# Search across your machines

A shared index lets your laptop find chats indexed on your desktop, and vice
versa. Each machine keeps its own local history and search service, then
publishes to one authenticated Mash server. Machines in the same group can
search each other's published history.

For one machine, skip this setup: local search already works offline.

## What you need

- Mash installed and initialized on each publishing machine.
- A local Qdrant instance on each machine.
- A server running the same Mash release, with Qdrant 1.19+ and persistent storage.
- An HTTPS endpoint for the Mash API, or a trusted private network.

For the recommended Qdrant setup, install Docker with Docker Compose on the
hosts running Qdrant. Follow [the Compose setup](installation.md#start-the-local-search-service)
and use Linux containers on Windows. An existing Qdrant service also works.

The provided Compose file starts **Qdrant only**. Run the shared Mash API on
the host with `mash serve`, as shown below. Connecting to an already-running
shared API does not require installing another server or Docker just for that
connection; each publishing machine still needs its own local search service.

## Configure a new server

This minimal example uses lexical search so no model download is needed.
Save it as `server.toml` on the API host:

```toml
version = 1
listen = "127.0.0.1:8787"
qdrant_url_env = "MASH_QDRANT_URL"
qdrant_grpc_url_env = "MASH_QDRANT_GRPC_URL"

[views.build]
representations = "lexical-only"

[[nodes]]
group_id = "personal"
node_id = "laptop"
token_env = "MASH_LAPTOP_TOKEN"

[[nodes]]
group_id = "personal"
node_id = "desktop"
token_env = "MASH_DESKTOP_TOKEN"
```

Set the two Qdrant URL variables to the HTTP and gRPC addresses of the **same**
server-side Qdrant instance. Set each node-token variable to a different random
secret of at least 32 printable ASCII bytes. Keep tokens out of Git.

Start the API:

```sh
mash serve --server-config /absolute/path/to/server.toml
```

The example listens only on loopback. Put an HTTPS reverse proxy in front of it
for other machines to reach it, or change `listen` to the host's private-network
IP and use that private URL. Keep Qdrant's own ports private to the API host.

These settings are for a new service. Back up an existing installation before
changing it.

## Connect each machine

Edit the node configuration created by `mash init`. Preserve its providers and
local Qdrant settings. Change the existing `[index]` table and add:

```toml
[index]
destination = "both"
since_days = 90

[shared]
group_id = "personal"
node_id = "laptop"
api_url = "https://mash.example.net"
token_env = "MASH_NODE_TOKEN"
```

Replace the URL with your reachable API endpoint. On the other machine, use
`node_id = "desktop"`. Set `MASH_NODE_TOKEN` to that machine's matching server
token. Make the variable available to the CLI, watcher, and agent's MCP process.

For the lexical server above, use `[views.build] representations = "lexical-only"`
and `[views.query] intent = "exact-mention"` on each node. These are the normal
model-free `init` settings. For semantic search, ask the server administrator for matching model settings.

## Publish and search

On **each** machine:

```sh
mash index
mash status --shared
```

For older history, use `mash index --all-history`. Check each node's publication
and search readiness before expecting its chats to appear elsewhere.

```sh
mash search 'authentication'
mash search 'authentication' --shared
mash search 'authentication' --local
```

The first combines local history and other nodes. The second searches published
history only, including this node. The third stays local.
Use `--node desktop` to narrow by publishing identity, not to connect to a host.

Run a watcher on each publishing machine for [ongoing updates](synchronization.md).
If publication fails, local history is retained; inspect the reported shared
error and retry indexing once the connection is fixed.

For schema changes, follow [upgrades and backups](upgrades.md).
