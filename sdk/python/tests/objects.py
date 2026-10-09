"""Test helpers: objects with fixed roles and kind labels."""

from mash_sdk import Object, Role


def channel(**fields: object) -> Object:
    return Object.model_validate(
        {"role": Role.container, "kind": "chat:channel", **fields}
    )


def segment(**fields: object) -> Object:
    return Object.model_validate({"role": Role.segment, "kind": "chat:turn", **fields})


def message(**fields: object) -> Object:
    return Object.model_validate({"role": Role.entry, "kind": "chat:message", **fields})


def note(**fields: object) -> Object:
    return Object.model_validate(
        {"role": Role.document, "kind": "notes:note", **fields}
    )
