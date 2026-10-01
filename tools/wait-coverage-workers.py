#!/usr/bin/env python3
"""Await idle workers from this coverage target before cleaning/exporting profiles."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def workers(target):
    if os.name != "posix":
        return []  # The search worker is compiled only on Unix.
    result = subprocess.run(
        ["ps", "-ww", "-u", str(os.getuid()), "-o", "pid=,command="],
        check=True, capture_output=True, text=True,
    )
    found = []
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) != 2:
            continue
        executable, marker, _ = fields[1].partition(" --internal-search-worker ")
        if not marker:
            continue
        path = Path(executable)
        if path.name == "mash" and path.resolve().is_relative_to(target.resolve()):
            found.append(int(fields[0]))
    return found


def wait(target, timeout):
    deadline = time.monotonic() + timeout
    announced = False
    while current := workers(target):
        if not announced:
            print(f"Waiting for {len(current)} profiled search workers to exit normally", flush=True)
            announced = True
        if time.monotonic() >= deadline:
            raise TimeoutError("profiled search workers remain active; coverage export aborted")
        time.sleep(min(1, max(0, deadline - time.monotonic())))
    if announced:
        print("Profiled search workers exited", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--timeout", type=float, default=125)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    target = args.target_dir
    if target is None:
        metadata = subprocess.run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            check=True, capture_output=True, text=True,
        )
        target = Path(json.loads(metadata.stdout)["target_directory"])
    wait(target, args.timeout)


if __name__ == "__main__":
    main()
