#!/bin/sh
# Run a hook script with the first working Python 3.9+: python3, python
# (Windows installs), then the Windows py launcher. Windows can ship a
# Microsoft Store python3 stub that only prints an install hint, so each
# candidate must actually run. With none, exit 0: hooks fail open.
for interpreter in python3 python; do
  if "$interpreter" -c 'import sys; sys.exit(sys.version_info < (3, 9))' >/dev/null 2>&1; then
    exec "$interpreter" "$@"
  fi
done
if py -3 -c 'import sys; sys.exit(sys.version_info < (3, 9))' >/dev/null 2>&1; then
  exec py -3 "$@"
fi
exit 0
