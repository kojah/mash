#!/usr/bin/env bash
set -euo pipefail
repository=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:?}/controller-environment-test.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir "$scratch/bin"
cat > "$scratch/bin/uv" <<'UV'
#!/usr/bin/env bash
printf '%s\n' "$UV_PROJECT_ENVIRONMENT" "$*" >> "$TEST_CONTROLLER_CALLS"
exit "${TEST_CONTROLLER_EXIT:-0}"
UV
chmod 755 "$scratch/bin/uv"
export PATH="$scratch/bin:$PATH" TEST_CONTROLLER_CALLS="$scratch/calls"
env -u UV_PROJECT_ENVIRONMENT bash -eu -c 'source "$1"' _ "$repository/tools/controller-environment.sh"
grep -Fxq '/home/james/scratch/mash-artifacts/ansible-venv' "$scratch/calls"
UV_PROJECT_ENVIRONMENT="$scratch/override" bash -eu -c 'source "$1"' _ "$repository/tools/controller-environment.sh"
grep -Fxq "$scratch/override" "$scratch/calls"
grep -Fq -- '--locked --quiet' "$scratch/calls"
if TEST_CONTROLLER_EXIT=42 bash -eu -c 'source "$1"; touch "$2"' _ \
    "$repository/tools/controller-environment.sh" "$scratch/continued"; then
    echo 'Controller setup failure was accepted.' >&2; exit 1
fi
[[ ! -e "$scratch/continued" ]]
echo 'Controller runtime selection, locked setup and failure checks passed.'
