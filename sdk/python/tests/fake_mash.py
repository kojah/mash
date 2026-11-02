"""Test support: a fake mash executable and a view of the state it writes.

Delivery runs as a subprocess now, so the HTTP ingress fake is gone; tests read
the same file the executable updates and inject failures as exit codes.
"""

import hashlib
from pathlib import Path

import orjson

TOKEN = "test-token-first-32-bytes-long-xx"
TOKEN_ENV = "MASH_SDK_TEST_SHARED_TOKEN"
STATE_DIR_ENV = "MASH_FAKE_STATE_DIR"


def install_executable(directory: Path) -> Path:
    """Copy the fake `mash` next to the test's state files."""
    source = Path(__file__).with_name("fake_mash_executable.py")
    target = directory / "mash"
    target.write_bytes(source.read_bytes())
    target.chmod(0o755)
    return target


class FileBehaviour:
    """The same view of state, backed by the file the fake executable writes.

    A subprocess cannot mutate a Python object, so every read reloads the file
    and every injected failure is persisted for the next invocation.
    """

    def __init__(self, directory: Path, api_url: str) -> None:
        digest = hashlib.sha256(api_url.encode()).hexdigest()[:16]
        self.path = directory / f"{digest}.json"
        self._write(
            {
                "revision": 0,
                "silver": 0,
                "receipts": {},
                "commits": [],
                "failures": [],
                "objects": {},
                "requests": [],
            }
        )

    def _read(self) -> dict:
        return orjson.loads(self.path.read_bytes())

    def _write(self, state: dict) -> None:
        self.path.write_bytes(orjson.dumps(state))

    @property
    def objects(self) -> dict[str, dict[str, object]]:
        return self._read()["objects"]

    def by_role(self, role: str) -> dict[str, dict[str, object]]:
        return {
            id: value for id, value in self.objects.items() if value["role"] == role
        }

    @property
    def chats(self) -> dict[str, dict[str, object]]:
        return self.by_role("container")

    @property
    def messages(self) -> dict[str, dict[str, object]]:
        return self.by_role("entry")

    @property
    def documents(self) -> dict[str, dict[str, object]]:
        return self.by_role("document")

    @property
    def commits(self) -> list[bytes]:
        return [orjson.dumps(commit) for commit in self._read()["commits"]]

    @property
    def failures(self) -> list[int]:
        return self._read()["failures"]

    @failures.setter
    def failures(self, values: list[int]) -> None:
        state = self._read()
        state["failures"] = list(values)
        self._write(state)
