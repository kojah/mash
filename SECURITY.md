# Security

Mash stores your agent chats, which can hold credentials and private work.
Problems that expose that history are taken seriously.

## Report a vulnerability

Report it privately through
[GitHub's vulnerability reporting](https://github.com/mashium/mash/security/advisories/new).
Do not open a public issue for a security problem.

Include the Mash version (`mash --version`), your platform, and the steps that
show the problem. Leave out real chat content, tokens and other secrets; a
made-up example that reproduces the problem is enough.

You should hear back within a week. Fixes ship in the next release, and the
advisory is published once a fixed release is available.

## What counts

- Reading or changing another user's history through the shared server.
- Bypassing the shared server's or dashboard's authentication.
- Credentials that Mash's masking is documented to catch reaching search
  results or shared history unmasked.
- Mash writing to an agent's own history files.
- An installer or package that does not match its published checksum.

Masking is best effort by design, and a dashboard without a login is expected
on its default local address; see [privacy](docs/privacy.md). These are worth
an ordinary issue rather than a private report.

## Supported versions

Only the latest release receives security fixes.
