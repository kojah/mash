#!/usr/bin/env bash
# Rust owns API names, examples, constraints and the release stamp.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
output_dir=plugins/mash/skills/mash-history
baseline=${MASH_CONTRACT_BASE:-HEAD}
scratch_root=${TMPDIR:-${RUNNER_TEMP:-${XDG_CACHE_HOME:-$HOME/.cache}/mash-codegen}}
mkdir -p "$scratch_root"
scratch_dir=$(mktemp -d "$scratch_root/history-skill.XXXXXXXX")
trap 'rm -rf "$scratch_dir"' EXIT
baseline_file=()
# Released stamps may precede the skill-folder rename.
for stamp in "$output_dir/contract.json" plugins/mash/skills/history/contract.json; do
    if git cat-file -e "$baseline:$stamp" 2>/dev/null; then
        git show "$baseline:$stamp" > "$scratch_dir/baseline.json"
        baseline_file=("$scratch_dir/baseline.json")
        break
    fi
done
skill_cargo_run=${MASH_SKILL_CARGO_RUN:-run-clif}
case "$skill_cargo_run" in
    run|run-clif) ;;
    *) echo 'MASH_SKILL_CARGO_RUN must be run or run-clif' >&2; exit 2 ;;
esac
cargo "$skill_cargo_run" --locked --example history-skill -- "$scratch_dir/generated" "${baseline_file[@]}"
if [[ ${1:-} == --check ]]; then
    diff -u "$output_dir/SKILL.md" "$scratch_dir/generated/SKILL.md"
    diff -u "$output_dir/contract.json" "$scratch_dir/generated/contract.json"
else
    cp "$scratch_dir/generated/SKILL.md" "$output_dir/SKILL.md"
    cp "$scratch_dir/generated/contract.json" "$output_dir/contract.json"
fi
