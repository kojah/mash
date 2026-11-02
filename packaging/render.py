"""Fill a packaging template for one release from its SHA256SUMS file.

    python3 packaging/render.py 0.8.0 dist/SHA256SUMS packaging/scoop/mash.json.in bucket/mash.json

Templates name values as @VERSION@ and @SHA256:<archive>@ (lowercase hex) or
@SHA256_UPPER:<archive>@. Standard library only, for stock release runners.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

TOKEN = re.compile(r"@(VERSION|SHA256|SHA256_UPPER)(?::([\w.-]+))?@")


def render(template: str, version: str, sums: str) -> str:
    digests = {}
    for line in sums.splitlines():
        digest, _, name = line.strip().partition("  ")
        digests[name.lstrip("*")] = digest.lower()

    def value(match: re.Match[str]) -> str:
        kind, archive = match.group(1), match.group(2)
        if kind == "VERSION":
            return version
        if archive not in digests:
            raise SystemExit(f"SHA256SUMS lacks {archive}")
        digest = digests[archive]
        return digest.upper() if kind == "SHA256_UPPER" else digest

    return TOKEN.sub(value, template)


def main() -> int:
    version, sums, template, output = sys.argv[1:5]
    text = render(
        Path(template).read_text(), version.removeprefix("v"), Path(sums).read_text()
    )
    Path(output).parent.mkdir(parents=True, exist_ok=True)
    Path(output).write_text(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
