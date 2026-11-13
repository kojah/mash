# Search across your machines

A shared index lets your laptop find chats indexed on your desktop, and the
reverse. Each computer keeps its own local index and also publishes it to one
Mash server that you run. Computers in the same group can then search and read
each other's published history.

Sharing is optional. On one computer, skip this guide: local search already
works offline.

**Sharing uploads chat history.** A computer that publishes sends the chats it
has indexed, with their retained attachments, to your server, where every
computer holding a token for the same group can read them. Run the server only
on a machine and network you trust.

## What runs where

| Where | What runs | You set it up |
| --- | --- | --- |
| One server machine | `mash-server` and a Qdrant 1.20+ instance for it, both with persistent storage | once, in step 1 |
| Each computer | `mash` with its local index and local Qdrant, as in [installation](installation.md) | in steps 2 and 3 |

The server machine can be one of your computers or a separate always-on host.
`mash-server` ships with `mash`; run the same release on the server and on
every computer.

Before you connect a computer you need:

- a server URL that the computer can reach: HTTPS, or a private-network address
  you trust;
- a group ID, shared by all your computers (the examples use `personal`);
- a node ID, unique to that computer (`laptop`, `desktop`);
- a token for that computer, different from every other computer's.

If someone already runs the server for you, get these four from them and go to
[step 2](#2-connect-each-computer).

## 1. Set up the server

On the server machine:

1. Start Qdrant for the server.
   [`mash qdrant install`](installation.md#start-the-local-search-service)
   runs one on `127.0.0.1`, ports 6333 (HTTP) and 6334 (gRPC). The
   [Docker image](installation.md#optional-qdrant-docker-image) or an existing
   Qdrant 1.20+ service also works. Keep Qdrant's ports private to this host.

2. Create one token per computer. Each must be 32 to 4096 printable ASCII
   characters with no spaces, for example:

   ```sh
   openssl rand -hex 32
   ```

3. Save this as `server.toml`. It lists every computer allowed to connect and
   uses word search only, so the server needs no model download:

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

   The file names environment variables; it never holds the secrets. Keep
   tokens out of Git.

4. Set those variables and start the server. Both Qdrant variables must point
   at the **same** Qdrant instance:

   ```sh
   export MASH_QDRANT_URL=http://127.0.0.1:6333
   export MASH_QDRANT_GRPC_URL=http://127.0.0.1:6334
   export MASH_LAPTOP_TOKEN='the token you made for the laptop'
   export MASH_DESKTOP_TOKEN='the token you made for the desktop'
   mash-server --server-config /absolute/path/to/server.toml
   ```

   Keep it running under your service manager, with the same variables.

5. Make it reachable. `listen = "127.0.0.1:8787"` accepts connections from the
   server machine only. Put an HTTPS reverse proxy in front of it, or set
   `listen` to the machine's private-network IP and use that address as the
   server URL.

This configuration is for a new server. Back up an existing one before
changing its settings.

## 2. Connect each computer

Do this on every computer that should publish or search shared history. Mash
must already be [installed](installation.md) there, with its local Qdrant
running.

### With a join string

The quickest way needs no token variables and no configuration editing. On the
server, add the computer by name and say which address it should use to reach
the server:

```sh
mash-server --server-config /absolute/path/to/server.toml \
  --add-node personal/laptop --api-url https://mash.example.net
```

It prints a `join` value starting with `mash-join.`. Restart the server so it
accepts the new computer, then on that computer run:

```sh
mash init --join 'mash-join.…'
```

That writes the sharing settings, saves the computer's token in
`~/.mash/node.env` (readable only by you), then indexes and uploads. The join
string contains the token, so pass it privately and treat it like a password.
The server keeps only a hash of the token, in `server.toml.nodes` beside its
configuration. To issue a new string for a computer, delete its `[[nodes]]`
entry from that file and add it again.

### By hand

Or set the token and settings yourself.

First put this computer's token in an environment variable, for example
`MASH_NODE_TOKEN`. Mash stores only the variable's name, so the variable must
be set wherever Mash runs: your shell, the
[watcher](synchronization.md), and the agent clients that start Mash's MCP
server.

Then either use the wizard or edit the configuration.

**Wizard.** Run `mash init` in a terminal and choose **Connect to an existing
shared server**. If Mash is already configured, choose **Review and edit
settings** first. Enter the server URL, group ID, this computer's node ID, and
the name of the token variable. The wizard checks that the server accepts the
token, then offers to index and upload. **Set up sharing later** finishes local
setup without sharing.

**By hand.** Edit the node configuration that `mash init` created. Keep its
providers and local Qdrant settings. In the existing `[index]` table set
`destination`, and add a `[shared]` table:

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

`group_id` and `node_id` must match one `[[nodes]]` entry on the server, and
the variable must hold that entry's token. On the desktop, use
`node_id = "desktop"` and the desktop's token.

Search settings must suit the server. For the word-search server above, each
computer uses `[views.build] representations = "lexical-only"` and
`[views.query] intent = "exact-mention"`, which is what `mash init` writes
when no models are installed. For semantic search, ask the server
administrator for matching model settings.

## 3. Publish and check

On **each** computer:

```sh
mash index
mash status --shared
```

`mash index` updates the local index, then uploads it. It publishes history
from the last `since_days` days; `mash index --all-history` includes older
chats. `mash status --shared` reports the server's view under `shared_index`,
or a `shared_error` when the server could not be reached or refused the token.

A computer's chats appear on the others only after that computer has
published. If publishing fails, local history is kept: fix the reported error
and run `mash index` again. To keep publishing new chats, run a
[watcher](synchronization.md) on each computer.

## Search

```sh
mash search 'authentication'            # this computer and the shared index
mash search 'authentication' --shared   # the shared index only, including this computer's published chats
mash search 'authentication' --local    # this computer only; the server is not contacted
```

Add `--node desktop` to keep only chats published by that node ID. It filters
results; it does not connect to that computer.

## Remove a machine, or let a rebuilt one publish again

The server ties each node ID to the local index that first published under it,
so another machine cannot overwrite that history by reusing the name. A machine
whose index was deleted and rebuilt is refused for the same reason: publishing
fails with "shared node ownership or revision changed". Restoring the index
from a [backup](upgrades.md#replace-an-index-that-cannot-be-upgraded) with
`--keep-identity` keeps the name. Otherwise, retire the name on the server:

```sh
# On the server machine, with mash-server stopped.
mash-server --server-config /absolute/path/to/server.toml --retire-node personal/laptop
```

**This permanently deletes everything that machine published:** its chats and
records on the server, their search data, and its shared attachments. The
command prints how many chats and messages it removed and exits without
serving. Other machines and the retired machine's own local history are not
touched.

Start `mash-server` again, then run `mash index` on the machine to publish
from its current index. To remove a machine for good, retire it and delete its
`[[nodes]]` entry from `server.toml` instead.

Before installing a new Mash release, read
[upgrades and backups](upgrades.md#upgrade-a-shared-index).
