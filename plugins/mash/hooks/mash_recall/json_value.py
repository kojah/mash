"""Typed reads from decoded JSON, whose shape is untrusted until checked."""

from __future__ import annotations

from typing import cast


def mapping(value: object) -> dict[str, object] | None:
    return cast("dict[str, object]", value) if isinstance(value, dict) else None


def at(value: object, *keys: str) -> object:
    """The JSON member at `keys`, or None when any step is missing."""
    for key in keys:
        members = mapping(value)
        if members is None:
            return None
        value = members.get(key)
    return value


def text_at(value: object, *keys: str) -> str | None:
    member = at(value, *keys)
    return member if isinstance(member, str) and member else None


def int_at(value: object, *keys: str) -> int | None:
    member = at(value, *keys)
    is_int = isinstance(member, int) and not isinstance(member, bool)
    return member if is_int else None


def items(value: object) -> list[object]:
    return list(cast("list[object]", value)) if isinstance(value, list) else []
