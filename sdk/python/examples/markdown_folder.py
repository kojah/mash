"""A complete connector: every Markdown file in a folder becomes a document.

Run it with a folder, a Mash index and a state file:

    uv run python examples/markdown_folder.py \
        ~/notes ~/.local/share/mash/index.sqlite3 notes-state.sqlite

It polls once a minute. Edited notes are resent, unchanged ones are skipped,
and a deleted file is deleted from Mash on the next poll. Notes become
searchable after Mash's next `mash index`. Press Ctrl-C to stop.
"""

import asyncio
import hashlib
import sys
from pathlib import Path

from mash_sdk import (
    Changes,
    Connector,
    Destination,
    DocumentSnapshot,
    KindRule,
    Mode,
    Object,
    Role,
)

SOURCE_ID = "markdown:notes"
NOTE = "markdown:note"
POLL_SECONDS = 60


def snapshot(folder: Path) -> DocumentSnapshot:
    """The folder as it is now; `complete` lets missing files become deletes."""
    notes = []
    for path in sorted(folder.rglob("*.md")):
        relative = path.relative_to(folder).as_posix()
        notes.append(
            Object(
                # A path is a stable enough identity for a plain folder.
                id=hashlib.sha256(relative.encode()).hexdigest(),
                role=Role.document,
                kind=NOTE,
                title=path.stem,
                content=path.read_text(encoding="utf-8", errors="replace"),
                # Object URLs must be http(s); keep the file's place as metadata.
                metadata={"path": relative},
            )
        )
    return DocumentSnapshot(documents=notes, scope_ids=["folder"], complete=True)


async def main(
    folder: Path, destination: Destination, state: Path, once: bool = False
) -> None:
    async with Connector(
        source_id=SOURCE_ID,
        # A folder is polled: each sync describes it as it is now.
        modes=[Mode.snapshot],
        destinations=[destination],
        state_path=state,
    ) as connector:
        # Declaring the topology makes Mash reject anything else from this source.
        await connector.send(
            Changes.declare(KindRule(kind=NOTE, role=Role.document, root=True))
        )
        while True:
            await connector.sync(snapshot(folder))
            if once:
                return
            await asyncio.sleep(POLL_SECONDS)


if __name__ == "__main__":
    folder, database, state = (Path(argument) for argument in sys.argv[1:4])
    asyncio.run(main(folder, Destination.local(database=database), state))
