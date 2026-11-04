# Connector blueprints

All external connectors are experimental blueprints. None is currently supported
for public use. Promote each connector out of this directory only after its
source behavior and end-to-end delivery have been tested sufficiently.

Packaged implementations live in `obsidian/`, `github/`, `slack/`,
and `discord/`. Their READMEs describe setup and checks; existing CI checks stay
enabled, but fake API tests do not establish live service readiness.

The sibling Python files sketch additional integrations and desired APIs. They
use the typing specification in `../api/`; shared selection helpers and their
unit tests remain in the parent directory. The runnable connector SDK remains
in [`sdk/python`](../../sdk/python/README.md).

No connector is loaded or executed by the Mash server.
