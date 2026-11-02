"""Render the Homebrew formula for a release from its SHA256SUMS file.

    python3 packaging/homebrew/render.py 0.7.0 dist/SHA256SUMS > mash.rb

Standard library only; the release workflow runs it on a stock runner.
"""

from __future__ import annotations

import sys
from pathlib import Path

TARGETS = {
    "macos_arm": "aarch64-apple-darwin",
    "linux_intel": "x86_64-unknown-linux-gnu",
    "linux_arm": "aarch64-unknown-linux-gnu",
}
TEMPLATE = Path(__file__).with_name("mash.rb.in")


def render(version: str, sums: str) -> str:
    digests = {}
    for line in sums.splitlines():
        digest, _, name = line.strip().partition("  ")
        digests[name.lstrip("*")] = digest
    values = {"version": version}
    for key, target in TARGETS.items():
        archive = f"mash-{target}.tar.gz"
        if archive not in digests:
            raise SystemExit(f"SHA256SUMS lacks {archive}")
        values[key] = digests[archive]
    return TEMPLATE.read_text().format(**values)


def main() -> int:
    version, sums = sys.argv[1], Path(sys.argv[2]).read_text()
    sys.stdout.write(render(version.removeprefix("v"), sums))
    return 0


if __name__ == "__main__":
    sys.exit(main())
