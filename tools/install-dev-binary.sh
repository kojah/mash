#!/usr/bin/env bash
set -euo pipefail

# Install a validated artifact without touching indexes or running processes.
# Build/validation and any API restart belong to the configured workflow.
if [[ $# != 2 || "$2" != /* ]]; then
    echo 'usage: install-dev-binary.sh ARTIFACT ABSOLUTE_DESTINATION' >&2
    exit 2
fi
artifact=$1
destination=$2
[[ -f "$artifact" && -x "$artifact" ]]
if [[ -f "$destination" && -x "$destination" ]] && cmp -s -- "$artifact" "$destination"; then
    exit 0
fi
mkdir -p -- "$(dirname -- "$destination")"
incoming=$(mktemp "${destination}.incoming.XXXXXX")
trap 'rm -f -- "$incoming"' EXIT
cp -- "$artifact" "$incoming"
chmod 755 "$incoming"
cmp -- "$artifact" "$incoming"
mv -f -- "$incoming" "$destination"
