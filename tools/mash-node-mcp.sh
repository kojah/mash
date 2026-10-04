#!/usr/bin/env bash
set -euo pipefail
set -a
source "$HOME/.config/mash/node.env"
set +a
exec "$HOME/.local/bin/mash" mcp "$@"
