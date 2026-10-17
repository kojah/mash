#!/usr/bin/env bash
set -euo pipefail
deployment=false
if [[ ${1:-} == --deployment ]]; then deployment=true; shift; fi
[[ $# == 5 ]] || { echo 'usage: check-dev-api.sh [--deployment] ENV POINTER API_URL TOKEN_ENV LOG_ROOT' >&2; exit 2; }
source "$(dirname "$0")/dev-api-common.sh"
load_api_environment "$1" "$4"
verify_api_process "$2"
if [[ $deployment == true ]]; then
    [[ -w "$2" && -w "$(dirname "$2")" && -d "$5" && -w "$5" ]] || {
        echo 'API release record or installation log directory is not writable.' >&2
        exit 1
    }
fi
mkdir -p "$5"
scratch=$(mktemp -d "$5/api-check.XXXXXX")
check_api_http "$3" "$scratch"
jq '{source_commit, binary_sha256, pid, authenticated_readiness: true}' "$2"
