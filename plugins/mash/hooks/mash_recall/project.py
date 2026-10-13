"""Resolve the session's project the way Mash records chats: a working directory
and, when present, the Git origin URL (`git config --get remote.origin.url`)."""

from __future__ import annotations

import hashlib
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .client import Chat


@dataclass(frozen=True)
class Project:
    toplevel: str
    repository: str | None

    @property
    def label(self) -> str:
        return self.repository or self.toplevel

    @property
    def key(self) -> str:
        """Stable identity: the origin URL shared by every clone and worktree,
        else the checkout path."""
        identity = (
            f"repository\0{self.repository}"
            if self.repository
            else f"path\0{self.toplevel}"
        )
        return hashlib.sha256(identity.encode()).hexdigest()[:24]

    def owns(self, chat: Chat) -> bool:
        if self.repository and chat.repository == self.repository:
            return True
        if chat.project is None:
            return False
        return chat.project == self.toplevel or chat.project.startswith(
            self.toplevel.rstrip("/") + "/"
        )


def git(directory: str, arguments: list[str], timeout: float) -> str | None:
    if timeout <= 0:
        return None
    try:
        result = subprocess.run(
            ["git", "-C", directory, *arguments],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    text = result.stdout.decode("utf-8", "replace").strip()
    return text or None


def resolve(cwd: str, timeout: float) -> Project | None:
    """The Git toplevel and origin of `cwd`; a non-Git directory is its own project."""
    if not cwd or not Path(cwd).is_absolute() or not Path(cwd).is_dir():
        return None
    toplevel = git(cwd, ["rev-parse", "--show-toplevel"], timeout / 2) or cwd
    repository = git(toplevel, ["config", "--get", "remote.origin.url"], timeout / 2)
    return Project(toplevel=toplevel, repository=repository)
