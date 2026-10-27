"""Desired Obsidian adapter: Markdown files become standalone Documents."""

from dataclasses import dataclass
from pathlib import Path

from .scope import Scope


@dataclass(frozen=True)
class Settings:
    vault_path: Path
    vault_id: str
    scope: Scope
    include_globs: tuple[str, ...] = ("**/*.md",)
    exclude_globs: tuple[str, ...] = (".obsidian/**", ".trash/**", ".git/**")


# The source adapter implements notes.Notes:
# - Scope IDs are vault-relative folder paths; recursive selection is explicit.
# - Read complete Markdown, title/frontmatter/tags, and resolve attachment references.
# - Preserve identity across renames with a durable source-side identity registry;
#   a filename alone is not a stable ID, and existing notes must not be rewritten.
# - Treat atomic file replacement as an update, and reconcile before confirming deletion.
# - Watch the vault, constrain resolved paths to it, and handle symlinks deliberately.
# - Preserve unknown creation time. A bounded creation-time window cannot silently
#   substitute filesystem mtime, which changes when a note is edited or copied.
# - Feed notes.run(source, source_id=f"obsidian:{settings.vault_id}", ...).
