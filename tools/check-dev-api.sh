#!/usr/bin/env bash
set -euo pipefail
[[ $# == 5 ]] || { echo 'usage: check-dev-api.sh ENV POINTER API_URL TOKEN_ENV LOG_ROOT' >&2; exit 2; }
source "$(dirname "$0")/dev-api-common.sh"
load_api_environment "$1" "$4"
verify_api_process "$2"
mkdir -p "$5"
scratch=$(mktemp -d "$5/api-check.XXXXXX")
check_api_http "$3" "$scratch"
jq '{source_commit, binary_sha256, pid, authenticated_readiness: true}' "$2"
