"""Opaque metadata retains JSON types while its keys remain source-owned."""

from typing import TypeAlias

from pydantic import JsonValue

Metadata: TypeAlias = dict[str, JsonValue]
