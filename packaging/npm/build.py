"""Lay out the npm packages for one release from its archives.

    python3 packaging/npm/build.py 0.8.0 dist npm-out

`@kojah/mash` holds a small launcher; each `@kojah/mash-<os>-<cpu>` package
holds one platform's binary and declares `os`/`cpu`, so npm installs only the
matching one as an optional dependency (the esbuild/Biome layout). Publish
the platform packages first. Standard library only.
"""

from __future__ import annotations

import json
import shutil
import sys
import tarfile
import zipfile
from pathlib import Path

SCOPE = "@kojah"
# Rust target -> (npm os, npm cpu, archive extension, binary name)
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux", "x64", "tar.gz", "mash"),
    "aarch64-unknown-linux-gnu": ("linux", "arm64", "tar.gz", "mash"),
    "aarch64-apple-darwin": ("darwin", "arm64", "tar.gz", "mash"),
    "x86_64-pc-windows-msvc": ("win32", "x64", "zip", "mash.exe"),
}
COMMON = {
    "license": "MIT",
    "homepage": "https://github.com/kojah/mash",
    "repository": {"type": "git", "url": "git+https://github.com/kojah/mash.git"},
}
LAUNCHER = """#!/usr/bin/env node
// Runs the platform binary that npm installed as an optional dependency.
const { spawnSync } = require("node:child_process");
const name = `@kojah/mash-${process.platform}-${process.arch}`;
const binary = process.platform === "win32" ? "mash.exe" : "mash";
let path;
try {
  path = require.resolve(`${name}/bin/${binary}`);
} catch {
  console.error(`mash: no prebuilt binary for ${process.platform} ${process.arch} (${name}).`);
  process.exit(1);
}
const result = spawnSync(path, process.argv.slice(2), { stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
"""


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def extract(archive: Path, binary: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            destination.write_bytes(source.read(binary))
    else:
        with tarfile.open(archive) as source:
            member = source.extractfile(binary)
            if member is None:
                raise SystemExit(f"{archive} lacks {binary}")
            destination.write_bytes(member.read())
    destination.chmod(0o755)


def main() -> int:
    version, dist, out = (
        sys.argv[1].removeprefix("v"),
        Path(sys.argv[2]),
        Path(sys.argv[3]),
    )
    shutil.rmtree(out, ignore_errors=True)
    platforms = {}
    for target, (os, cpu, extension, binary) in TARGETS.items():
        name = f"{SCOPE}/mash-{os}-{cpu}"
        directory = out / f"mash-{os}-{cpu}"
        extract(dist / f"mash-{target}.{extension}", binary, directory / "bin" / binary)
        write_json(
            directory / "package.json",
            {
                "name": name,
                "version": version,
                "description": f"The mash binary for {os} {cpu}.",
                "os": [os],
                "cpu": [cpu],
                "files": ["bin"],
                **COMMON,
            },
        )
        platforms[name] = version
    main_directory = out / "mash"
    (main_directory / "bin").mkdir(parents=True)
    (main_directory / "bin" / "mash.js").write_text(LAUNCHER)
    write_json(
        main_directory / "package.json",
        {
            "name": f"{SCOPE}/mash",
            "version": version,
            "description": "Search your coding agents' past conversations "
            "from a CLI and MCP server.",
            "bin": {"mash": "bin/mash.js"},
            "files": ["bin"],
            "optionalDependencies": platforms,
            "engines": {"node": ">=18"},
            **COMMON,
        },
    )
    shutil.copy(Path(__file__).resolve().parents[2] / "LICENSE", main_directory)
    return 0


if __name__ == "__main__":
    sys.exit(main())
