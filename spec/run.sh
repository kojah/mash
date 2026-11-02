#!/usr/bin/env bash
# Model-check the publication protocol, including the two broken variants.
set -euo pipefail
cd "$(dirname "$0")"

JAR=${TLA_JAR:-$PWD/tla2tools.jar}
if [ ! -f "$JAR" ]; then
  echo "downloading tla2tools.jar"
  curl -fsSL -o "$JAR" \
    https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar
fi

run() {
  local config=$1
  local module=${MODULE:-Publication}
  local log
  log=$(mktemp)
  if java -jar "$JAR" -workers 4 -config "$config" "$module.tla" >"$log" 2>&1; then
    echo "  ran    $config"
  else
    echo "  ran    $config (reported a violation)"
  fi
  cat "$log"
  rm -f "$log"
}

module_for() {
  case "$1" in
    coherence-*) echo Coherence ;;
    *) echo Publication ;;
  esac
}

expect_clean() {
  local config=$1 log
  MODULE=$(module_for "$config") log=$(run "$config")
  if ! grep -q "No error has been found" <<<"$log"; then
    echo "FAIL: $config was expected to be clean"
    tail -20 <<<"$log"
    exit 1
  fi
  echo "ok: $config is clean"
}

expect_violation() {
  local config=$1 invariant=$2 log
  MODULE=$(module_for "$config") log=$(run "$config")
  if ! grep -q "Invariant $invariant is violated" <<<"$log"; then
    echo "FAIL: $config was expected to violate $invariant"
    tail -20 <<<"$log"
    exit 1
  fi
  echo "ok: $config violates $invariant as intended"
}

expect_clean ok.cfg
expect_clean sequential.cfg
expect_violation rebase-bug.cfg OrderedRevisions
expect_violation scope-bug.cfg UniqueBackfillIdentity

expect_clean coherence-ok.cfg
expect_violation coherence-device-scope.cfg NoDoubleIngest
expect_violation coherence-overwrite.cfg NoDoubleIngest

echo "all model checks agree with the implementation"
