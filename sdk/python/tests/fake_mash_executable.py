#!/usr/bin/env python3
"""Stand-in for `mash index --connector-batch ... --shared` in SDK tests.

The destination's api_url selects a state file under MASH_FAKE_STATE_DIR, so
two destinations in one test stay independent. Injected failures are exit
codes: 11 is retryable (capacity/unavailable), 2 is permanent.
"""

import hashlib
import json
import os
import pathlib
import re
import sys

DIRECTORY = pathlib.Path(os.environ["MASH_FAKE_STATE_DIR"])


def state_path(arguments) -> pathlib.Path:
    config = pathlib.Path(arguments[arguments.index("--config") + 1]).read_text()
    match = re.search(r'api_url = "(.*)"', config)
    if match is None:
        raise SystemExit("fake mash: configuration has no api_url")
    api_url = match.group(1)
    digest = hashlib.sha256(api_url.encode()).hexdigest()[:16]
    return DIRECTORY / f"{digest}.json"


def load(path):
    return json.loads(path.read_text())


def save(path, state):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(state))
    temporary.replace(path)


def apply(objects, changes):
    for change in changes:
        object_id = str(change["id"])
        if change["op"] == "upsert-object":
            objects[object_id] = change
            continue
        doomed = {object_id}
        while True:
            children = {
                key
                for key, value in objects.items()
                if value.get("parent") in doomed and key not in doomed
            }
            if not children:
                break
            doomed |= children
        for key in doomed:
            objects.pop(key, None)


def main() -> int:
    arguments = sys.argv
    path = state_path(arguments)
    batch = json.loads(
        pathlib.Path(arguments[arguments.index("--connector-batch") + 1]).read_text()
    )
    state = load(path)
    state["requests"].append(arguments)
    delay = float(os.environ.get("FAKE_MASH_SLEEP", "0") or 0)
    if delay:
        import time

        time.sleep(delay)

    forced = os.environ.get("FAKE_MASH_EXIT")
    if forced:
        save(path, state)
        return int(forced)
    if state["failures"]:
        status = state["failures"].pop(0)
        save(path, state)
        return 11 if status in {429, 500, 502, 503, 504} else 2

    batch_id = str(batch["batch_id"])
    if batch_id in state["receipts"]:
        save(path, state)
        print(json.dumps(state["receipts"][batch_id]))
        return 0

    apply(state["objects"], batch["changes"])
    state["revision"] += 1
    state["silver"] += 1
    state["commits"].append(batch)
    receipt = {
        "schema_version": 2,
        "source_id": batch["source_id"],
        "batch_id": batch_id,
        "source_revision": state["revision"],
        "silver_revision": state["silver"],
        "views_ready": True,
    }
    state["receipts"][batch_id] = receipt
    save(path, state)
    print(json.dumps(receipt))
    return 0


if __name__ == "__main__":
    sys.exit(main())
