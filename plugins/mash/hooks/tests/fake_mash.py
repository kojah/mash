"""A stand-in Mash executable for hook tests, driven by a JSON fixture.

FAKE_MASH_FIXTURE names a JSON file:
    {"chats": [...list-chats rows...],
     "messages": [{"id", "content", "created_at"}...],   # view order
     "revision": 0, "batches": [], "fail": null, "sleep": 0}
`index --connector-batch` appends the batch and bumps the revision, like silver.
"""

import json
import os
import sys
import time
from pathlib import Path


def main() -> int:
    fixture = Path(os.environ["FAKE_MASH_FIXTURE"])
    data = json.loads(fixture.read_text())
    time.sleep(data.get("sleep", 0))
    arguments = sys.argv[1:]
    command = arguments[0]
    if (data.get("fail") or {}).get(command):
        print(json.dumps({"error": {"message": "fake failure"}}))
        return int(data["fail"][command])
    if command == "list-chats":
        offset = int(arguments[arguments.index("--offset") + 1])
        limit = int(arguments[arguments.index("--limit") + 1])
        chats = data["chats"][offset : offset + limit]
        more = offset + limit < len(data["chats"])
        print(
            json.dumps(
                {"chats": chats, "next_offset": offset + limit if more else None}
            )
        )
        return 0
    if command == "view":
        target = arguments[1]
        context = int(arguments[arguments.index("--context") + 1])
        messages = data["messages"]
        ids = [message["id"] for message in messages]
        if target not in ids:
            print(json.dumps({"error": {"message": "message not found"}}))
            return 3
        index = ids.index(target)
        window = messages[max(0, index - context) : index + context + 1]
        print(json.dumps({"id": target, "messages": window}))
        return 0
    if command == "index":
        batch = json.loads(
            Path(arguments[arguments.index("--connector-batch") + 1]).read_text()
        )
        for previous in data["batches"]:
            if previous["batch_id"] == batch["batch_id"]:
                print(
                    json.dumps({"source_revision": previous["expected_revision"] + 1})
                )
                return 0
        if batch["expected_revision"] != data["revision"]:
            print(
                json.dumps(
                    {
                        "error": {
                            "message": "connector ingestion failed: RevisionConflict"
                        }
                    }
                )
            )
            return 12
        data["revision"] += 1
        data["batches"].append(batch)
        fixture.write_text(json.dumps(data))
        print(json.dumps({"source_revision": data["revision"], "views_ready": False}))
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
