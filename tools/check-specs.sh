#!/usr/bin/env bash
# Run every model in protocol/ under TLC.
#
# Each .cfg is checked against the .tla whose basename is the longest prefix of
# the config name (Publication.cfg -> Publication.tla, ProgressiveBackfillResurrection.cfg
# -> ProgressiveBackfill.tla).
#
# Configurations that model a *rejected* behaviour must report a violation; the
# rest must be clean. Both directions are enforced, so a model that stops
# catching its bug fails the check just like a broken model.
set -uo pipefail
cd "$(dirname "$0")/../protocol"

JAR=${TLA_JAR:-$PWD/tla2tools.jar}
if [ ! -f "$JAR" ]; then
  echo "downloading tla2tools.jar"
  curl -fsSL -o "$JAR" \
    https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar
fi

# Configurations whose bug is the point: they must violate an invariant.
BROKEN='AckLatest|DatabaseFirst|RetireFirst|Unfair|NoTombstones|EarlyFinish|Resurrection|SparseAck|StaleWrite|LostInsert|StaleEvidence|WrongPayload|ChangedPayload|ReleaseWorker|TwoSlots|DeviceScope|Overwrite|RebaseBug|ScopeBug|LanesBug|SeqNoLanes'

module_for() {
  local cfg=${1%.cfg} best="" name
  for tla in *.tla; do
    name=${tla%.tla}
    case "$cfg" in
      "$name"*) [ ${#name} -gt ${#best} ] && best=$name ;;
    esac
  done
  echo "$best"
}

failed=0
summary=()
for cfg in *.cfg; do
  module=$(module_for "$cfg")
  if [ -z "$module" ]; then
    echo "FAIL $cfg: no matching .tla"
    failed=1
    continue
  fi
  log=$(mktemp)
  java -jar "$JAR" -workers 4 -nowarning -config "$cfg" "$module.tla" >"$log" 2>&1
  if grep -q "No error has been found" "$log"; then
    outcome=clean
  elif grep -q "is violated" "$log"; then
    outcome=violation
  else
    outcome=error
  fi

  expect=clean
  if [[ "$cfg" =~ $BROKEN ]]; then expect=violation; fi

  if [ "$outcome" = "$expect" ]; then
    summary+=("ok   $cfg ($module, $outcome)")
  else
    summary+=("FAIL $cfg: expected $expect, got $outcome")
    echo "----- $cfg -----"
    grep -E "Error|violated|line [0-9]+" "$log" | head -6
    failed=1
  fi
  rm -f "$log"
done

printf '%s\n' "${summary[@]}"
exit $failed
